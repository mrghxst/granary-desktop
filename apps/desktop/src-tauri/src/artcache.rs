//! Disk cache for artwork (posters, backdrops, logos) served to the WebView (ADR-0010).
//!
//! - **Bounded:** the cache never exceeds [`MAX_BYTES`]; every write that would go past it first evicts entries.
//! - **Least recently used first,** but artwork of items in Continue Watching / Next Up is *kept*: those entries are
//!   evicted only after every other one, and only while the cache is still over the limit.
//! - **Kept for a while:** entries live until [`MAX_AGE`] without use, so a finished film's art stays cached for weeks;
//!   protection of kept art lapses [`KEEP_FOR`] after the item was last seen in Continue Watching.
//! - **Fresh:** an entry older than [`REVALIDATE`] is fetched again; if the server cannot be reached the cached copy
//!   is served instead.
//!
//! Signed artwork references carry an expiry and a signature that change on every listing; those query parameters
//! are left out of the cache key so the same image is not stored again and again.
//! Each file stores its full key and content type in a small header, so a hash collision can never serve the wrong
//! image.
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// Hard upper bound of the cache on disk.
pub const MAX_BYTES: u64 = 300 * 1024 * 1024;
/// Evict down to this share of the limit, so a full cache does not evict on every write.
const LOW_WATER: f64 = 0.85;
/// Entries unused for this long are dropped even when there is room.
pub const MAX_AGE: Duration = Duration::from_secs(60 * 24 * 3600);
/// Kept (Continue Watching) art loses its protection this long after it was last marked.
pub const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 3600);
/// Entries older than this are fetched again (served from disk if that fails).
pub const REVALIDATE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Single images larger than this are served but not cached.
const MAX_ENTRY: usize = 8 * 1024 * 1024;
const MAGIC: &[u8; 4] = b"GRA1";
const KEEP_FILE: &str = "keep.txt";
/// At most this many kept names are remembered.
const MAX_KEEP: usize = 5000;
/// Marker written once older caches were swept of seek-preview sheets (stored on disk before 0.4.2).
const SWEPT_FILE: &str = "swept-v2";

#[derive(Debug, Clone)]
struct Entry {
    size: u64,
    used: SystemTime,
    stored: SystemTime,
}

#[derive(Default)]
struct Index {
    dir: PathBuf,
    entries: HashMap<String, Entry>,
    total: u64,
    /// File name → when it was last marked as kept.
    keep: HashMap<String, SystemTime>,
    loaded: bool,
}

#[derive(Default)]
pub struct ArtCache {
    inner: Mutex<Index>,
}

pub struct Cached {
    pub content_type: String,
    pub bytes: Vec<u8>,
    /// Older than [`REVALIDATE`]: fetch again and fall back to this copy.
    pub stale: bool,
}

/// The cache key: server plus the reference without its volatile signing parameters.
pub fn cache_key(server_id: &str, reference: &str) -> String {
    let (path, query) = reference.split_once('?').unwrap_or((reference, ""));
    let mut params: Vec<&str> = query
        .split('&')
        .filter(|p| {
            let name = p.split('=').next().unwrap_or_default();
            !p.is_empty() && !matches!(name, "exp" | "expires" | "sig" | "signature" | "token")
        })
        .collect();
    params.sort_unstable();
    format!("{server_id}\n{path}?{}", params.join("&"))
}

/// Seek-preview sprite sheets (server trickplay keys start with `trickplay/`, silo-server
/// `internal/trickplay/manifest.go@6791aac` L9, L41-44) only matter while that file plays: they stay in memory.
pub fn is_preview_sheet(reference: &str) -> bool {
    reference.split('?').next().unwrap_or_default().contains("trickplay/")
}

type MemEntry = Arc<(String, Vec<u8>)>;

/// Byte-bounded in-memory LRU for images that must never reach the disk (seek-preview sheets).
pub struct MemCache {
    inner: Mutex<(VecDeque<(String, MemEntry)>, usize)>,
    limit: usize,
}

