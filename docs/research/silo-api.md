# Silo server API: client contract for Granary (native Windows desktop client)

Research note R1. Read-only analysis of the upstream repositories. Everything below is either
cited to a source line at the pinned SHA or marked **UNVERIFIED**. Recommendations specific to
Granary (Tauri 2 + Rust + libmpv) are labelled *Granary note* and are not upstream facts.

## Upstream versions

| Repo | SHA (full) | Short form used in citations |
| --- | --- | --- |
| silo-server | `6791aac867664c41c55ee82909fe063d145911a6` | `@6791aac` |
| silo-android | `ee178d887ff02ce74150796e5f80934416bf88a7` | `@ee178d8` |
| silo-apple | `77def097c47d69c5796a10b93087545b4999a2df` | `@77def09` (not cross-checked in depth) |

The canonical machine-readable contract is `silo-server/contracts/api/v2/openapi.json@6791aac`
(abbreviated **OAS** below; `OAS:L<n>` is the line of the `operationId` or component schema).
The same document is served live at `GET /api/v2/openapi.json`, and Swagger UI at
`GET /api/v2/docs`. Both are reachable before login (`silo-server/docs/api-docs.md@6791aac` L1-33).

## Summary

1. Target **`/api/v2` only**. `/api/v1` is a frozen bridge that will answer `410 client_upgrade_required` at 1.0. Only `/api/v1/health` and `/api/v1/ready` survive (`silo-server/docs/architecture/api-contract.md@6791aac` L1811-1835).
2. Auth: `POST /api/v2/auth/login` returns HS256 JWT access (8h) and refresh (30d) tokens. Send `Authorization: Bearer <access>` on every call and rotate with `POST /api/v2/auth/refresh`. The session is checked server-side on every request.
3. Profiles: almost every viewer route requires `X-Profile-Id`. A PIN-locked profile also needs `X-Profile-Token`, obtained from `POST /api/v2/profiles/{id}/verify-pin`.
4. Device identity: send `X-Silo-Device-Id` (a stable UUID), `X-Silo-Device-Name` and `X-Silo-Device-Platform`, plus the `X-Silo-Client*` headers.
5. Discovery before login: `GET /api/v2/system/info` (`api_major` must be 2) and `GET /api/v2/system/identity` (`server_id`).
6. Browse uses `GET /api/v2/user/libraries`, `GET /api/v2/catalog` (filter, sort, search with `q`), `GET /api/v2/home/sections` and `GET /api/v2/library/{id}/sections`. Responses use cursor paging (`page.next_cursor`).
7. Item detail: `GET /api/v2/catalog/items/{id}` for the page; `GET /api/v2/watch/{id}` for what to play (versions, tracks, HDR/DV fields, chapters, markers, subtitles, `user_data` resume).
8. Playback is protocol v3, negotiated with **`POST /api/v2/playback/start`**. The client declares codec evidence and delivery classes, and the server returns a `PlaybackDecision` with one plan (stream URL + headers, timeline offsets, subtitle inventory).
9. Progress: `POST /api/v2/playback/{session_id}/progress` every 10 s with a monotonically increasing `sequence`. Stop: `DELETE /api/v2/playback/{session_id}` with a client-minted `stop_id` UUID. Watched is 90% and minimum resume is 5% (server defaults).
10. Realtime: `POST /api/v2/events/ws-ticket` (30 s ticket), then `GET /api/v2/events/ws` with subprotocols `silo.events.v2` and `silo.ticket.<ticket>`. Connections last at most 5 min, so the client must re-ticket and reconnect.

---

## 1. Auth

### 1.1 Transport and token model

- Bearer tokens: `extractBearerToken` reads `Authorization: Bearer <token>` and falls back to the `?token=` query parameter (`silo-server/internal/api/middleware/auth.go@6791aac` L458-470).
- Token types: JWTs are signed HS256 (`silo-server/internal/auth/jwt.go@6791aac` L266).
  - Claims include `user_id`, `role`, `session_id`, `profile_id`, `token_type`, `impersonator_user_id`, `password_change_required`, `api_key_id`, `rate_tier`, `file_id`, `session_purpose` and `profile_pin_revision` (`jwt.go` L20-75).
  - A token starting with `sa_` is treated as an API key rather than a JWT (`middleware/auth.go` L101-200).
- `RequireAuth` (`middleware/auth.go@6791aac` L101-200):
  - requires `token_type == access`;
  - looks up the login session on every request, and an inactive session gives `401` (session invalid);
  - a role change since issue gives `401 token_refresh_required`.
- Lifetimes are admin settings, with defaults `auth.access_token_expiry = "8h"` and `auth.refresh_token_expiry = "30d"` (`silo-server/internal/config/admin_settings.go@6791aac` L375-376; also `DefaultAuthRefreshTokenExpiry` 30d at L344).
- Refresh validates the refresh JWT, checks the session is still active, slides the session expiry and issues a **new pair** (`silo-server/internal/auth/service.go@6791aac` L1184-1230). Always persist the returned `refresh_token`.

### 1.2 Endpoints

| Op | Request | Response |
| --- | --- | --- |
| `POST /api/v2/auth/login` (`login`, OAS:L144782), no auth | Body `LoginInputBody {username*: string, password*: string, provider?: string}`. Headers `X-Silo-Device-Id`, `X-Silo-Device-Name` (≤120 chars), `X-Silo-Device-Platform` (≤40 chars) are recorded on the login session. | `200 TokenPair {access_token*, refresh_token*, expires_in*: int (seconds), user*: Account}` (OAS:L54121) |
| `POST /api/v2/auth/refresh` (`refreshSession`, OAS:L146357), no auth | `{refresh_token*: string}` | `200 RefreshedTokens {access_token*, refresh_token*, expires_in*}` |
| `POST /api/v2/auth/logout` (`logout`, OAS:L144975), bearer | none | `204` |
| `GET /api/v2/account/me` (`getCurrentUser`, OAS:L59905), bearer | none | `Account {id*, username*, email*, role*: "admin"\|"user", permissions*: [string], download_allowed*, password_change_required*, impersonation?: {active, impersonator_user_id, impersonator_username}}` |

Error codes for `login`: 400, 401, 403, 406, 408, 409, 413, 415, 422, 429, 500, 503 (OAS:L144782).

### 1.3 Required client behaviour (from server docs and Android)

