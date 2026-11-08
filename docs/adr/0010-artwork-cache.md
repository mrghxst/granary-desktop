# ADR-0010: Bounded on-disk artwork cache

- Status: Accepted
- Date: 2026-11-08
- Inputs: owner request for 0.4.1 ("cache them for a while even if the movie is completed but make sure it never gets
  too big … prefer to keep for movies / shows which I haven't completed yet and are in the continue watching
  category"); ADR-0004 amendment 3 (artwork fetched by the host without credentials, served through `granary-img`).

## Context
- Every poster, backdrop and logo went through the host to the server again after each start; WebView2's HTTP cache
  is small and does not survive well for a custom scheme. Browsing felt slower than it needed to and showed blank
  cards offline.
- Artwork links are presigned (`exp` / `sig` query values change), so the URL alone is not a stable key.

## Decision
- `artcache::ArtCache` stores served artwork in `%LOCALAPPDATA%\Granary\artwork`. Each file is named by a 128-bit FNV
  hash of the key (server id + reference with `exp`, `expires`, `sig`, `signature` and `token` removed) and holds a
  small header (magic, key, content type) plus the bytes. Only raster types that the scheme would serve are stored.
  Entries larger than 8 MiB are not stored.
- **Hard limit 300 MiB.** Every write checks the total. Over the limit, eviction runs down to 85 %. It removes entries
  that are not marked "keep" before kept ones, and the least recently used first. The file's modification time is the
  last-used stamp and is refreshed on read.
- **Keep set.** The home page sends the artwork references of its Continue Watching / Next Up / in-progress cards
  (`artwork_keep`). These are kept 30 days after they were last marked, and they are evicted only when nothing else is
  left. Entries unused for 60 days are dropped anyway.
- **Freshness.** A cached copy is served at once. When it is older than 7 days, it is fetched again, and the old copy
  is served if the server cannot be reached (offline browsing).
- Settings → Servers shows the size and the limit, and has a Clear button.

## Consequences
- Repeat visits and restarts show artwork immediately, also offline. Disk use is bounded.
- Artwork changed on the server can take up to 7 days to appear (Clear forces it).

## Amendment 1 (2026-11-08, after 0.4.1)
- Seek-preview sheets (server trickplay; keys start with `trickplay/`, `silo-server/internal/trickplay/manifest.go@6791aac`
  L9, L41-44, or requested with `&p=1`) never go to disk: an in-memory LRU of 64 MiB (`PreviewSheets`) holds them.
  A one-time sweep (marker `swept-v2`) removes sheets and unreadable files earlier versions stored.
- Stale entries are served immediately and refreshed in the background (stale-while-revalidate).
- Torn writes (`*.tmp`) are deleted when the cache index loads; the keep list is capped at 5,000 names.
- Local preview scratch files (`%TEMP%\granary-previews`, named `<kind><pid>-<n>`) of other runs are swept at startup.
