//! Optional seek-bar previews from Seekr (seekr.tv), a third-party service with the user's own API key (ADR-0011).
//!
//! Contract (Seekr developer docs, provided by the owner as a PDF, 2026-11-08):
//! - `GET https://api.seekr.tv/sprites?duration_ms=…&<one identifier>` with `X-API-Key`; identifiers: `tmdb_id`,
//!   `imdb_id`, or `show_tmdb_id` / `show_imdb_id` with `season` + `episode`. Answers `{vtt_url, scale, …}`.
//! - The `vtt_url` and the tile sheets need no key. Each WebVTT cue is a time range and a sheet URL with
//!   `#xywh=x,y,w,h`; tiles are 320×180, about every 10 s. A `scale` other than 1 maps positions:
//!   `source = client / scale`. Signed URLs expire (403): look up again.
//! - `GET /v1/keys/validate` answers `{valid}` with status 200.
//! - Errors: 401 bad key, 404 no previews for that title and duration, 429 limit reached (`Retry-After` seconds;
//!   5,000 lookups, 20 movies and 70 episodes per day), 500 retry later.
//!
//! The key lives in Windows Credential Manager, never in settings, logs or the WebView. Lookups are cached for a few
//! minutes (as the docs allow), titles without previews and an exhausted quota are remembered so they are not asked
//! again and again, and tile sheets are fetched by the host (only `https://*.seekr.tv`) and served through the image
//! scheme, so the CSP still forbids every remote image origin.
use granary_silo_api::api::ImageData;
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const API: &str = "https://api.seekr.tv";
/// Credential Manager key of the API key (service "Granary").
pub const KEY_NAME: &str = "seekr/api-key";
/// Lookups are reused this long (signed URLs stay valid at least that long).
const LOOKUP_TTL: Duration = Duration::from_secs(10 * 60);
/// A title Seekr has no previews for is not asked again for this long.
const MISSING_TTL: Duration = Duration::from_secs(6 * 3600);
/// Without a `Retry-After`, a reached limit pauses lookups this long.
const LIMIT_PAUSE: Duration = Duration::from_secs(3600);
const SHEETS: usize = 24;
const MAX_SHEET: usize = 16 * 1024 * 1024;

/// How a title is identified to Seekr.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ident {
    Tmdb(i64),
    Imdb(String),
    ShowTmdb { show: i64, season: i64, episode: i64 },
    ShowImdb { show: String, season: i64, episode: i64 },
}

impl Ident {
    fn query(&self) -> Vec<(&'static str, String)> {
        match self {
            Self::Tmdb(id) => vec![("tmdb_id", id.to_string())],
            Self::Imdb(id) => vec![("imdb_id", id.clone())],
            Self::ShowTmdb { show, season, episode } => vec![
                ("show_tmdb_id", show.to_string()),
                ("season", season.to_string()),
                ("episode", episode.to_string()),
            ],
            Self::ShowImdb { show, season, episode } => vec![
                ("show_imdb_id", show.clone()),
                ("season", season.to_string()),
                ("episode", episode.to_string()),
            ],
        }
    }

    fn cache_key(&self, duration_ms: u64) -> String {
        format!("{self:?}|{duration_ms}")
    }
}

/// An IMDb id as Seekr expects it (`tt` + digits); anything else is not sent.
pub fn imdb(id: Option<&str>) -> Option<String> {
    let id = id?.trim();
    (id.len() > 2 && id.starts_with("tt") && id[2..].bytes().all(|b| b.is_ascii_digit())).then(|| id.to_owned())
}

pub fn tmdb(id: Option<&str>) -> Option<i64> {
    id?.trim().parse().ok().filter(|n| *n > 0)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeekrCue {
    /// Seconds in the source (multiply by `scale` for the playing file).
    pub start: f64,
    pub end: f64,
    /// Sheet URL without the fragment (https, *.seekr.tv).
    pub url: String,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SeekrTrack {
    pub scale: f64,
    pub cues: Vec<SeekrCue>,
}

#[derive(Debug)]
pub enum SeekrError {
    /// No key configured, or Seekr rejected it.
    Unauthorized,
    /// Daily quota or rate limit; lookups pause until then.
    Limited,
    /// No previews for this title and duration.
    NotFound,
    Other(String),
}

/// Only Seekr's own hosts over https may be fetched with the image scheme.
pub fn allowed_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str()
                .is_some_and(|h| h == "seekr.tv" || h.ends_with(".seekr.tv"))
            && u.port().is_none()
    })
}

fn timestamp(s: &str) -> Option<f64> {
    let mut parts: Vec<&str> = s.trim().split(':').collect();
    let secs: f64 = parts.pop()?.replace(',', ".").parse().ok()?;
    let mut total = secs;
    let mut mult = 60.0;
    while let Some(p) = parts.pop() {
        total += p.parse::<f64>().ok()? * mult;
        mult *= 60.0;
    }
    (total.is_finite() && total >= 0.0).then_some(total)
}

