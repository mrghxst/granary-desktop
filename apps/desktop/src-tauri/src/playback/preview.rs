//! Seek-bar previews for direct-played files without server trickplay (ADR-0009). One headless libmpv handle per
//! source, opened on first hover and dropped when playback moves on; frames are cached per two-second bucket so
//! sweeping back and forth over the seek bar decodes each spot once.
use super::host::{Cmd, PlayerHost};
use crate::settings::SettingsStore;
use crate::silo::CmdResult;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

/// Seconds per cache bucket (keyframes are rarely closer than this).
const BUCKET: f64 = 2.0;
/// Frames kept per source (about 10 KB each).
const CACHE: usize = 400;

#[derive(Default)]
pub struct PreviewState {
    inner: Mutex<Previews>,
}

#[derive(Default)]
struct Previews {
    source: String,
    #[cfg(windows)]
    thumb: Option<granary_player::thumbs::Thumbnailer>,
    /// The source could not be opened: do not retry it on every hover.
    failed: bool,
    cache: HashMap<u64, Arc<String>>,
    order: VecDeque<u64>,
}

impl Previews {
    fn reset(&mut self, source: &str) {
        *self = Self {
            source: source.to_owned(),
            ..Self::default()
        };
    }

    fn remember(&mut self, bucket: u64, url: Arc<String>) {
        if self.cache.insert(bucket, url).is_none() {
            self.order.push_back(bucket);
            while self.order.len() > CACHE {
                if let Some(old) = self.order.pop_front() {
                    self.cache.remove(&old);
                }
            }
        }
    }
}

fn bucket(seconds: f64) -> Option<u64> {
    // Finite and non-negative: the cast below cannot wrap.
    (seconds.is_finite() && seconds >= 0.0).then(|| (seconds / BUCKET).floor() as u64)
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18u32, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(char::from(B64[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A JPEG data URL of the frame near `seconds` in the playing file, or `None` when previews are off, nothing
/// previewable is playing (transcode, stopped) or the frame could not be decoded.
#[tauri::command]
pub async fn player_preview(
    app: AppHandle,
    host: State<'_, PlayerHost>,
    settings: State<'_, SettingsStore>,
    seconds: f64,
) -> CmdResult<Option<String>> {
    if !settings.current().player.seek_previews {
        return Ok(None);
    }
    let Some(b) = bucket(seconds) else { return Ok(None) };
    let Some(source) = host.ask(Cmd::PreviewSource).await.flatten() else {
        // Nothing previewable: release the handle and its connection.
        if let Ok(mut p) = app.state::<PreviewState>().inner.lock() {
            p.reset("");
        }
        return Ok(None);
    };
    let at = b as f64 * BUCKET;
    let result = tauri::async_runtime::spawn_blocking(move || frame(&app, &source, b, at)).await;
    Ok(result.ok().flatten().map(|s| (*s).clone()))
}

#[cfg(windows)]
fn frame(app: &AppHandle, source: &str, bucket: u64, at: f64) -> Option<Arc<String>> {
    use granary_player::thumbs::Thumbnailer;
    use std::time::Duration;
    let state = app.state::<PreviewState>();
    // One decode at a time; a poisoned lock only means an earlier decode panicked, the cache is still consistent.
    let mut p = state.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if p.source != source {
        p.reset(source);
    }
    if let Some(hit) = p.cache.get(&bucket) {
        return Some(Arc::clone(hit));
    }
    if p.failed {
        return None;
    }
    if p.thumb.is_none() {
        let dir = std::env::temp_dir().join("granary-previews");
        match Thumbnailer::open(source, &dir, Duration::from_secs(12)) {
            Ok(t) => p.thumb = Some(t),
            Err(e) => {
                tracing::warn!(error = %e, "seek previews unavailable for this file");
                p.failed = true;
                return None;
            }
        }
    }
    let jpg = p.thumb.as_ref()?.frame(at, Duration::from_secs(6));
    match jpg {
        Ok(bytes) => {
            let url = Arc::new(format!("data:image/jpeg;base64,{}", base64(&bytes)));
            p.remember(bucket, Arc::clone(&url));
            Some(url)
        }
        Err(e) => {
            tracing::debug!(error = %e, at, "seek preview frame failed");
            None
        }
    }
}

#[cfg(not(windows))]
fn frame(_app: &AppHandle, _source: &str, _bucket: u64, _at: f64) -> Option<Arc<String>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648_vectors() {
        for (input, want) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), want);
        }
    }

    #[test]
    fn buckets_and_cache_bound() {
        assert_eq!(bucket(0.0), Some(0));
        assert_eq!(bucket(3.9), Some(1));
        assert_eq!(bucket(-1.0), None);
        assert_eq!(bucket(f64::NAN), None);
        let mut p = Previews::default();
        for i in 0..(CACHE as u64 + 10) {
            p.remember(i, Arc::new(String::new()));
        }
        assert_eq!(p.cache.len(), CACHE);
        assert!(!p.cache.contains_key(&0));
        p.reset("x");
        assert!(p.cache.is_empty() && p.source == "x");
    }
}
