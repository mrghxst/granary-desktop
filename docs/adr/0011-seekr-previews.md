# ADR-0011: Seek-preview source chain with optional Seekr (seekr.tv)

- Status: Accepted
- Date: 2026-11-08
- Inputs:
  - The owner's request for 0.4.1 asked that Seekr be tried first when the server has no previews, then local
    generation "if daily limit is reached or the media is not available", with every source able to be turned on or
    off and reordered in the settings.
  - Seekr's developer documentation (behind sign-up; the owner provided it as a PDF on 2026-11-08, read as data).
  - ADR-0009 (local previews).

## Context
- There are three possible preview sources:
  - the server's trickplay sheets (`getWatchTrickplay`), which are often not generated;
  - Seekr, a third-party service with pre-made sprite sheets for movies and episodes, keyed by TMDB / IMDb id and
    runtime, which needs the user's own API key;
  - local decoding (ADR-0009), which always works for direct play but costs bandwidth.
- Seekr's contract:
  - **Lookup:** `GET https://api.seekr.tv/sprites?duration_ms=…` with exactly one identifier (`tmdb_id`, `imdb_id`,
    or `show_tmdb_id` / `show_imdb_id` plus `season` and `episode`) and the header `X-API-Key`. The answer contains
    `vtt_url` and `scale`.
  - **VTT:** each cue is a sheet URL plus `#xywh=`. Tiles are 320×180, about every 10 s. Neither the VTT nor the
    sheets need the key.
  - **Scale:** `source = client / scale`.
  - **Signed URLs** expire and then answer 403, which means the lookup has to be repeated.
  - **Status codes:** 401 bad key, 404 no previews for this title and duration, 429 limit reached with
    `Retry-After`.
  - **Daily limits:** 5,000 lookups, 20 distinct movies and 70 distinct episodes.
  - Lookups may be cached for a few minutes.

## Decision
- **Settings.**
  - `player.previewSources` is an ordered list of `{id: server|seekr|local, enabled}`. By default all three are
    enabled, in the order server → seekr → local.
  - `seekPreviews` remains the master switch.
  - Normalisation keeps the known ids once each, in the saved order, and appends any missing one as enabled.
- **Resolution per file** (`lib/previews.ts`, `resolvePreviewProvider`): the enabled sources are tried in order, and
  the first one that has previews wins.
  - A server source with no sheets, or a failed request, falls through to the next source.
  - Seekr with no key, no TMDB/IMDb id, 404, 429 or a network error falls through as well.
  - The local source always succeeds.
  - Local decoding is also gated on the host (`preview_source_on("local")`).
- **Seekr in the host** (`src-tauri/src/seekr.rs`):
  - **Key storage:** the key is validated with `GET /v1/keys/validate` and stored in Windows Credential Manager under
    `seekr/api-key`. It is never returned to the WebView, written to settings or logged. The UI only learns whether
    a key is saved.
  - **Identification:**
    - a movie is identified by its TMDB id, else its IMDb id;
    - an episode is identified by the show's TMDB id (from the series detail) with season and episode, else by the
      show's IMDb id, else by the episode's own IMDb id;
    - the duration sent is the file's runtime in whole seconds.
  - **Caching:**
    - lookups are cached in memory for 10 minutes;
    - a 404 is remembered for 6 hours;
    - a 429 pauses all lookups for `Retry-After` seconds (1 hour if the header is missing), so a reached daily limit
      falls back to local previews without repeated calls.
  - **Sheets:**
    - Sheets are fetched by the host, only over https from `seekr.tv` / `*.seekr.tv` on the default port.
    - They are kept in memory (24 sheets) and served through `granary-img` under the server id `~seekr`. The CSP
      still allows no remote image origin.
    - A 403 on a sheet clears the cached lookups. The seek bar probes each sheet once and asks for a new lookup when
      it fails, at most once a minute.
- **Settings UI:**
  - each source has a checkbox and move-up / move-down buttons;
  - the Seekr entry has a password field for the key with Save and Remove;
  - Seekr's hint states what is sent.

## Consequences
- **Privacy:** with a key saved and Seekr enabled, Seekr learns which titles are played (TMDB/IMDb id and runtime)
  together with the user's key. No server address, account or file data is sent. Without a key, nothing is sent.
- **Limits:** the daily limits allow about 20 movies and 70 episodes a day. Beyond that, local decoding takes over
  until the limit resets.
- **Network dependency:** Seekr is a third-party online service. If it changes or disappears, the chain falls
  through to local previews.