/// Parse Seekr's WebVTT: a timing line, then a sheet URL with `#xywh=`. Cues with other URLs are dropped.
pub fn parse_vtt(text: &str) -> Vec<SeekrCue> {
    let mut cues = Vec::new();
    let mut timing: Option<(f64, f64)> = None;
    for line in text.lines().map(str::trim) {
        if let Some((a, b)) = line.split_once("-->") {
            let end = b.split_whitespace().next().unwrap_or_default();
            timing = timestamp(a).zip(timestamp(end));
            continue;
        }
        let Some((start, end)) = timing else { continue };
        let Some((url, frag)) = line.split_once("#xywh=") else {
            continue;
        };
        timing = None;
        let n: Vec<u32> = frag.split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if n.len() != 4 || n[2] == 0 || n[3] == 0 || !allowed_url(url) {
            continue;
        }
        cues.push(SeekrCue {
            start,
            end,
            url: url.to_owned(),
            x: n[0],
            y: n[1],
            w: n[2],
            h: n[3],
        });
    }
    cues
}

#[derive(Default)]
struct Inner {
    tracks: HashMap<String, (Instant, Arc<SeekrTrack>)>,
    missing: HashMap<String, Instant>,
    limited_until: Option<Instant>,
    sheets: VecDeque<(String, Arc<ImageData>)>,
}

pub struct SeekrState {
    http: reqwest::Client,
    inner: Mutex<Inner>,
}

impl Default for SeekrState {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(concat!("Granary/", env!("CARGO_PKG_VERSION")))
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_default(),
            inner: Mutex::default(),
        }
    }
}

fn retry_after(resp: &reqwest::Response) -> Duration {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map_or(LIMIT_PAUSE, |s| Duration::from_secs(s.clamp(1, 24 * 3600)))
}

#[derive(serde::Deserialize)]
struct Lookup {
    vtt_url: String,
    #[serde(default)]
    scale: Option<f64>,
}