- **401 `token_refresh_required`**: refresh once, then retry the original request. Do not sign out (`silo-server/docs/auth-api.md@6791aac` L335-348; `silo-server/docs/architecture/api-contract.md@6791aac` L624-627).
- **503 with `Retry-After`** during auth: keep the tokens and retry later. A dependency outage does not mean signed out (`auth-api.md` L350-378).
- Android refresh behaviour:
  - it refreshes proactively 60 s before expiry (`PROACTIVE_REFRESH_MARGIN_MS = 60_000`);
  - on a 401 it refreshes **once**, under a mutex, so concurrent requests share one refresh (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/AuthInterceptorImpl.kt@ee178d8` L25, L83-96).
- Session quota: login sessions are capped per account. Media and playback routes are exempt from the per-request quota accounting (`auth-api.md` L514-562).
- Other auth flows exist (**not needed for an MVP**): PIN rules (`auth-api.md` L563-626), device sign-in / remote-playback sessions (`auth-api.md` L627-678), and account identity linking (`/api/v2/account/identities/*`, OAS).
- **Errors** are RFC 9457 `application/problem+json`:
  - fields are `{type, title, status, detail, instance: "urn:silo:request:<id>", errors?: [{location, code, detail}]}`;
  - `instance` matches the `X-Request-ID` response header;
  - clients must never parse `detail` (`api-contract.md` L590-639);
  - the schema is `#/components/schemas/Problem` (OAS:L58829).

### 1.4 Device and client headers

- Device headers are `X-Silo-Device-Id`, `X-Silo-Device-Name` and `X-Silo-Device-Platform` (`auth-api.md@6791aac` L307-322).
- Client identity headers on playback ops (OAS `replanPlayback`, L201353):
  - `X-Silo-Client`, `X-Silo-Client-Version`, `X-Silo-Client-Build`, `X-Silo-Client-Channel`;
  - `X-Silo-Client-Family` (`tv|mobile|tablet|desktop|web`; see `listHomeSections`, OAS:L179770).
  - Alternatively `X-Client-Name`, `-Version`, `-Build`, `-Channel`, `-Model`, `-Platform` and `-OS-Version`. When `X-Client-Name` is non-blank it **replaces** the `X-Silo-Client*` identity.
- Android attaches these on every request (`AuthInterceptorImpl.kt@ee178d8` L907-920):
  - the device id is a random UUID persisted in prefs `silo_device_metadata`;
  - the name is manufacturer + model;
  - the client name is `Silo Android` or `Silo Android TV`;
  - the family is `tv`, `tablet` or `mobile` (`silo-android/android-shared/src/androidMain/kotlin/org/siloserver/silo/common/network/AndroidDeviceMetadataProvider.kt@ee178d8`).
- A successful playback start records `X-Silo-Device-Id` (first 128 chars) in the profile's device registry (OAS `replanPlayback` / `startPlayback` header descriptions; `silo-server/docs/playback-api.md@6791aac` L163-280).

*Granary note:* generate a UUID on first run and persist it. Send `X-Silo-Client: Granary`, `X-Silo-Client-Family: desktop` and `X-Silo-Device-Platform: windows`. Whether the server treats an unknown `X-Silo-Client` value specially is **UNVERIFIED**; the OAS describes these values as opaque.

## 2. Server info, health and version semantics (pre-login)

| Op | Auth | Response |
| --- | --- | --- |
| `GET /api/v2/system/info` (`getSystemInfo`, OAS:L227535) | none | `SystemInfo {api_major*: int, server_version*: string, contract_digest*: string, links*: {capabilities*, identity*, openapi*}, request_origin?: string}` (OAS:L53300) |
| `GET /api/v2/system/identity` (`getServerIdentity`, OAS:L227455) | none | `ServerIdentity {server_id*: string}`. Use it to de-duplicate saved servers. |
| `GET /api/v2/system/connections` (`getServerConnections`, OAS:L227297) | see OAS | Alternative connection addresses. Shape not dumped (**UNVERIFIED** details). |
| `GET /api/v2/system/setup` (`getSetupStatus`, OAS:L227604) | none | Whether first-run setup is complete. Shape not dumped. |
| `GET /api/v1/health` | none | `{status, server_name?, server_id?}` (`silo-server/internal/api/handlers/health.go@6791aac` L30-80) |
| `GET /api/v1/ready` | none | `{status: ok\|error\|degraded\|shutting_down, postgres?, s3?, artwork?}` (`health.go` L30-80) |

- Routing: `r.Handle("/api/v2/*", apiv2.NewHandler(v2deps))` (`silo-server/internal/api/router.go@6791aac` L3288). The `/api/v1` group keeps `/health` and `/ready` (L3290-3292).
- **Android add-server probe** (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/ApiV2Probe.kt@ee178d8` L47-140):
  - it calls `GET /api/v2/system/info` without auth, with a 6 s timeout;
  - it requires `api_major == 2`;
  - a legacy 404 body is taken to mean the server must be updated.
- Android also calls `GET "/health"` unauthenticated (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/api/HealthApi.kt@ee178d8`). The server registers no root `/health` route at `router.go` L3288-3292, so how that resolves is **UNVERIFIED**. Granary should use `/api/v2/system/info` plus `/api/v2/system/identity`.
- **Versioning** (`silo-server/docs/release-versioning.md@6791aac`):
  - releases use SemVer tags, but no tagged release exists yet at this SHA;
  - images are tagged `build-N`, `latest` and by commit SHA;
  - pre-1.0 minor releases may break.
- `/api/v2` is the stable surface intended to lock at 1.0 (`silo-server/docs/architecture/v1-scope.md@6791aac`).
- Version gating rules for Granary:
  - Gate on `api_major` and then on capability endpoints (`features[]`, `revision`), never on `server_version` string comparison.
  - `contract_digest` changes whenever the contract changes and can be logged for diagnostics.

## 3. Users and profiles

| Op | Request | Response |
| --- | --- | --- |
| `GET /api/v2/profiles` (`listProfiles`, OAS:L203258), bearer | none | `ProfileCollection {avatar_upload_enabled*, items*: [Profile]}`. `Profile` (OAS:L46528) has these required fields: `id`, `name`, `avatar`, `avatar_source`, `has_pin`, `is_child`, `is_primary`, `language`, `subtitle_language`, `show_forced_subtitles`, `quality_preference`, `max_playback_quality`, `max_content_rating`, `max_advisory_age`, `require_advisory_age`, `auto_skip_intro`, `auto_skip_credits`, `auto_skip_recap`, `auto_play_next_preview`, `allowed_library_ids`, `library_restrictions_enabled`, `preferred_metadata_language`, `created_at`. `avatar_url` is optional. |
| `POST /api/v2/profiles/{id}/verify-pin` (`verifyProfilePIN`, OAS:L205764) | `{pin*: string}` | `ProfileVerification {valid*: bool, profile_token?: string, expires_at*: date-time\|null}` (OAS:L47478) |

- Viewer requests carry `X-Profile-Id` (required on profile-scoped ops) and `X-Profile-Token` (required only when that profile is PIN-locked) (OAS parameter text on `listCatalogItems`, OAS:L150065).
- A `403 profile_verification_required` means the profile token no longer verifies. Go back to profile select or PIN entry (`silo-server/docs/realtime-api.md@6791aac` L289-295).
- A profile token is bound to the profile's PIN, so only a PIN change invalidates it (same lines).
- Android applies the profile headers to API calls (`AuthInterceptorImpl.kt@ee178d8` L849-870). It **removes** them from the events socket request (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/EventsSocketV2Api.kt@ee178d8` L146).
- Profile CRUD exists (`POST/PATCH/DELETE /api/v2/profiles...`, OAS ops list) but is not needed for a viewer client.
- On v1 viewer routes, PIN/restricted households require `X-Profile-Id` (`v1-scope.md@6791aac`). This is not relevant for v2-only clients.

## 4. Libraries, browse and search

### 4.1 Libraries visible to the viewer

- **`GET /api/v2/user/libraries`** (`listUserLibraries`, OAS:L227883):
  - `X-Profile-Id` is optional, `X-Profile-Token` as above, and the query `include_hidden` is a bool;
  - the response is `{items*: [UserLibrary], page?}`;
  - `UserLibrary` (OAS:L54514) is `{id*: string, name*: string, type*: string, sort_order*: int, box_sets_enabled*: bool, poster_url?: string}`;
  - Android uses this path (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/api/PersonalDataApi.kt@ee178d8` L59).
- `GET /api/v2/libraries` (`listLibraries`) is `x-silo-class: acting_admin`, so it is admin only. **Do not use it for browsing** (OAS ops list; dump of `listLibraries`).
- Per-library pages:
  - `GET /api/v2/library/{id}/layout`, `GET /api/v2/library/{id}/sections` (`listLibrarySections`, OAS:L187307) and `GET /api/v2/library/{id}/sections/{section_id}/items` (`getLibrarySectionItems`, OAS:L187591);
  - collections at `GET /api/v2/library/{id}/collections`;
  - Android uses these (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/LibrarySectionItemsV2Api.kt@ee178d8` L17, L32; `CatalogV2Api.kt@ee178d8` L145).
  - Response shapes were not dumped. They are assumed to be `Section`-like (**UNVERIFIED**).

### 4.2 Catalog browse and search

**`GET /api/v2/catalog`** (`listCatalogItems`, OAS:L150065). It is `profile_scoped` and requires `X-Profile-Id`. Query parameters:

| Param | Type / values |
| --- | --- |
| `source` | `query` (default), `section`, `library_collection`, `user_collection`, `favorites`, `watchlist`, `history`, `person` |
| `scope` | `home` or `library` (for `source=section`) |
| `section_id`, `library_id`, `collection_id`, `person_id` | string |
| `q` | search text |
| `type` | `movie`, `series`, `episode`, `audiobook`, `ebook`, `manga`, `video` (movies + series), or `video_with_episodes` (with `q`) |
| `genre`, `status`, `year_min`, `year_max` | filters |
| `content_rating` | repeatable |
| `match` | `all` or `any` |
| `name_prefix` | alphabet jump |
| `sort` | `field` or `-field`. Fields include `title`, `year`, `release_date`, `added_at`, `rating`, `runtime`, `random`… (the full list is in the filters document) |
| `group=work` | collapses book editions |
| `skip_total` | bool |
| `limit` | default 50, max 200 |
| `cursor` | from `page.next_cursor` |
| `seek` | zero-based window jump |
| `groups` | JSON rule groups |
| `query_limit` | int |
| Image sizing: `image_size` | `small`, `medium`, `large` or `original` |
| Image sizing: `poster_width`, `backdrop_width`, `logo_width`, `still_width`, `photo_width` | device pixels |

- The response is `CatalogBrowseCollection`:
  - `items*: [CatalogItem]` and `page?: {has_more*, next_cursor?}`;
  - `total*: int`, `total_exact*: bool` and `window_cursor*`;
  - `effective_sort?: {field, order}` and `search_diagnostics?`.
- The body form is `POST /api/v2/catalog/query` (`queryCatalogItems`, OAS:L155465), with `x-silo-retry-safety: natural_idempotent`.
- Facets come from `GET /api/v2/catalog/filters` (`getCatalogFilters`, OAS:L151423) and `GET /api/v2/catalog/filters/search`.
- Search mode is described by `GET /api/v2/catalog/search/capabilities` (`getCatalogSearchCapabilities`, OAS:L155758; shape not dumped).

**`CatalogItem`** card (OAS:L26983):

- Required: `content_id*` (deterministic string), `title*`, `type*` (`movie|series|season|episode|audiobook|ebook|podcast|podcast_episode`), `status*`, `genres*`, `keywords*`.
- Artwork:
  - `poster_url`, `backdrop_url`, `logo_url` (presigned, short-lived), each with a `*_thumbhash` where one exists;
  - on episode cards, `season_poster_url`, `series_poster_url` and `series_backdrop_url`;
  - flags `poster_is_episode_still` and `backdrop_is_episode_still`.
- Metadata: `year`, `release_date`, `runtime` (minutes), `duration_seconds`, `overview`, `content_rating`, `rating_tmdb`, `rating_imdb`, `rating_rt_*`, `season_number`, `episode_number`, `series_id`, `series_title`, `badges`, `overlay_summary`.
- Viewer state:
  - `user_state?: {played*, is_favorite*, in_watchlist*, not_interested*, watched_count?, unplayed_count?}` (OAS:L28026);
  - on continue-watching cards, `position_seconds`, `progress_updated_at` and `item_source` (`in_progress` or `next_up`);
  - `play_content_id` names the item to play when the card is a series or season.

### 4.3 Series structure

| Op | Notes |
| --- | --- |
| `GET /api/v2/catalog/series/{id}/seasons` (`listSeriesSeasons`, OAS:L155969) | `SeasonCollection {items*: [Season], page?}`. Takes `include_artwork`. |
| `GET /api/v2/catalog/series/{id}/seasons/{num}/episodes` (`listSeasonEpisodes`, OAS:L156533) | `num` is an int, with 0 for specials. Returns `EpisodeCollection {items*: [Episode], page?}`. |
| `GET /api/v2/catalog/items/{id}/episodes` (`listCatalogItemEpisodes`, OAS:L152574) | All episodes of an item (shape not dumped). |

The fields of `Season` and `Episode` were not dumped and are **UNVERIFIED**; they are expected to resemble `CatalogItem`.

## 5. Item details, versions, streams, HDR metadata and artwork

### 5.1 Detail page

- **`GET /api/v2/catalog/items/{id}`** (`getCatalogItem`, OAS:L152009) returns `CatalogItemDetail`.
  - It is a large superset of `CatalogItem`: cast, audiobook extension, air dates, backdrop urls and more. It was not fully enumerated.
  - Query: `library_id`, `file_id` (the selected version) and the image sizing params.
  - Header: `X-Silo-Device-Id`, which resolves device-scoped playback preferences.
- **`GET /api/v2/catalog/items/{id}/versions`** (`listCatalogItemVersions`, OAS:L154461) returns `FileVersionCollection {items*: [FileVersion], page?}`.

### 5.2 Watch state (what to play): the primary pre-play read

**`GET /api/v2/watch/{id}`** (`getWatchState`, OAS:L234067) works for a movie, episode, audiobook or ebook. A series is not playable. Query: `file_id`, `library_id`, `image_size`. Header: `X-Silo-Device-Id`. It returns `WatchDetail` (OAS:L54986):

- Identity and artwork:
  - `content_id*`, `title*` and `type*` (`movie|episode|audiobook|ebook`);
  - `year`, `overview`, `season_number`, `episode_number`, `series_id`, `series_title`;
  - `poster_url` and `backdrop_url` (both `uri-reference`), each with a thumbhash.
- `versions*: [WatchFileVersion]`, `subtitles*: [WatchSubtitle {language*, source*: embedded|external, forced*, hearing_impaired*, codec?, title?}]`, and `playback_variants`.
- Item-level markers: `intro`, `credits`, `recap`, `preview` (`WatchMarker {start_seconds*, end_seconds*, source?, provider?, algorithm?, confidence?}`).
- Profile-resolved preferences: `effective_subtitle_language`, `effective_subtitle_mode`, `effective_show_forced_subtitles`, `effective_subtitle_track_signature` and `effective_version_*`.
- `user_data?: WatchUserData` (OAS:L57950): `{played*, position_seconds?, duration_seconds?, is_in_progress?, last_file_id?, watched_count*, unplayed_count*, in_progress_count*, ...}`. This is where the **resume position** comes from.

**`WatchFileVersion`** (OAS:L55124):

- Identity and container: `file_id*` (opaque string), `container*`, `codec_video*`, `codec_audio*`, `resolution*`, `hdr*: bool`, `bitrate*` (**bits/s** here; note that `FileVersion.bitrate` is **kbit/s**, OAS:L35329), `duration_seconds*`, `file_size*`.
- Editions and multi-part: `edition_key` / `edition_raw`, `presentation_*` (multi-part), `multi_episode_start` / `end`.
- Tracks: `audio_tracks`, `video_tracks`, `subtitle_tracks` and `chapters`.
- Defaults: `effective_audio_track_index` and `effective_audio_language`.
- Markers and previews: `marker_segments*: [MarkerOccurrence]` and `trickplay_available*: bool`.

**HDR and Dolby Vision metadata per video track**:

- `VideoTrack` (OAS:L54784; the `WatchVideoTrack` variant is used inside `WatchFileVersion`) carries:
  - `codec`, `profile`, `level`, `width`, `height`, `bit_depth`, `pixel_format`, `frame_rate` (string), `interlaced*`;
  - `color_primaries`, `color_transfer`, `color_space`, `color_range`;
  - `video_range` and `video_range_type`;
  - `hdr10_plus: bool` and `dolby_vision`;
  - Dolby Vision details: `dv_profile`, `dv_level`, `dv_bl_compat_id`, `dv_bl_compat_id_present*`, `dv_config_present*`, `dv_bl_present`, `dv_el_present`, `dv_rpu_present`, `dv_enhancement_layer`.
- `AudioTrack` (OAS:L25578): `codec`, `profile`, `channels`, `layout`, `sample_rate`, `bit_depth`, `bitrate`, `language` (BCP 47, `und` when unknown), `title`, `embedded_title`, `default*`.
- `VersionSubtitleTrack`: `index`, `codec`, `language`, `forced*`, `hearing_impaired*`, `default*`, `external*`, `file_name`, `title`, `embedded_title`, `resolution`.
- The `Watch*Track` variants were not field-diffed against these and are assumed equivalent (**UNVERIFIED**).

*Granary note:* `video_range_type` plus the `dv_*` fields are enough to decide HDR10 / HLG / DV display switching before playback starts. The exact value vocabulary of `video_range_type` (for example `HDR10`, `DOVI`) was not confirmed (**UNVERIFIED**).

### 5.3 Artwork

- Artwork URLs in responses are **presigned and short-lived** (`CatalogItem.poster_url` description, OAS:L26983). Do not cache them by URL. Cache by `content_id` plus size, and refetch the metadata when a URL expires.
- `GET /api/v2/artwork/{key}?exp=<int>&sig=<string>` (`getArtwork`, OAS:L142607):
  - `x-silo-class: public`, so no bearer is needed;
  - supports `If-None-Match` and `Range`, and answers `200` or `206 image/*`, `304`, `404` or `503`.
- `GET /api/v2/images/capabilities` (`getImageCapabilities`, OAS:L180324) returns `ImageCapabilities` (OAS:L36645):
  - `state*` and `allowed*`;
  - `delivery*: server|direct` and `storage_backend*: local|s3`;
  - `sizes*: [string]`, `widths*: {size: ImageSizeWidths}` and `original_max_width_px*`;
  - `param*`, `slot_width_params*` and `season_list_artwork_param*`;
  - `revision*`;
  - it is a conditional GET, so send `If-None-Match`.
- Size selection: send `image_size=small|medium|large|original`, or per-slot pixel widths (`poster_width`, …). The server then picks the narrowest cached size at least that wide (OAS param text on `listCatalogItems`).
- Watch artwork (`poster_url`, `backdrop_url`, thumbhash) is described in `silo-server/docs/playback-api.md@6791aac` L116-162.

## 6. Playback negotiation (protocol v3 over `/api/v2`)

Primary references:

- `silo-server/docs/architecture/playback-protocol-v3.md@6791aac`:
  - §1 invariants, L22-68;
  - §2 endpoints and features, L70-475;
  - §3 evidence, L476-717;
  - §4 deliveries, L719-1010;
  - §5 timeline, L1011-1091;
  - §6 replan, L1092+;
  - §8 tracks and subtitles, L1573+.
- `silo-server/docs/playback-api.md@6791aac` L1-551.

### 6.1 Sequence

1. `GET /api/v2/playback/capabilities` (`getPlaybackCapabilities`, OAS:L198406). This is a conditional GET; send `If-None-Match`. It returns `PlaybackCapabilities` (OAS:L43970):
   - `state*`, `allowed*` and `revision*`;
   - `installation_id?` (**must be echoed** in every start, progress, stop and replan);
   - `protocol_versions*: [int]` (expect 3), `features*: [string]` and `deliveries*: [string]`;
   - `bandwidth_probe_url?`, `bandwidth_probe_target_kbps?`, `bandwidth_probe_max_age_seconds?` and `bandwidth_probe_automatic?`.
2. Optional bandwidth probe:
   - `GET /api/v2/playback/bandwidth-probe` (OAS:L198085), then `POST /api/v2/playback/bandwidth-probe/results` (OAS:L198221) (`playback-api.md` L59-115);
   - Android follows one redirect **without** credentials (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/PlaybackV2Api.kt@ee178d8` L296);
   - the connection's learned limit is read with `GET /api/v2/playback/connection` (OAS:L198607) and reset with `DELETE /api/v2/playback/connection/learned-limit`.
3. **`POST /api/v2/playback/start`** (`startPlayback`, OAS:L199765). It returns a `PlaybackDecision`.
4. Load `playback_plan.stream.url` with `playback_plan.stream.headers`, then seek to `timeline.player_start_seconds`.
5. Every 10 s, `POST /api/v2/playback/{session_id}/progress`.
6. On a seek outside the window, a track change, a quality change, an output change or a failure, call `POST /api/v2/playback/{session_id}/replan` (OAS:L201353).
7. To end the session, `DELETE /api/v2/playback/{session_id}` with a `stop_id`.

### 6.2 Start request: `PlaybackStartBody` (OAS:L45534)

Required: `installation_id`, `protocol_version` (3), `client_features: [string]`, `file_id` (string), `profile_id` (string), `playback_attempt_id`, `quality_preference`, `subtitle_fidelity_preference`, `client_capabilities`, `client_playback_context`.

Optional fields:

- Position: `start_position` (number, seconds). When it is omitted the server uses the stored resume position (`playback-protocol-v3.md` §2 start semantics).
- Tracks: `audio_track_id`, `audio_track_index`, `subtitle_track_id` and `subtitle_track_index`. Omitting the subtitle fields means subtitles **off**, which is how Android requests no subtitle.
- Versions: `allow_alternate_versions` (bool). Setting it to false pins the source file.
- Bandwidth: `bandwidth_estimate_kbps`, `bandwidth_estimate_source` (`probe|observed_rebuffer`), `bandwidth_cap_kbps` and `observed_throughput_kbps`.
- `learned_limit` (`ignore|clear`).
- `progress_persistence` (`server|client`; the default is server, and audiobooks use `client`).
- `metered` (deprecated and ignored).

`quality_preference`: `auto` (the default), a resolution such as `1080p`, or a rung label from `available_qualities`. `subtitle_fidelity_preference` is `preserve` or `compatible` (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/model/playback/PlaybackProtocolV3.kt@ee178d8` L267-276).

`playback_attempt_id` is a client-minted idempotency key: a retried start with the same id replays the decision (`playback-api.md` L163-280; `x-silo-retry-safety` in OAS).

**`client_capabilities: ClientCodecCapabilitiesV3`** (OAS:L28930):

```jsonc
{
  "video_evidence": "exact|platform_attested|declared",   // required
  "audio_evidence": "exact|platform_attested|declared",   // required
  "codecs_video": ["h264","hevc","av1","vp9", ...],       // required
  "codecs_video_hardware": ["h264","hevc", ...],          // required
  "codecs_audio": ["aac","ac3","eac3","flac","opus", ...],// required
  "containers": ["mkv","mp4", ...],                       // required
  "hdr": true,                                            // required
  "hdr_details": { "hdr10": true, "hdr10_plus": false, "hlg": true,
                   "dolby_vision_profiles": [5,8], "dolby_vision_profile_levels": [...],
                   "hdr10_max_width": 3840, "hdr10_max_height": 2160,
                   "hdr10_max_frame_rate": 60, "hdr10_max_bitrate_kbps": 0 },
  "max_resolution": "2160p",
  "video_decode": [ { "codec": "hevc", "hardware": true, "profiles": ["main10"],
                      "levels": [153], "bit_depths": [8,10], "max_width": 3840,
                      "max_height": 2160, "max_frame_rate": 60, "max_bitrate_kbps": 0,
                      "decoder_name": "..." } ],
  "audio_passthrough": { "passthrough_codecs": ["eac3","truehd"], "max_channels": 8,
                         "spatializer_enabled": false, "entries": [...] }
}
```

The field names and requiredness come from OAS `ClientCodecCapabilitiesV3`, `HDRCapabilitiesV3` (OAS:L35528), `VideoDecodeCapabilityV3` and `AudioPassthroughV3`. The concrete values above are illustrative only.

**Evidence tiers** (`playback-protocol-v3.md@6791aac` L484-530):

- `exact` (Android `MediaCodecList`):
  - strict walk of `video_decode[]`;
  - a codec that is only in the flat lists and has no matching `hardware: true` entry is **refused** for direct play (`evidence_insufficient_for_direct`);
  - this is the only tier that can earn a validated audio passthrough claim.
- `platform_attested` (Apple): the same walk, but profile and level are skipped for hardware entries.
- `declared` (Web): a flat list match. `video_decode[]` entries can only narrow it.
- Software decode entries (`hardware: false`) count at the exact and attested tiers only when the request advertises `software_video_decode_v1`.

**`client_playback_context: ClientPlaybackContextV3`** (OAS:L28993):

- Required: `protocol_version*`, `form_factor*`, `app_version*`, `device*` and `output*`.
- Optional: `app_build` and `app_channel`.
- `device: DeviceContextV3 {platform, manufacturer, model, os_version, platform_details: map<string,string>}`.
- `output: OutputContextV3 {output_context_id, sink_type, current_sink, display: {display_id, hdr_evidence*, hdr_types}, hdr_details, audio_passthrough}`.
- `deliveries*: map<deliveryClass, DeliveryCapabilityV3>`. The keys are the transport classes `original_http`, `progressive` and `hls` (`PlaybackProtocolV3.kt@ee178d8` L129-136).
- `DeliveryCapabilityV3` (OAS:L31459): `{enabled*, supported_on_device*, containers*, video_codecs*, audio_decode_codecs*, audio_passthrough_codecs*, subtitles*: DeliverySubtitleCapabilitiesV3, features*, transformations*, validated_claims*, auth_header_refresh*, max_channels?, max_channels_by_codec?, hdr_details?, failure_reason?}`.
- `DeliverySubtitleCapabilitiesV3` (OAS:L31550): `{embedded_text*, embedded_bitmap*, sidecar_text*, sidecar_bitmap*, ass_styling*, font_attachments*, native_embedded?: [{...}]}`.

**What Android sends.** All `@ee178d8`:

- `client_features`:
  - always `playback_plan_v3`, `client_video_transformations_v1`, `device_quirks_v1`, `seek_reanchor_v1` and `direct_stream_resume_v1` (`PlaybackProtocolV3.kt` L16-25, L171-177);
  - `embedded_subtitles_v1` when `deliveries.original_http.subtitles.native_embedded` is non-empty;
  - `layout_aware_passthrough` when output passthrough entries exist;
  - `hls_subtitle_renditions_v1` when the HLS delivery has `native_hls_playback_v1` (`PlaybackProtocolV3.kt` L188-204).
- `sequenced_progress_v1` is a protocol constant (`PlaybackV2Api.kt` L19). Whether it appears in `client_features` was not traced (**UNVERIFIED**).
- Evidence is `exact`, from `MediaCodecList` (`silo-android/android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/MediaCodecCapabilitiesProbe.kt` L108-214). Codec names are `h264`, `hevc`, `av1`, `vp9`, `vp8`, `mpeg2video` and `vc1`.
- Playback context: built by `PlaybackCapabilityDetector.detect()` / `detectPlaybackContext` (`silo-android/android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/PlaybackCapabilityDetector.kt` L336-405, L439-648):
  - `platform_details` keys are at L650-672;
  - `form_factor` is one of `tv`, `watch`, `automotive`, `tablet` or `mobile` (L675-684);
  - container lists come from `PlaybackContainerPolicy.kt`.
- Native embedded subtitle codecs (`PlaybackProtocolV3.kt` L784-787):
  - `mp4` supports `mov_text`;
  - `mkv` supports `subrip`, `ass`, `hdmv_pgs_subtitle`, `dvd_subtitle` and `dvb_subtitle`.
- Request body:
  - `quality_preference` is `"auto"`, and tracks use stable track ids;
  - subtitle off is expressed by omitting the fields;
  - the bandwidth fields are populated (`PlaybackSessionManager.kt@ee178d8` L595-625);
  - the v2 wrapper adds `installation_id` and stringifies `file_id` (`PlaybackV2Api.kt` L185-190).
- Android does **not** negotiate `header_authenticated_media_v1`. It plays `st`-signed plan URLs and also attaches Bearer, `X-Profile-*` and `X-Silo-*` headers to same-origin media requests (`MediaAuthInterceptor.kt`, `MediaAuthSession.kt` L224).

### 6.3 Start response: `PlaybackDecision` (OAS:L44188)

```jsonc
{
  "protocol_version": 3,
  "outcome": "playable" /* or a terminal outcome */,
  "session_id": "…",              // only when outcome is playable
  "server_features": ["…"],
  "terminal": { "reason": "…", "message": "…", "retryable": false },  // TerminalV3
  "playback_plan": {              // PlaybackPlan, OAS:L44282
    "protocol_version": 3, "plan_id": "…", "plan_attempt_key": "…",
    "delivery": "original_http|server_remux_progressive|server_remux_hls|server_transcode_hls",
    "stream":   { "url": "…", "protocol": "…", "headers": {…}, "header_refresh": "…",
                  "header_refresh_url": "…", "container": "…", "mime_type": "…" },   // StreamV3
    "timeline": { "source_start_seconds": 0, "stream_origin_seconds": 0,
                  "player_start_seconds": 0, "timeline_offset_seconds": 0,
                  "can_seek_anywhere": true, "seek_restoration": "player_position",
                  "seek_window_start_seconds": 0, "seek_window_end_seconds": 0 },    // TimelineV3
    "selected_tracks": { "audio": {"id":"…","index":1}, "subtitle": {"id":"…"} },
    "subtitle": { "mode": "…", "track_id": "…", "inventory": [ … ],
                  "artifact": {"url","format","mime_type","timing_origin_seconds"},
                  "embedded": {"stream_index", "container_track_id"}, "hls_rendition": {…} },
    "available_qualities": [ … ], "active_quality": …, "bandwidth_limit": …,
    "requested_media_file_id": "…", "effective_media_file_id": "…", "source": {…},
    "effective_recipe": …, "claims": [ … ], "transformations": [ … ],
    "applied_quirks": [ … ], "runtime_corrections": [ … ],
    "degradation_warnings": [ … ], "decision_reason": "…",
    "subtitle_fidelity_policy": "…", "expires_at": "…", "session_id": "…"
  }
}
```

The field names come from OAS `PlaybackDecision`, `PlaybackPlan`, `StreamV3` (OAS:L51929), `TimelineV3` (OAS:L53946) and `SubtitleDecisionV3` (OAS:L52265). The concrete values are illustrative, and the delivery names come from `playback-api.md@6791aac` L369-397.

The full set of `outcome` values was not enumerated (**UNVERIFIED**). The OAS mentions `adaptation_unavailable` as a terminal decision. `426` is returned for protocol and upgrade mismatches (`playback-protocol-v3.md` §2).

**Timeline** (`playback-protocol-v3.md@6791aac` L1011-1050). The four numbers must be kept distinct:

- `source_start_seconds` and `stream_origin_seconds`;
- `player_start_seconds`, which is where to seek after load;
- `timeline_offset_seconds`, which is added to the player position to give the source position.

The routes differ:

- Direct and transcode routes: origin and offset are 0, and the client seeks locally.
- Copy-remux HLS starts at the preceding keyframe, and the offset then matters.
- Source positions are container-relative, where the first container timestamp is 0.
- An engine that keeps raw container timestamps must subtract that origin.

Progress positions are reported as **source** positions.

*Granary note:* mpv's `time-pos` normally rebases to the container start (`--rebase-start-time=yes` is the default). Verify this against MPEG-TS-derived files (**UNVERIFIED**).

**Media URL authentication**:

- Media URLs carry a signed `st` reference. `GET /api/v2/stream/{session_id}` (`getPlaybackMedia`, OAS:L220333) accepts:
  - query `token` (bearer fallback) and `st`;
  - `Range` and `If-*` headers.
- It answers `200` or `206`, with content types covering `video/x-matroska`, `video/mp4`, `video/mp2t` and others.
- Account and viewer authorization are **always** required in addition to `st`. In practice that means a Bearer header or `?token=`, plus the profile.
- With `header_authenticated_media_v1` negotiated (only when the server advertises it):
  - URLs become relative and contain no `st`;
  - the client must send `Authorization` and the profile headers itself (`playback-protocol-v3.md@6791aac` L793-815).
- `stream.headers` and `stream.header_refresh` tell the client which headers to send and when to refresh them (`StreamV3`).

*Granary note:* libmpv can send the needed headers via the `http-header-fields` option. Set it to `Authorization: Bearer …` plus `X-Profile-Id` and `X-Profile-Token` when present, plus any `stream.headers`. This avoids putting tokens in URLs (which also end up in mpv logs). Re-apply the headers after a token refresh, or prefer `header_authenticated_media_v1` when advertised.

### 6.4 Replan: `PlaybackReplanBody` (OAS:L44548)

- `operation` is one of `failure_recovery`, `seek_reanchor`, `seek_failure_recovery`, `track_change`, `quality_change` or `output_change`.
- Required fields: `installation_id`, `protocol_version`, `playback_attempt_id`, `replan_request_id` (client-minted idempotency key), `failed_plan_id`, `plan_attempt_id`, `plan_attempt_key`, `attempted_plan_keys`, `attempt_count`, `quality_preference`, `position_seconds`, … (the list is truncated in the dump; see OAS).
- Optional fields: `client_capabilities`, `client_playback_context`, `client_features`, `selected_tracks`, `failure: FailureV3`, `local_mutations` and the bandwidth fields.
- The response is a whole replacement `PlaybackDecision`.
- Error statuses (`playback-api.md@6791aac` L353-367):
  - `409` covers a reused id with different input, a superseded plan, or the same replan still in flight;
  - `410 playback_session_ended` means an administrator ended the session;
  - `404` means any other ended session.
- Operation rules (`playback-protocol-v3.md` L1103-1118):
  - `quality_change` must name a rung;
  - `track_change`, `quality_change` and `output_change` keep the previous route when possible;
  - `output_change` requires the server feature `output_change_v1` (L129).
- Route events: `POST /api/v2/playback/route-events` (OAS:L198912) takes the v3 event plus `installation_id` and an `event_id`.
  - The server answers `202` (queued, not a durable write).
  - Clients must never retry automatically, and treat `429` as a drop (`playback-api.md` L361-367).

### 6.5 Session liveness and the control socket

- Liveness windows (§2 of `playback-protocol-v3.md`, L330-475): 45 s when playing, 30 min when paused, and a 90 s grace. Steady progress keeps the session alive.
- Control socket:
  - `POST /api/v2/playback/sessions/{session_id}/control/ws-ticket` (`createPlaybackControlSocketTicket`, OAS:L199554), with optional body `installation_id`;
  - then `GET /api/v2/playback/sessions/{session_id}/control/ws` (OAS:L199422);
  - the socket carries server commands such as `plan_invalidated`;
  - hello frame example: `{"type":"hello","session_id":…,"client":{name,version},"capabilities":{"commands":["plan_invalidated"]}}` (`playback-protocol-v3.md` L450; `realtime-api.md@6791aac` L318-373).
- Whether an MVP desktop client must open this socket was not established (**UNVERIFIED**). Admin pause/stop commands that go unanswered end the session with a `410` (`playback-api.md` L339-351).

### 6.6 HLS

- `GET /api/v2/playback/transcode/{session_id}/master.m3u8` (`getPlaybackManifest`, OAS:L200108) takes the query `token`, `st`, `silo_rendition` (`video|subtitles`) and `silo_track`.
- Segments are served at `GET /api/v2/playback/transcode/{session_id}/segment/{name}` (OAS:L200322).
- `hls_copy_mpegts_v1` and `hls_variable_substitution_v1` are v2 extras (`playback-protocol-v3.md` §2).

*Granary note:* libmpv/ffmpeg's HLS demuxer handles relative segment URLs. Header propagation to segments when using `http-header-fields` is expected but **UNVERIFIED** for this server.

## 7. Subtitles

- **The plan's `subtitle.inventory` is authoritative** for what can be shown with the current route (`playback-protocol-v3.md@6791aac` §8, L1573+).
- `SubtitleInventoryItemV3` (OAS:L52462) has these fields:
  - identity: `track_id*`, `combined_index*`, `source*`, `delivery*`, `codec`, `language` (BCP 47), `label`;
  - flags: `forced*`, `hearing_impaired*`, `default*`, `unplayable?`;
  - URLs: `url?`, `font_bundle_url?`, `sync_key?`;
  - embedded-only: `stream_index?` (absolute probed stream index), `container_track_id?` (the Matroska TrackNumber or MP4 track id) and `native_selectable?`;
  - `hls_rendition?`.
- `combined_index` is one ordinal space across embedded and external tracks (`playback-protocol-v3.md` §8).
- Sidecar fetch: **`GET /api/v2/stream/{session_id}/subtitles/{track}`** (`getPlaybackSubtitle`, OAS:L221191).
  - `{track}` ends in `.vtt`, `.ass`, `.sup` or `.srt`.
  - Query: `token`, `st`, `file_id`, `downloaded_subtitle_id`, `original=1` (raw SRT under `subrip_sidecar_v1`), `position` / `duration` (windowed text extraction) and `windowed` (PGS).
  - Responses are `text/vtt`, `text/x-ssa`, `application/x-subrip` or `application/octet-stream`.
- Selecting tracks:
  - at start, use `subtitle_track_id` / `subtitle_track_index`; omitting them means off;
  - mid-playback, a `replan` with `operation: "track_change"` and `selected_tracks` (`playback-protocol-v3.md` L1104, L1152-1182).
  - The old v1 `PATCH` audio route was replaced by `track_change` (`v1-scope.md@6791aac`).
- Native embedded selection: advertise `deliveries.original_http.subtitles.native_embedded` and the `embedded_subtitles_v1` feature (Android, §6.2). The player can then select the embedded track in-process, matching on `container_track_id` or `stream_index`.
- Stored and downloaded subtitles (not needed for an MVP): `GET /api/v2/subtitles/{media_file_id}` (OAS:L225581), `POST /api/v2/subtitles/search` (OAS:L224010) and `POST /api/v2/subtitles/download` (OAS:L223627), plus sync and AI routes (OAS ops list).

*Granary note:* libmpv renders ASS/SSA, SRT, PGS and DVB natively from MKV. Advertise mkv `native_embedded` codecs `subrip`, `ass`, `hdmv_pgs_subtitle`, `dvd_subtitle` and `dvb_subtitle`, with `ass_styling: true` and `font_attachments: true`. Whether the server then always prefers in-process selection over sidecars for `original_http` is **UNVERIFIED**.

## 8. Progress and watch state

### 8.1 During playback

**`POST /api/v2/playback/{session_id}/progress`** (`updatePlaybackProgress`, OAS:L201020):

- Body `PlaybackProgressBody` (OAS:L44515): `{installation_id*: string, sequence*: int64 (>0, strictly increasing per session), position*: number (seconds, source timeline), is_paused*: bool}`.
- The response is `PlaybackMutation` (OAS:L44251): `{outcome*: string, accepted?: {sequence*, position*, is_paused*}, stop_id?, history_id?}`.
- Outcomes (`playback-api.md@6791aac` L281-302):

  | Case | Result |
  | --- | --- |
  | Higher sequence | `applied` |
  | Lower sequence | `stale_sample` (with the latest `accepted`) |
  | Equal sequence, same payload | `replayed` |
  | Equal sequence, different payload | `409 progress_conflict` |
  | Session owned by another profile | `403` |
  | Session ended by an admin | `410 playback_session_ended` |
  | Session stopped or unknown | `404` |

- The cadence is 10 s on Android (`PROGRESS_REPORT_INTERVAL_MS = 10_000L`, `silo-android/android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/PlaybackSessionLifecycle.kt@ee178d8` L953; loop at L680-720). The loop ends on session-ended (410).
- Android model: `PlaybackProgressV2 {installation_id, sequence, position, is_paused}` (`PlaybackV2Api.kt@ee178d8` L160-166). Paths are built at L352-356.

**`DELETE /api/v2/playback/{session_id}`** (`stopPlayback`, OAS:L200687):

- Body `PlaybackStopBody` (OAS:L45666): `{installation_id*, stop_id*: UUID, sequence?, position?, is_paused?}`. `sequence` and `position` are sent together or not at all.
- The first stop wins and returns `200 {outcome: "stopped", stop_id, accepted?, history_id?}`. Every later DELETE returns `200 {outcome: "replayed", …}` with the stored receipt.
- There is no 202 and no polling (`playback-api.md` L304-325).
- Mint the `stop_id` once and reuse it on every retry. Server-minted stop ids are refused with `400`.
- After a `410`, do **not** restart at the same position. After a `404` (expiry or restart), a fresh start is the recovery path (`playback-api.md` L339-351).

**Thresholds** (server defaults):

- `playback.watched_threshold = "90"` and `playback.min_resume_threshold = "5"` (`silo-server/internal/config/admin_settings.go@6791aac` L458-459).
- `ResolveProgressState` (`silo-server/internal/userstore/threshold.go@6791aac` L5-50):
  - below min-resume, nothing is recorded as a resume position;
  - strictly above the watched threshold (`position/duration > WatchedFraction` in `threshold.go`), the item is completed and the position reset to 0.
- These are admin-configurable, so the client should not hard-code them for UI decisions. Use `user_data.played` and `position_seconds` from the server.

### 8.2 Outside playback

| Op | Notes |
| --- | --- |
| `POST /api/v2/watched/{id}` (`markWatched`, OAS:L234696) | `204`. `id` can be a movie, audiobook, ebook, episode, season or series; a season or series expands to its episodes. |
| `DELETE /api/v2/watched/{id}` (`unmarkWatched`, OAS:L234547) | `204` |
| `DELETE /api/v2/progress/{media_item_id}/position` (`clearResumePosition`, OAS:L206366) | `204`. Movie or episode only. |
| `GET /api/v2/progress` (`listProgress`, OAS:L205961) | Query `status=in_progress\|completed`, `library_id`, `limit` (≤200) and `cursor`. Returns `{items*: [ProgressEntry {media_item_id*, position_seconds*, duration_seconds* (0 = unknown), completed*, updated_at*}], page?}` (OAS:L47651). |
| `GET /api/v2/progress/capabilities` (OAS:L206165) | Shape not dumped. |
| `GET /api/v2/history` (`listHistory`, OAS:L177051) | `{items*: [HistoryCard], page?}`, most recent first. |

## 9. WebSocket events

### 9.1 Handshake and lifetime

1. Optionally call `GET /api/v2/events/capabilities` (`getEventsCapabilities`, OAS:L175755). It returns `{state*, allowed*, schema_version*, subscribe_frame*, declared_channels*, subscribe_grace_period_seconds*, max_requested_channels*, channels*, revision*}`. First-party clients do not call it (`realtime-api.md@6791aac` L1-24).
2. Mint a ticket with `POST /api/v2/events/ws-ticket` (`createEventsSocketTicket`, OAS:L176024), sending Bearer and (optionally) the profile headers.
   - The response is `{ticket*, expires_in*, max_connection_seconds*: 300, protocol*: "silo.events.v2"}` (OAS:L34928).
   - The ticket is opaque, expires within 30 s, is single-use, and is bound to the session, role and profile proof (`realtime-api.md` L36-50).
3. Connect with `GET /api/v2/events/ws` (`connectEventsSocket`, OAS:L175913).
   - Offer the subprotocols **`silo.events.v2`, `silo.ticket.<ticket>`**, in that order.
   - Do not put a token in the URL. The optional query `channels` is a comma-separated list.
   - Native clients may omit `Origin` (`realtime-api.md` L52-66).
   - Android uses `encodedPath "/api/v2/events/ws"` and strips the profile headers from the socket request (`EventsSocketV2Api.kt@ee178d8` L139, L146).
4. Lifetime:
   - the connection closes at access-token expiry or after **5 minutes**, whichever comes first;
   - policy is rechecked every 15 s;
   - reconnect with a **new** ticket (`realtime-api.md` L73-81).
5. Compression: `permessage-deflate` is accepted. An oversized message is closed with `1009` (`realtime-api.md` L26-34).

### 9.2 Frames

The frame structs are defined in `silo-server/internal/events/types.go@6791aac`:

- Server sends `hello`: `{type:"hello", schema_version, connection_id, available_channels: [..], required_action}`.
- Client sends `subscribe`: `{type:"subscribe", request_id?, channels: [..]}`.
- Server replies `subscribed`: `{type:"subscribed", request_id?, channels, rejected?: [{channel, code, message}]}`.
- Server may send `snapshot`: `{type:"snapshot", channel, timestamp, data}`.
- Events arrive in an `event` envelope: `{type:"event", channel, event, event_id, timestamp, data}`. An example is at `realtime-api.md` L178-180.
- Server may send `error`.
- Server sends `access_changed`: `{"type":"access_changed"}`, then closes with code **4001**, reason `access_changed`.
  - On it, refetch libraries, home, search and item details, and reconnect at once with a new ticket.
  - If the ticket mint returns `401 token_refresh_required`, refresh and retry (`realtime-api.md` L257-305).

Client channel names (`types.go` `ClientChannels`): `catalog`, `jobs`, `sessions`, `tasks`, `scans`, `history_import`, `user_state`, `user_settings`, `settings`, `notifications`, `download_preparations`. The admin-only channels for a viewer are `sessions`, `jobs`, `scans`, `tasks` and others (`realtime-api.md` L66-81).

Useful for a viewer client:

- **`user_state` / `user_state.changed`**:
  - data is `{profile_id, content_id?, series_id?, change, played?, is_favorite?, in_watchlist?, not_interested?}` (`silo-server/internal/api/handlers/user_state_events.go@6791aac` L1-55);
  - observed `change` literals are `progress`, `watched`, `watchlist`, `not_interested` and `home_dismissal` (grep of call sites; the list may be incomplete, **UNVERIFIED**);
  - delivery is restricted to the bound profile (`realtime-api.md` L123-153);
  - Android filters on `user_state.changed` and refetches Home (`silo-android/shared/src/commonMain/kotlin/org/siloserver/silo/network/HomeRealtimeClient.kt@ee178d8` L66-110).
- **`catalog` / `catalog.item.changed`**:
  - data is `{library_id, content_id, change}`;
  - viewers receive only `change: "metadata_updated"`, while v2 admins also receive `added`, `removed` and `files_changed`;
  - also `catalog.library.changed`;
  - delivery is at most once, so re-list after a reconnect (`realtime-api.md` L155-202).
- `user_settings`: setting key and scope only, never the value (`realtime-api.md` L123-150).

## 10. Chapters, markers and trickplay

- **Chapters**: `WatchFileVersion.chapters` / `FileVersion.chapters` are `VersionChapter {index*, title*, start_seconds*, end_seconds*, source*, thumbnail_url?, thumbnail_thumbhash?}` (OAS:L54617).
- **Markers**:
  - Per version: `WatchFileVersion.marker_segments*: [MarkerOccurrence {kind*: intro|credits|recap|preview, start_seconds*, end_seconds*, thumbnail_url?, thumbnail_thumbhash?, thumbnail_capture_seconds?}]` (OAS:L39442), plus the singular `intro`, `credits`, `recap` and `preview` (`playback-api.md@6791aac` L116-162).
  - Standalone reads: `GET /api/v2/markers/items/{item_id}` (`getItemMarkers`, OAS:L188568) and `GET /api/v2/markers/files/{file_id}` (`getFileMarkers`, OAS:L188030). Both return `FileMarkers {file_id*, intro*, credits*, recap*, preview*: MarkerSegment {start_seconds, end_seconds, source, provider, algorithm, confidence, detected_at}, marker_segments*}`.
  - `PUT` variants exist for editing.
  - Thumbnails are negotiated with the `marker_thumbnails_v1` feature. The `marker_thumbnail_ready` event is native v2 and negotiated (`realtime-api.md` L769+).
- Profile auto-skip flags: `Profile.auto_skip_intro`, `auto_skip_credits`, `auto_skip_recap` and `auto_play_next_preview` (OAS:L46528).
- **Trickplay**: `GET /api/v2/watch/{id}/trickplay?file_id=` (`getWatchTrickplay`, OAS:L234363). Use it only when `trickplay_available` is true. It returns `WatchTrickplay`:
  - `{file_id*, interval_ms*, thumbnail_width*, thumbnail_height*, tile_columns*, tile_rows*, thumbnail_count*, expires_at*, sheets*: [{index*, url*}]}`;
  - each sheet URL is a JPEG that "carries its own authorization" (OAS:L57928);
  - refetch after `expires_at`.
  - Thumbnail `n` is on sheet `floor(n / (cols*rows))`, at the tile position within that sheet. That arithmetic is derived from the field names and is **UNVERIFIED** against server code.

## 11. Next-up and continue watching

- Home:
  - `GET /api/v2/home/sections` (`listHomeSections`, OAS:L179770) returns `{sections*: [Section]}`. It takes the query `item_limit` and `image_size`, and the header `X-Silo-Client-Family`.
  - `GET /api/v2/home/sections/{id}/items` (`getHomeSectionItems`, OAS:L180040) returns one `Section`.
  - Also `GET /api/v2/home/layout` (OAS:L179483) and `GET /api/v2/home/capabilities` (OAS:L178908).
- `Section` (OAS:L49929) is `{id*, title*, section_type*, featured*, customized*, is_custom*, item_limit*, total_count*, items*: [CatalogItem], has_more?}`.
- Section types (`silo-server/internal/sections/types.go@6791aac` L10-30):
  - `continue_watching`, `next_up`, `recently_added`, `recently_released`;
  - `watchlist`, `favorites`, `genre`, `custom_filter`, `random`, `collection`;
  - `recommended_for_you`, `because_you_watched`, `similar_users_liked`, `taste_match`, `next_in_series`, `hidden_gems`, …
- Continue-watching semantics (`silo-server/docs/catalog-api.md@6791aac` L484-498):
  - With the profile setting `ui.next_up_mode = combined` (the default), the **Continue Watching** row merges in-progress cards with next-up episodes. It keeps one card per series and orders by most recent activity.
  - Each card's `item_source` is `in_progress` or `next_up`. `position_seconds` and `progress_updated_at` are set on resume cards.
  - With `separate`, the server injects a synthetic row `id: "system-next-up"`, `section_type: "next_up"` after Continue Watching (`silo-server/internal/api/handlers/sections.go@6791aac` L2072-2090).
- Raw in-progress data is available as `GET /api/v2/progress?status=in_progress` (§8.2).
- A dedicated "next episode after this one" endpoint for post-play autoplay was **not found** (**UNVERIFIED**). Candidates are the `next_in_series` section type or computing it from `listSeasonEpisodes`.
- Dismissing an item from a surface: `PUT` / `DELETE /api/v2/home/dismissals/{surface}/{item_id}` (OAS ops list).

## 12. Version-dependent and unstable areas

| Area | Risk | Evidence |
| --- | --- | --- |
| `/api/v1` | Frozen. It becomes `410 client_upgrade_required` at the 1.0 cutover. Never use it except `/api/v1/health` and `/api/v1/ready`. | `api-contract.md@6791aac` L1811-1835 |
| Legacy v1 playback start | Returns `426`. `/playback/transcode/start` was removed. | `v1-scope.md@6791aac` removal table |
| Pre-1.0 releases | No tagged release yet. Minor versions may break. Builds are published as `build-N` and `latest`. | `release-versioning.md@6791aac` |
| Playback features | Many opt-in v2 features (`header_authenticated_media_v1`, `authorized_media_origins_v1`, `buffering_quality_v1`, `playback_plan_summary_v1`, `subrip_sidecar_v1`, `marker_thumbnails_v1`, `hls_subtitle_renditions_v1`, `hls_subtitle_renditions_all_v1`, `hls_remux_subtitle_renditions_v1`, `plan_active_quality_v1`, `hls_copy_mpegts_v1`, `hls_variable_substitution_v1`, `codec_channel_ceilings_v1`, `native_embedded_limitations_v1`, `output_change_v1`, `software_video_decode_v1`, …). Always intersect them with the server's `features[]`. | `playback-protocol-v3.md@6791aac` §2 (L70-140) |
| Capability endpoints | Carry `revision` and support `If-None-Match`. The client should cache by revision. | OAS `getPlaybackCapabilities`, `getImageCapabilities` |
| Server docs vs reality | `playback-api.md` L472-497 says Apple and Android still use v1 for playback, but Android at `ee178d8` uses `/api/v2/playback/*`. Server docs can lag. | `PlaybackV2Api.kt@ee178d8` L214-366 |
| Realtime | Per-connection cap of 5 min, a 30 s ticket, the `4001` close code, and at-most-once delivery (no replay). | `realtime-api.md@6791aac` L36-90, L257-305 |
| Artwork URLs | Presigned and short-lived. | OAS `CatalogItem` |
| Thresholds | Admin settings, not constants. | `admin_settings.go@6791aac` L458-459 |
| Rolling deploys | Some events, such as lifecycle events, are absent on older nodes. | `realtime-api.md@6791aac` L250-256 |

---

## Endpoint table

| Method | Path | Purpose | Citation | Status |
| --- | --- | --- | --- | --- |
| GET | /api/v2/system/info | Pre-login probe, `api_major` | OAS:L227535 `getSystemInfo`; `ApiV2Probe.kt@ee178d8` L47-140 | VERIFIED |
| GET | /api/v2/system/identity | `server_id` | OAS:L227455 | VERIFIED |
| GET | /api/v2/system/connections | Alternative addresses | OAS:L227297 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/system/setup | Setup status | OAS:L227604 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/openapi.json, /api/v2/docs | Contract, Swagger | `docs/api-docs.md@6791aac` L1-33 | VERIFIED |
| GET | /api/v1/health, /api/v1/ready | Liveness/readiness | `router.go@6791aac` L3290-3292; `health.go` L30-80 | VERIFIED |
| POST | /api/v2/auth/login | Sign in → TokenPair | OAS:L144782 | VERIFIED |
| POST | /api/v2/auth/refresh | Rotate tokens | OAS:L146357; `auth/service.go@6791aac` L1184-1230 | VERIFIED |
| POST | /api/v2/auth/logout | End session | OAS:L144975 | VERIFIED |
| GET | /api/v2/account/me | Current account | OAS:L59905 | VERIFIED |
| GET | /api/v2/profiles | List profiles | OAS:L203258 | VERIFIED |
| POST | /api/v2/profiles/{id}/verify-pin | PIN → profile_token | OAS:L205764 | VERIFIED |
| GET | /api/v2/user/libraries | Viewer libraries | OAS:L227883; `PersonalDataApi.kt@ee178d8` L59 | VERIFIED |
| GET | /api/v2/library/{id}/sections | Library page rows | OAS:L187307; `LibrarySectionItemsV2Api.kt@ee178d8` L32 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/library/{id}/sections/{section_id}/items | Row items | OAS:L187591 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/catalog | Browse/search/filter | OAS:L150065 | VERIFIED |
| POST | /api/v2/catalog/query | Body-form browse | OAS:L155465 | VERIFIED |
| GET | /api/v2/catalog/filters | Facets | OAS:L151423 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/catalog/search/capabilities | Search mode | OAS:L155758 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/catalog/items/{id} | Detail page | OAS:L152009 | VERIFIED |
| GET | /api/v2/catalog/items/{id}/versions | File versions | OAS:L154461 | VERIFIED |
| GET | /api/v2/catalog/items/{id}/episodes | Episodes of item | OAS:L152574 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/catalog/series/{id}/seasons | Seasons | OAS:L155969 | VERIFIED |
| GET | /api/v2/catalog/series/{id}/seasons/{num}/episodes | Season episodes | OAS:L156533 | VERIFIED |
| GET | /api/v2/watch/{id} | Pre-play state, versions, tracks, resume | OAS:L234067 | VERIFIED |
| GET | /api/v2/watch/{id}/trickplay | Seek previews | OAS:L234363 | VERIFIED |
| GET | /api/v2/markers/items/{item_id} | Markers by item | OAS:L188568 | VERIFIED |
| GET | /api/v2/markers/files/{file_id} | Markers by file | OAS:L188030 | VERIFIED |
| GET | /api/v2/artwork/{key}?exp&sig | Artwork bytes (public, signed) | OAS:L142607 | VERIFIED |
| GET | /api/v2/images/capabilities | Image sizes | OAS:L180324 | VERIFIED |
| GET | /api/v2/playback/capabilities | installation_id, features, deliveries | OAS:L198406; `PlaybackV2Api.kt@ee178d8` L214 | VERIFIED |
| GET | /api/v2/playback/bandwidth-probe | Probe payload | OAS:L198085 | VERIFIED |
| POST | /api/v2/playback/bandwidth-probe/results | Report probe | OAS:L198221; `PlaybackV2Api.kt` L296 | VERIFIED |
| GET | /api/v2/playback/connection | Learned limit | OAS:L198607; `PlaybackV2Api.kt` L224 | VERIFIED |
| DELETE | /api/v2/playback/connection/learned-limit | Reset learned limit | OAS ops list; `PlaybackV2Api.kt` L230 | VERIFIED |
| POST | /api/v2/playback/start | **Negotiate playback** | OAS:L199765; `PlaybackV2Api.kt` L316 | VERIFIED |
| POST | /api/v2/playback/{session_id}/replan | Replan | OAS:L201353; `PlaybackV2Api.kt` L323-326 | VERIFIED |
| POST | /api/v2/playback/{session_id}/progress | Progress sample | OAS:L201020; `PlaybackV2Api.kt` L352-356 | VERIFIED |
| DELETE | /api/v2/playback/{session_id} | Stop | OAS:L200687; `PlaybackV2Api.kt` L363-366 | VERIFIED |
| POST | /api/v2/playback/route-events | Route telemetry | OAS:L198912; `PlaybackV2Api.kt` L335 | VERIFIED |
| GET | /api/v2/stream/{session_id} | Media bytes (Range) | OAS:L220333 | VERIFIED |
| GET | /api/v2/stream/{session_id}/subtitles/{track} | Subtitle sidecar | OAS:L221191 | VERIFIED |
| GET | /api/v2/playback/transcode/{session_id}/master.m3u8 | HLS manifest | OAS:L200108 | VERIFIED |
| GET | /api/v2/playback/transcode/{session_id}/segment/{name} | HLS segment | OAS:L200322 | VERIFIED |
| POST | /api/v2/playback/sessions/{session_id}/control/ws-ticket | Control socket ticket | OAS:L199554; `realtime-api.md` L318+ | VERIFIED |
| GET | /api/v2/playback/sessions/{session_id}/control/ws | Control socket | OAS:L199422 | VERIFIED |
| GET | /api/v2/progress | Progress list (in_progress / completed) | OAS:L205961 | VERIFIED |
| GET | /api/v2/progress/capabilities | Progress caps | OAS:L206165 | VERIFIED (shape UNVERIFIED) |
| DELETE | /api/v2/progress/{media_item_id}/position | Clear resume | OAS:L206366 | VERIFIED |
| POST | /api/v2/watched/{id} | Mark watched | OAS:L234696 | VERIFIED |
| DELETE | /api/v2/watched/{id} | Unmark watched | OAS:L234547 | VERIFIED |
| GET | /api/v2/history | Watch history | OAS:L177051 | VERIFIED |
| GET | /api/v2/home/sections | Home rows incl. continue watching / next up | OAS:L179770 | VERIFIED |
| GET | /api/v2/home/sections/{id}/items | One home row | OAS:L180040 | VERIFIED |
| GET | /api/v2/home/layout | Home layout | OAS:L179483 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/subtitles/{media_file_id} | Stored subtitles | OAS:L225581 | VERIFIED (shape UNVERIFIED) |
| GET | /api/v2/events/capabilities | Event protocol caps | OAS:L175755 | VERIFIED |
| POST | /api/v2/events/ws-ticket | Events ticket | OAS:L176024 | VERIFIED |
| GET | /api/v2/events/ws | Events socket | OAS:L175913; `EventsSocketV2Api.kt@ee178d8` L139 | VERIFIED |
| GET | /health (root) | Android health call | `HealthApi.kt@ee178d8`; not in `router.go` L3288-3292 | UNVERIFIED |

## Open questions / UNVERIFIED

1. **Evidence tier for libmpv**: `declared` matches a decoder stack that cannot enumerate hardware limits; `exact` requires `video_decode[]` with hardware entries. Which tier the server treats best for a desktop and libmpv client, and whether `form_factor: "desktop"` is accepted (Android only sends `tv|watch|automotive|tablet|mobile`), is **UNVERIFIED**. Server tests do set `ClientPlaybackContext.FormFactor = "desktop"` (`silo-server/internal/playback/protocol_v3_test.go@6791aac` L2860, L4243), so `desktop` is very likely accepted; the request-validation path is still to be confirmed against a live server.
2. **Full `outcome` vocabulary** of `PlaybackDecision` and `PlaybackMutation` beyond `playable`, `applied`, `stale_sample`, `replayed` and `stopped`: **UNVERIFIED**.
3. **Whether `sequenced_progress_v1`** must appear in `client_features`: **UNVERIFIED**. The constant exists in Android `PlaybackV2Api.kt` L19.
4. **Root `/health`**: how Android's `GET /health` resolves on the server is **UNVERIFIED**.
5. **The playback control socket** (`plan_invalidated`, admin commands): whether a client that does not open it is penalised beyond `410` on unanswered admin commands is **UNVERIFIED**.
6. **HLS segment authentication with mpv `http-header-fields`**: whether segment requests must carry `st` or the headers when `header_authenticated_media_v1` is not negotiated is **UNVERIFIED**. The manifest URLs are expected to be self-authorising via `st`.
7. **Value vocabularies** of `VideoTrack.video_range_type` and `dolby_vision`, and the delivery `features` strings expected per class (for example `native_hls_playback_v1`): **UNVERIFIED**.
8. **Shapes not dumped**: `Season`, `Episode`, `CatalogItemDetail` (full), library section items, `HistoryCard`, `getCatalogFilters`, `getServerConnections`, `FailureV3`, `TerminalV3.reason` values. All are **UNVERIFIED** in detail.
9. **Post-play "next episode" endpoint**: none found. Candidates are the `next_in_series` section or `listSeasonEpisodes` (**UNVERIFIED**).
10. **Trickplay tile indexing arithmetic**: **UNVERIFIED**.
11. **The `user_state.changed` `change` list** may be incomplete. For example, a favourite toggle may pass the change string through a variable (**UNVERIFIED**).
12. **The Apple client (`@77def09`)** was not cross-checked.

## Sources

All paths are relative to the upstream repos cloned under `C:\Users\mr\Documents\ACLAUDE\granary\.ref\`.

Not needed for the viewer/playback contract and therefore not mined in depth: `silo-server/docs/settings-api.md`,
`silo-server/docs/downloads-api.md` and `silo-server/contracts/settings/v1` (server/admin settings and offline
downloads, both out of Granary's v0.1 scope).

silo-server @ `6791aac867664c41c55ee82909fe063d145911a6`:

- `contracts/api/v2/openapi.json` (operations and component schemas at the line numbers cited above)
- `docs/api-docs.md`
- `docs/release-versioning.md`
- `docs/auth-api.md`
- `docs/playback-api.md`
- `docs/realtime-api.md`
- `docs/catalog-api.md`
- `docs/architecture/api-contract.md`
- `docs/architecture/v1-scope.md`
- `docs/architecture/playback-protocol-v3.md`
- `internal/auth/jwt.go`
- `internal/auth/service.go`
- `internal/config/admin_settings.go`
- `internal/api/middleware/auth.go`
- `internal/api/router.go`
- `internal/api/handlers/health.go`
- `internal/api/handlers/playback.go` (L479-505, v1 thresholds and progress request)
- `internal/api/handlers/sections.go`
- `internal/api/handlers/user_state_events.go`
- `internal/userstore/threshold.go`
- `internal/sections/types.go`
- `internal/events/types.go`

silo-android @ `ee178d887ff02ce74150796e5f80934416bf88a7`:

- `shared/src/commonMain/kotlin/org/siloserver/silo/network/AuthInterceptorImpl.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/HomeRealtimeClient.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/api/HealthApi.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/api/PersonalDataApi.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/ApiV2Probe.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/PlaybackV2Api.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/EventsSocketV2Api.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/CatalogV2Api.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/network/apiv2/LibrarySectionItemsV2Api.kt`
- `shared/src/commonMain/kotlin/org/siloserver/silo/model/playback/PlaybackProtocolV3.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/network/AndroidDeviceMetadataProvider.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/PlaybackCapabilityDetector.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/MediaCodecCapabilitiesProbe.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/PlaybackSessionManager.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/PlaybackSessionLifecycle.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/MediaAuthInterceptor.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/MediaAuthSession.kt`
- `android-shared/src/androidMain/kotlin/org/siloserver/silo/common/player/video/PlaybackContainerPolicy.kt`

silo-apple @ `77def097c47d69c5796a10b93087545b4999a2df`: not consulted for this note.

External URLs referenced by upstream docs (not fetched):

- RFC 9457 Problem Details
- RFC 7692 permessage-deflate
- https://github.com/Silo-Server/silo-server/issues/886 (v1 retirement tracking)