impl MemCache {
    pub fn new(limit: usize) -> Self {
        Self {
            inner: Mutex::default(),
            limit,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, (VecDeque<(String, MemEntry)>, usize)> {
        // A poisoned lock only means an earlier call panicked; the cache stays usable.
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn get(&self, key: &str) -> Option<MemEntry> {
        let mut g = self.lock();
        let i = g.0.iter().position(|(k, _)| k == key)?;
        let e = g.0.remove(i)?;
        let v = Arc::clone(&e.1);
        g.0.push_back(e);
        Some(v)
    }

    pub fn put(&self, key: String, content_type: String, bytes: Vec<u8>) {
        if bytes.len() > self.limit / 4 {
            return;
        }
        let mut g = self.lock();
        if let Some(i) = g.0.iter().position(|(k, _)| *k == key)
            && let Some((_, old)) = g.0.remove(i)
        {
            g.1 = g.1.saturating_sub(old.1.len());
        }
        g.1 += bytes.len();
        g.0.push_back((key, Arc::new((content_type, bytes))));
        while g.1 > self.limit {
            let Some((_, old)) = g.0.pop_front() else { break };
            g.1 = g.1.saturating_sub(old.1.len());
        }
    }

    pub fn clear(&self) {
        let mut g = self.lock();
        g.0.clear();
        g.1 = 0;
    }

    #[cfg(test)]
    fn bytes(&self) -> usize {
        self.lock().1
    }
}

/// The key stored in a cache file's header, read without loading the image.
fn stored_key(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = fs::File::open(path).ok()?;
    let mut head = [0u8; 6];
    f.read_exact(&mut head).ok()?;
    if &head[..4] != MAGIC {
        return None;
    }
    let mut key = vec![0u8; usize::from(u16::from_le_bytes([head[4], head[5]]))];
    f.read_exact(&mut key).ok()?;
    String::from_utf8(key).ok()
}

/// File name for a key: two independent 64-bit FNV-1a hashes (collisions are caught by the stored key anyway).
fn file_name(key: &str) -> String {
    let fnv = |seed: u64| {
        key.bytes()
            .fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
    };
    format!(
        "{:016x}{:016x}.img",
        fnv(0xcbf2_9ce4_8422_2325),
        fnv(0x6c62_272e_07bb_0142)
    )
}

fn encode(key: &str, content_type: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    let k = u16::try_from(key.len()).ok()?;
    let c = u8::try_from(content_type.len()).ok()?;
    let mut out = Vec::with_capacity(bytes.len() + key.len() + content_type.len() + 7);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&k.to_le_bytes());
    out.extend_from_slice(key.as_bytes());
    out.push(c);
    out.extend_from_slice(content_type.as_bytes());
    out.extend_from_slice(bytes);
    Some(out)
}

/// `(content type, image bytes)` when the file holds `key`.
fn decode(key: &str, data: &[u8]) -> Option<(String, Vec<u8>)> {
    let rest = data.strip_prefix(MAGIC)?;
    let (len, rest) = rest.split_at_checked(2)?;
    let klen = usize::from(u16::from_le_bytes([len[0], len[1]]));
    let (stored_key, rest) = rest.split_at_checked(klen)?;
    if stored_key != key.as_bytes() {
        return None;
    }
    let (&clen, rest) = rest.split_first()?;
    let (ctype, bytes) = rest.split_at_checked(usize::from(clen))?;
    Some((String::from_utf8(ctype.to_vec()).ok()?, bytes.to_vec()))
}

impl Index {
    fn load(&mut self, dir: &Path) {
        if self.loaded && self.dir == dir {
            return;
        }
        self.dir = dir.to_owned();
        self.entries.clear();
        self.total = 0;
        self.loaded = true;
        let _ = fs::create_dir_all(dir);
        let sweep = !dir.join(SWEPT_FILE).exists();
        if let Ok(list) = fs::read_dir(dir) {
            for e in list.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                // A write cut short by a crash or power loss was never counted: remove it.
                if name.ends_with(".tmp") {
                    let _ = fs::remove_file(e.path());
                    continue;
                }
                if !name.ends_with(".img") {
                    continue;
                }
                // Once per cache: preview sheets older versions stored on disk, and unreadable files.
                if sweep
                    && stored_key(&e.path()).is_none_or(|k| is_preview_sheet(k.split_once('\n').map_or("", |x| x.1)))
                {
                    let _ = fs::remove_file(e.path());
                    continue;
                }
                let Ok(meta) = e.metadata() else { continue };
                let used = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                let stored = meta.created().unwrap_or(used);
                self.total += meta.len();
                self.entries.insert(
                    name,
                    Entry {
                        size: meta.len(),
                        used,
                        stored,
                    },
                );
            }
        }
        self.keep = fs::read_to_string(dir.join(KEEP_FILE))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let (name, secs) = l.split_once(' ')?;
                let at = SystemTime::UNIX_EPOCH + Duration::from_secs(secs.parse().ok()?);
                Some((name.to_owned(), at))
            })
            .collect();
        if sweep {
            let _ = fs::write(dir.join(SWEPT_FILE), b"");
        }
    }

    fn kept(&self, name: &str, now: SystemTime) -> bool {
        self.keep
            .get(name)
            .is_some_and(|at| now.duration_since(*at).unwrap_or_default() < KEEP_FOR)
    }

    fn remove(&mut self, name: &str) {
        if let Some(e) = self.entries.remove(name) {
            self.total = self.total.saturating_sub(e.size);
        }
        let _ = fs::remove_file(self.dir.join(name));
    }

    /// Drop expired entries, then evict until `incoming` more bytes fit under the limit (down to the low-water mark):
    /// unkept entries oldest-used first, then kept ones the same way.
    fn evict(&mut self, incoming: u64, now: SystemTime) {
        let expired: Vec<String> = self
            .entries
            .iter()
            .filter(|(_, e)| now.duration_since(e.used).unwrap_or_default() > MAX_AGE)
            .map(|(n, _)| n.clone())
            .collect();
        for n in expired {
            self.remove(&n);
        }
        if self.total + incoming <= MAX_BYTES {
            return;
        }
        let target = ((MAX_BYTES as f64 * LOW_WATER) as u64).saturating_sub(incoming);
        let mut order: Vec<(bool, SystemTime, String)> = self
            .entries
            .iter()
            .map(|(n, e)| (self.kept(n, now), e.used, n.clone()))
            .collect();
        // Unkept before kept, least recently used first.
        order.sort();
        for (_, _, n) in order {
            if self.total <= target {
                break;
            }
            self.remove(&n);
        }
    }

    fn save_keep(&mut self, now: SystemTime) {
        self.keep
            .retain(|_, at| now.duration_since(*at).unwrap_or_default() < KEEP_FOR);
        if self.keep.len() > MAX_KEEP {
            let mut by_age: Vec<(SystemTime, String)> = self.keep.iter().map(|(n, at)| (*at, n.clone())).collect();
            by_age.sort();
            let extra = self.keep.len() - MAX_KEEP;
            for (_, n) in by_age.into_iter().take(extra) {
                self.keep.remove(&n);
            }
        }
        let text: String = self
            .keep
            .iter()
            .map(|(n, at)| {
                let secs = at.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs();
                format!("{n} {secs}\n")
            })
            .collect();
        let _ = fs::write(self.dir.join(KEEP_FILE), text);
    }
}