impl SeekrState {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        // A poisoned lock only means an earlier call panicked; the caches stay usable.
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `true` when Seekr accepts the key.
    pub async fn validate(&self, key: &str) -> Result<bool, SeekrError> {
        let resp = self
            .http
            .get(format!("{API}/v1/keys/validate"))
            .header("X-API-Key", key)
            .send()
            .await
            .map_err(|e| SeekrError::Other(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Ok(false);
        }
        let v: serde_json::Value = resp.json().await.map_err(|e| SeekrError::Other(e.to_string()))?;
        Ok(v.get("valid").and_then(serde_json::Value::as_bool).unwrap_or(false))
    }

    /// The previews for a title, from the cache or one lookup plus the VTT.
    pub async fn track(&self, key: &str, ident: &Ident, duration_ms: u64) -> Result<Arc<SeekrTrack>, SeekrError> {
        let ck = ident.cache_key(duration_ms);
        let now = Instant::now();
        {
            let mut g = self.lock();
            if let Some((at, t)) = g.tracks.get(&ck)
                && now.duration_since(*at) < LOOKUP_TTL
            {
                return Ok(Arc::clone(t));
            }
            if g.missing
                .get(&ck)
                .is_some_and(|at| now.duration_since(*at) < MISSING_TTL)
            {
                return Err(SeekrError::NotFound);
            }
            if g.limited_until.is_some_and(|until| now < until) {
                return Err(SeekrError::Limited);
            }
            g.limited_until = None;
        }
        let mut query = ident.query();
        query.push(("duration_ms", duration_ms.to_string()));
        let resp = self
            .http
            .get(format!("{API}/sprites"))
            .query(&query)
            .header("X-API-Key", key)
            .send()
            .await
            // Never log the URL: it carries the title ids only, but keep logs free of third-party request details.
            .map_err(|e| SeekrError::Other(e.without_url().to_string()))?;
        match resp.status().as_u16() {
            200 => {}
            401 => return Err(SeekrError::Unauthorized),
            404 => {
                self.lock().missing.insert(ck, now);
                return Err(SeekrError::NotFound);
            }
            429 => {
                let pause = retry_after(&resp);
                self.lock().limited_until = Some(now + pause);
                tracing::info!(
                    pause_s = pause.as_secs(),
                    "Seekr limit reached; local previews until then"
                );
                return Err(SeekrError::Limited);
            }
            s => return Err(SeekrError::Other(format!("lookup status {s}"))),
        }
        let lookup: Lookup = resp.json().await.map_err(|e| SeekrError::Other(e.to_string()))?;
        if !allowed_url(&lookup.vtt_url) {
            return Err(SeekrError::Other("unexpected VTT host".into()));
        }
        let vtt = self
            .http
            .get(&lookup.vtt_url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| SeekrError::Other(e.without_url().to_string()))?
            .text()
            .await
            .map_err(|e| SeekrError::Other(e.to_string()))?;
        let cues = parse_vtt(&vtt);
        if cues.is_empty() {
            self.lock().missing.insert(ck, now);
            return Err(SeekrError::NotFound);
        }
        let scale = lookup.scale.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(1.0);
        let track = Arc::new(SeekrTrack { scale, cues });
        self.lock().tracks.insert(ck, (now, Arc::clone(&track)));
        Ok(track)
    }

    /// Forget cached lookups (a sheet answered 403: its signature expired).
    pub fn invalidate(&self) {
        let mut g = self.lock();
        g.tracks.clear();
        g.sheets.clear();
    }

    /// One tile sheet (no key needed), kept in memory while it is in use.
    pub async fn sheet(&self, url: &str) -> Result<Arc<ImageData>, SeekrError> {
        if !allowed_url(url) {
            return Err(SeekrError::Other("not a Seekr URL".into()));
        }
        if let Some((_, img)) = self.lock().sheets.iter().find(|(u, _)| u == url) {
            return Ok(Arc::clone(img));
        }
        let resp = self
            .http
            .get(url)
            .header("Accept", "image/*")
            .send()
            .await
            .map_err(|e| SeekrError::Other(e.without_url().to_string()))?;
        if resp.status().as_u16() == 403 {
            self.invalidate();
            return Err(SeekrError::Unauthorized);
        }
        if !resp.status().is_success() {
            return Err(SeekrError::Other(format!("sheet status {}", resp.status().as_u16())));
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("image/jpeg")
            .to_owned();
        let bytes = resp.bytes().await.map_err(|e| SeekrError::Other(e.to_string()))?;
        if bytes.len() > MAX_SHEET {
            return Err(SeekrError::Other("sheet too large".into()));
        }
        let img = Arc::new(ImageData {
            content_type,
            bytes: bytes.to_vec(),
        });
        let mut g = self.lock();
        g.sheets.push_back((url.to_owned(), Arc::clone(&img)));
        while g.sheets.len() > SHEETS {
            g.sheets.pop_front();
        }
        Ok(img)
    }
}

// ---- Tauri commands -------------------------------------------------------------------------------------------

use crate::settings::SettingsStore;
use crate::silo::{CommandError, SiloState};
use granary_silo_api::secret::Secret;
use tauri::{AppHandle, Manager, Runtime, State};

/// Image-scheme server id for Seekr tile sheets (`http://granary-img.localhost/~seekr?u=<sheet url>`).
pub const SHEET_SERVER: &str = "~seekr";

fn stored_key(silo: &SiloState) -> Option<Secret> {
    silo.credentials()
        .get(KEY_NAME)
        .ok()
        .flatten()
        .filter(|k| !k.is_empty())
}

/// Whether a Seekr key is saved (the key itself never leaves the host).
#[tauri::command]
pub async fn seekr_key_status(silo: State<'_, SiloState>) -> Result<bool, CommandError> {
    Ok(stored_key(&silo).is_some())
}

/// Save a key after Seekr accepted it (`true`), or delete it with `null`. A rejected key is not saved (`false`).
#[tauri::command]
pub async fn seekr_key_set(
    silo: State<'_, SiloState>,
    seekr: State<'_, SeekrState>,
    key: Option<String>,
) -> Result<bool, CommandError> {
    let store = silo.credentials();
    let Some(key) = key.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()) else {
        store.delete(KEY_NAME)?;
        seekr.invalidate();
        return Ok(false);
    };
    if key.len() > 256 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Ok(false);
    }
    let valid = seekr
        .validate(&key)
        .await
        .map_err(|e| CommandError::plain("network", format!("Seekr unreachable: {e:?}")))?;
    if valid {
        store.set(KEY_NAME, &Secret::new(key))?;
        seekr.invalidate();
    }
    Ok(valid)
}

/// Resolve how Seekr knows this item: a movie by TMDB (else IMDb) id; an episode by its show's TMDB / IMDb id with
/// season and episode, else by the episode's own IMDb id.
async fn identify(silo: &SiloState, server_id: &str, item_id: &str) -> Result<Option<Ident>, CommandError> {
    let id = item_id.to_owned();
    let item = silo
        .with_session(server_id, |s| async move { s.catalog_item(&id).await })
        .await?;
    if item.kind != "episode" {
        return Ok(tmdb(item.tmdb_id.as_deref())
            .map(Ident::Tmdb)
            .or_else(|| imdb(item.imdb_id.as_deref()).map(Ident::Imdb)));
    }
    let own_imdb = imdb(item.imdb_id.as_deref()).map(Ident::Imdb);
    let (Some(series), Some(season), Some(episode)) = (item.series_id.clone(), item.season_number, item.episode_number)
    else {
        return Ok(own_imdb);
    };
    let show = silo
        .with_session(server_id, |s| async move { s.catalog_item(&series).await })
        .await
        .ok();
    let by_show = show.and_then(|sh| {
        tmdb(sh.tmdb_id.as_deref())
            .map(|show| Ident::ShowTmdb { show, season, episode })
            .or_else(|| imdb(sh.imdb_id.as_deref()).map(|show| Ident::ShowImdb { show, season, episode }))
    });
    Ok(by_show.or(own_imdb))
}

/// Seekr previews for an item, or `null` when Seekr is off, has no key, has nothing for this title or its limit is
/// reached (the caller then falls back to local previews).
#[tauri::command]
pub async fn preview_seekr(
    silo: State<'_, SiloState>,
    seekr: State<'_, SeekrState>,
    settings: State<'_, SettingsStore>,
    server_id: String,
    item_id: String,
    duration_ms: u64,
) -> Result<Option<SeekrTrack>, CommandError> {
    if !settings.current().player.preview_source_on("seekr") || duration_ms < 1000 {
        return Ok(None);
    }
    let Some(key) = stored_key(&silo) else { return Ok(None) };
    let Some(ident) = identify(&silo, &server_id, &item_id).await? else {
        tracing::debug!("Seekr: item has no TMDB or IMDb id");
        return Ok(None);
    };
    match seekr.track(key.expose(), &ident, duration_ms).await {
        Ok(t) => Ok(Some((*t).clone())),
        Err(SeekrError::NotFound | SeekrError::Limited) => Ok(None),
        Err(SeekrError::Unauthorized) => {
            tracing::warn!("Seekr rejected the saved API key");
            Ok(None)
        }
        Err(SeekrError::Other(e)) => {
            tracing::debug!(error = %e, "Seekr lookup failed");
            Ok(None)
        }
    }
}

/// One tile sheet for the image scheme.
pub async fn serve_sheet<R: Runtime>(app: &AppHandle<R>, url: &str) -> Result<ImageData, CommandError> {
    app.state::<SeekrState>()
        .sheet(url)
        .await
        .map(|img| (*img).clone())
        .map_err(|e| CommandError::plain("not_found", format!("Seekr sheet: {e:?}")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    #[test]
    fn parses_the_documented_vtt() {
        let vtt = "WEBVTT\n\n00:00:00.000 --> 00:00:10.000\nhttps://sprites.seekr.tv/603/8280000/sheet_001.jpg?exp=1&sig=a#xywh=0,0,320,180\n\n\
                   00:00:10.000 --> 00:00:20.000\nhttps://sprites.seekr.tv/603/8280000/sheet_001.jpg?exp=1&sig=a#xywh=320,0,320,180\n\n\
                   01:00.000 --> 01:10.000 align:start\nhttps://evil.example/x.jpg#xywh=0,0,320,180\n\n\
                   1:00:00.000 --> 1:00:10.000\nhttps://sprites.seekr.tv/s.jpg#xywh=0,180,320,180\n";
        let cues = parse_vtt(vtt);
        assert_eq!(cues.len(), 3);
        assert_eq!(cues[1].x, 320);
        assert_eq!(
            cues[1].url,
            "https://sprites.seekr.tv/603/8280000/sheet_001.jpg?exp=1&sig=a"
        );
        assert!((cues[2].start - 3600.0).abs() < 1e-9 && cues[2].y == 180);
    }

    #[test]
    fn only_seekr_https_hosts() {
        assert!(allowed_url("https://sprites.seekr.tv/a.jpg"));
        assert!(allowed_url("https://seekr.tv/a.jpg"));
        assert!(!allowed_url("http://sprites.seekr.tv/a.jpg"));
        assert!(!allowed_url("https://seekr.tv.evil.example/a.jpg"));
        assert!(!allowed_url("https://notseekr.tv/a.jpg"));
        assert!(!allowed_url("https://sprites.seekr.tv:8443/a.jpg"));
    }

    #[test]
    fn identifiers() {
        assert_eq!(imdb(Some("tt0133093")).as_deref(), Some("tt0133093"));
        assert!(imdb(Some("0133093")).is_none() && imdb(Some("tt")).is_none() && imdb(None).is_none());
        assert_eq!(tmdb(Some("603")), Some(603));
        assert!(tmdb(Some("x")).is_none() && tmdb(Some("0")).is_none());
        let q = Ident::ShowTmdb {
            show: 1399,
            season: 1,
            episode: 5,
        }
        .query();
        assert_eq!(q[0], ("show_tmdb_id", "1399".to_owned()));
        assert_eq!(q.len(), 3);
    }
}