impl ArtCache {
    fn index(&self, dir: &Path) -> std::sync::MutexGuard<'_, Index> {
        // A poisoned lock only means an earlier call panicked; the index is rebuilt from disk if needed.
        let mut g = self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        g.load(dir);
        g
    }

    pub fn get(&self, dir: &Path, key: &str) -> Option<Cached> {
        let name = file_name(key);
        let mut idx = self.index(dir);
        let stored = idx.entries.get(&name)?.stored;
        let path = idx.dir.join(&name);
        let Some((content_type, bytes)) = fs::read(&path).ok().and_then(|d| decode(key, &d)) else {
            idx.remove(&name);
            return None;
        };
        let now = SystemTime::now();
        if let Some(e) = idx.entries.get_mut(&name) {
            e.used = now;
        }
        // The modification time doubles as the last-used stamp across restarts.
        if let Ok(f) = fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(now);
        }
        Some(Cached {
            content_type,
            bytes,
            stale: now.duration_since(stored).unwrap_or_default() > REVALIDATE,
        })
    }

    pub fn put(&self, dir: &Path, key: &str, content_type: &str, bytes: &[u8]) {
        if bytes.len() > MAX_ENTRY {
            return;
        }
        let Some(data) = encode(key, content_type, bytes) else {
            return;
        };
        let name = file_name(key);
        let mut idx = self.index(dir);
        let now = SystemTime::now();
        idx.remove(&name);
        idx.evict(data.len() as u64, now);
        let path = idx.dir.join(&name);
        let tmp = idx.dir.join(format!("{name}.tmp"));
        let written = fs::File::create(&tmp)
            .and_then(|mut f| f.write_all(&data))
            .and_then(|()| fs::rename(&tmp, &path));
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
            return;
        }
        idx.total += data.len() as u64;
        idx.entries.insert(
            name,
            Entry {
                size: data.len() as u64,
                used: now,
                stored: now,
            },
        );
    }

    /// Mark the artwork of items in Continue Watching / Next Up as kept (evicted last).
    pub fn keep(&self, dir: &Path, keys: &[String]) {
        let mut idx = self.index(dir);
        let now = SystemTime::now();
        for k in keys {
            idx.keep.insert(file_name(k), now);
        }
        idx.save_keep(now);
    }

    /// Bytes on disk (for Settings).
    pub fn size(&self, dir: &Path) -> u64 {
        self.index(dir).total
    }

    /// Delete everything.
    pub fn clear(&self, dir: &Path) {
        let mut idx = self.index(dir);
        let names: Vec<String> = idx.entries.keys().cloned().collect();
        for n in names {
            idx.remove(&n);
        }
        idx.keep.clear();
        let _ = fs::remove_file(idx.dir.join(KEEP_FILE));
        // Anything else that is not ours to keep (stray temp files).
        if let Ok(list) = fs::read_dir(&idx.dir) {
            for e in list.flatten() {
                if e.file_name().to_string_lossy().ends_with(".tmp") {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("granary-artcache-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn keys_ignore_signing_parameters() {
        let a = cache_key("s1", "/api/v2/artwork/k?exp=1&sig=a&w=300");
        let b = cache_key("s1", "/api/v2/artwork/k?w=300&sig=b&exp=2");
        assert_eq!(a, b);
        assert_ne!(a, cache_key("s1", "/api/v2/artwork/k?w=600"));
        assert_ne!(a, cache_key("s2", "/api/v2/artwork/k?w=300"));
    }

    #[test]
    fn roundtrip_and_collision_safety() {
        let d = dir("rt");
        let c = ArtCache::default();
        c.put(&d, "k1", "image/jpeg", b"abc");
        let got = c.get(&d, "k1").unwrap();
        assert_eq!(
            (got.content_type.as_str(), got.bytes.as_slice(), got.stale),
            ("image/jpeg", &b"abc"[..], false)
        );
        assert!(c.get(&d, "k2").is_none());
        let data = encode("k1", "image/png", b"x").unwrap();
        assert!(decode("other", &data).is_none());
        // A fresh index sees the same entry after a restart.
        let c2 = ArtCache::default();
        assert!(c2.get(&d, "k1").is_some());
        c2.clear(&d);
        assert_eq!(c2.size(&d), 0);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn eviction_keeps_continue_watching_art_and_respects_the_limit() {
        let d = dir("ev");
        let mut idx = Index::default();
        idx.load(&d);
        let now = SystemTime::now();
        let chunk = MAX_BYTES / 4;
        for (i, name) in ["a.img", "b.img", "c.img", "d.img"].iter().enumerate() {
            idx.entries.insert(
                (*name).to_owned(),
                Entry {
                    size: chunk,
                    used: now - Duration::from_secs(100 - i as u64),
                    stored: now,
                },
            );
            idx.total += chunk;
        }
        // "a" is the least recently used but kept; an expired entry goes regardless.
        idx.keep.insert("a.img".into(), now);
        idx.entries.insert(
            "old.img".into(),
            Entry {
                size: 1,
                used: now - MAX_AGE - Duration::from_secs(1),
                stored: now,
            },
        );
        idx.total += 1;
        idx.evict(chunk, now);
        assert!(idx.total + chunk <= MAX_BYTES);
        assert!(idx.entries.contains_key("a.img"));
        assert!(!idx.entries.contains_key("old.img"));
        assert!(!idx.entries.contains_key("b.img"));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn startup_sweeps_preview_sheets_and_torn_writes() {
        let d = dir("sw");
        let c = ArtCache::default();
        c.put(&d, &cache_key("s1", "/api/v2/artwork/poster?w=300"), "image/jpeg", b"p");
        c.put(
            &d,
            &cache_key("s1", "https://cdn.example/trickplay/7/3/0.3.jpg?sig=a"),
            "image/jpeg",
            b"t",
        );
        fs::write(d.join("0000.img.tmp"), b"half").unwrap();
        // Older versions had no sweep marker.
        let _ = fs::remove_file(d.join(SWEPT_FILE));
        let c2 = ArtCache::default();
        assert!(c2.get(&d, &cache_key("s1", "/api/v2/artwork/poster?w=300")).is_some());
        assert!(
            c2.get(&d, &cache_key("s1", "https://cdn.example/trickplay/7/3/0.3.jpg"))
                .is_none()
        );
        assert!(!d.join("0000.img.tmp").exists());
        assert_eq!(
            c2.size(&d),
            fs::metadata(d.join(file_name(&cache_key("s1", "/api/v2/artwork/poster?w=300"))))
                .unwrap()
                .len()
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn memory_cache_is_bounded_and_lru() {
        assert!(is_preview_sheet("/api/v2/artwork/trickplay/9/1/2.1.jpg?exp=1"));
        assert!(!is_preview_sheet("/api/v2/artwork/posters/9.jpg?k=trickplay/"));
        let m = MemCache::new(100);
        m.put("a".into(), "image/jpeg".into(), vec![0; 20]);
        m.put("b".into(), "image/jpeg".into(), vec![0; 20]);
        assert!(m.get("a").is_some()); // a is now the most recent
        for k in ["c", "d", "e", "f"] {
            m.put(k.into(), "image/jpeg".into(), vec![0; 20]);
        }
        assert!(m.bytes() <= 100);
        assert!(m.get("a").is_some() && m.get("b").is_none());
        m.put("huge".into(), "image/jpeg".into(), vec![0; 60]);
        assert!(
            m.get("huge").is_none(),
            "an entry over a quarter of the limit is not kept"
        );
        m.clear();
        assert_eq!(m.bytes(), 0);
    }
}
