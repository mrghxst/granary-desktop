//! The [`SiloApi`] trait (one method per endpoint Granary uses, all listed in docs/research/silo-api.md) and its
//! HTTP implementation. Methods are stateless: tokens and refresh live in [`crate::session::Session`].
use crate::catalog::*;
use crate::device::{CLIENT_FAMILY, CLIENT_NAME, DEVICE_PLATFORM, DeviceIdentity};
use crate::error::{ApiError, Result};
use crate::media::{FORWARDED_REQUEST_HEADERS, FORWARDED_RESPONSE_HEADERS, MediaRequest, MediaResponse};
use crate::models::*;
use crate::playback::*;
use crate::secret::Secret;
use crate::together::{RoomGrant, RoomSelection, TogetherCapabilities};
use async_trait::async_trait;
use reqwest::{Method, RequestBuilder, StatusCode};
use serde::de::DeserializeOwned;
use std::time::Duration;

/// Selected profile: `X-Profile-Id` and, for a PIN-locked profile, `X-Profile-Token` (silo-api.md §3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileAuth {
    pub id: String,
    pub token: Option<Secret>,
}

/// An image body fetched for the WebView (artwork proxy, Phase 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageData {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// Largest artwork body accepted (bytes); larger responses are refused rather than buffered.
pub const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// Largest theme tune the app loads (it is decoded in memory).
pub const MAX_AUDIO_BYTES: usize = 32 * 1024 * 1024;
/// Largest ebook file the reader opens in memory (the web reader's limit, `docs/ebooks-api.md@749db96a`).
pub const MAX_EBOOK_BYTES: usize = 512 * 1024 * 1024;
/// Container and codec pairs sent as `accepted_formats` of a theme grant.
const THEME_FORMATS: &[(&str, &str)] = &[
    ("mp3", "mp3"),
    ("mp4", "aac"),
    ("m4a", "aac"),
    ("aac", "aac"),
    ("flac", "flac"),
    ("ogg", "vorbis"),
    ("ogg", "opus"),
    ("opus", "opus"),
    ("wav", "pcm"),
];

/// Raster image types Granary displays. SVG is refused: it can carry script, and the artwork scheme is served from
/// an origin Tauri treats as local (phase-5 review M3).
pub const IMAGE_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp", "image/avif", "image/gif"];

/// The essence (`type/subtype`, lower case) of a Content-Type if it is an allowed raster type.
pub fn raster_image_type(content_type: &str) -> Option<&'static str> {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    IMAGE_TYPES.iter().copied().find(|t| *t == essence)
}

/// Credentials for one authenticated request.
#[derive(Debug, Clone)]
pub struct Auth {
    pub access: Secret,
    pub profile: Option<ProfileAuth>,
}

#[async_trait]
pub trait SiloApi: Send + Sync {
    /// `GET /api/v2/system/info` (getSystemInfo), public.
    async fn system_info(&self, origin: &str) -> Result<SystemInfo>;
    /// `GET /api/v2/system/identity` (getServerIdentity), public.
    async fn server_identity(&self, origin: &str) -> Result<ServerIdentity>;
    /// `POST /api/v2/auth/login` (login), public; device headers recorded on the session.
    async fn login(&self, origin: &str, username: &str, password: &Secret) -> Result<TokenPair>;
    /// `POST /api/v2/auth/refresh` (refreshSession), public; rotates the pair.
    async fn refresh(&self, origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens>;
    /// `POST /api/v2/auth/logout` (logout), bearer.
    async fn logout(&self, origin: &str, auth: &Auth) -> Result<()>;
    /// `GET /api/v2/account/me` (getCurrentUser), bearer.
    async fn current_user(&self, origin: &str, auth: &Auth) -> Result<Account>;
    /// `GET /api/v2/profiles` (listProfiles), bearer.
    async fn list_profiles(&self, origin: &str, auth: &Auth) -> Result<ProfileCollection>;
    /// `POST /api/v2/profiles/{id}/verify-pin` (verifyProfilePIN), bearer.
    async fn verify_profile_pin(
        &self,
        origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification>;
    /// `GET /api/v2/user/libraries` (listUserLibraries), bearer + profile headers.
    async fn list_user_libraries(&self, origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>>;
    /// `GET /api/v2/home/sections` (listHomeSections), profile-scoped; sends `X-Silo-Client-Family`.
    async fn home_sections(&self, origin: &str, auth: &Auth) -> Result<SectionCollection>;
    /// `GET /api/v2/catalog` (listCatalogItems), profile-scoped.
    async fn catalog_browse(&self, origin: &str, auth: &Auth, query: &CatalogQuery) -> Result<CatalogBrowseCollection>;
    /// `GET /api/v2/catalog/items/{id}` (getCatalogItem), profile-scoped.
    async fn catalog_item(&self, origin: &str, auth: &Auth, id: &str) -> Result<CatalogItemDetail>;
    /// `GET /api/v2/catalog/series/{id}/seasons` (listSeriesSeasons), profile-scoped.
    async fn series_seasons(&self, origin: &str, auth: &Auth, series_id: &str) -> Result<SeasonCollection>;
    /// `GET /api/v2/catalog/series/{id}/seasons/{num}/episodes` (listSeasonEpisodes), profile-scoped.
    async fn season_episodes(
        &self,
        origin: &str,
        auth: &Auth,
        series_id: &str,
        season: i64,
    ) -> Result<EpisodeCollection>;
    /// `GET /api/v2/watch/{id}` (getWatchState), profile-scoped.
    async fn watch_state(&self, origin: &str, auth: &Auth, id: &str) -> Result<WatchDetail>;
    /// Fetch an artwork URL (presigned `getArtwork` link or an absolute image URL) **without** any credential or
    /// device header: artwork links carry their own `exp`/`sig` (silo-api.md §5.3).
    async fn fetch_image(&self, url: &str) -> Result<ImageData>;

    // Playback protocol v3 (Phase 6, silo-api.md §13, ADR-0008). All profile-scoped.
    /// `getPlaybackCapabilities`.
    async fn playback_capabilities(&self, origin: &str, auth: &Auth) -> Result<PlaybackCapabilities>;
    /// `startPlayback` (201 `PlaybackDecision`).
    async fn start_playback(&self, origin: &str, auth: &Auth, body: &PlaybackStartBody) -> Result<PlaybackDecision>;
    /// `updatePlaybackProgress`.
    async fn playback_progress(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackProgressBody,
    ) -> Result<PlaybackMutation>;
    /// `stopPlayback` (DELETE with a body).
    async fn stop_playback(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackStopBody,
    ) -> Result<PlaybackMutation>;
    /// `replanPlayback`.
    async fn replan_playback(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackReplanBody,
    ) -> Result<PlaybackDecision>;
    /// `getWatchTrickplay`.
    async fn watch_trickplay(&self, origin: &str, auth: &Auth, id: &str, file_id: &str) -> Result<WatchTrickplay>;
    /// `createEventsSocketTicket`.
    async fn events_ticket(&self, origin: &str, auth: &Auth) -> Result<EventsSocketTicket>;
    /// Stream a media URL from a playback plan (`getPlaybackMedia`/`Manifest`/`Segment`/`Subtitle`).
    async fn fetch_media(&self, request: MediaRequest) -> Result<MediaResponse>;
    /// `addFavorite`/`deleteFavorite`, `addToWatchlist`/`deleteWatchlistEntry`, `markWatched`/`unmarkWatched`,
    /// `markNotInterested`/`unmarkNotInterested` (all `204`).
    async fn set_flag(&self, origin: &str, auth: &Auth, flag: UserFlag, id: &str, on: bool) -> Result<()>;
    /// `dismissHomeItem`: `PUT /api/v2/home/dismissals/{surface}/{item_id}` (`204`), surface `continue_watching`
    /// (needs `progress_updated_at`) or `next_up` (needs `series_id`) (`internal/apiv2/home.go@6791aac` L65-81,
    /// `internal/api/router.go@6791aac` L3933-3937).
    async fn dismiss_home(&self, origin: &str, auth: &Auth, dismissal: &HomeDismissal) -> Result<()>;
    /// `setRating` (1–5 stars) / `deleteRating` (`None`).
    async fn set_rating(&self, origin: &str, auth: &Auth, id: &str, rating: Option<i64>) -> Result<()>;
    /// `getPerson`.
    async fn person(&self, origin: &str, auth: &Auth, id: &str) -> Result<Person>;
    /// `listLibrarySections` (a library's Recommended tab).
    async fn library_sections(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<SectionCollection>;
    /// `getLibraryCollections`.
    async fn library_collections(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<LibraryCollectionTab>;
    /// `getLibraryCollectionItems`.
    async fn library_collection_items(
        &self,
        origin: &str,
        auth: &Auth,
        library_id: &str,
        collection_id: &str,
        page: &PageQuery,
    ) -> Result<CatalogItemCollection>;
    /// `listSimilar`.
    async fn similar(&self, origin: &str, auth: &Auth, id: &str) -> Result<CatalogItemCollection>;
    /// `getCatalogFilters` for one library.
    async fn catalog_filters(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<CatalogFilters>;
    /// `listCollections`: the profile's own collections, then those shared with it.
    async fn collections(&self, origin: &str, auth: &Auth) -> Result<PersonalCollectionCollection>;
    /// `listServerCollections`.
    async fn server_collections(&self, origin: &str, auth: &Auth) -> Result<ServerCollections>;
    /// `getCollection`.
    async fn collection(&self, origin: &str, auth: &Auth, id: &str) -> Result<PersonalCollection>;
    /// `getCatalogBoxSet`.
    async fn box_set(&self, origin: &str, auth: &Auth, id: &str, library_id: &str) -> Result<BoxSet>;
    /// `getCatalogItemBoxSet`.
    async fn item_box_set(&self, origin: &str, auth: &Auth, item_id: &str) -> Result<CatalogItemBoxSet>;
    /// `createThemeSongPlayback`: a grant for one theme's audio in a format Chromium decodes.
    async fn theme_playback(&self, origin: &str, auth: &Auth, owner_id: &str, theme_id: &str) -> Result<ThemeGrant>;
    /// Fetch granted audio (no credentials: the grant is in the URL), up to [`MAX_AUDIO_BYTES`].
    async fn fetch_audio(&self, url: &str) -> Result<Vec<u8>>;
    /// `getProfileSectionSettings` for the home page: its rows as the profile sees them, hidden ones included.
    async fn home_row_settings(&self, origin: &str, auth: &Auth) -> Result<HomeRowSettingCollection>;
    /// `listProfileSectionOverrides` for the home page.
    async fn home_row_overrides(&self, origin: &str, auth: &Auth) -> Result<HomeRowOverrideCollection>;
    /// `replaceProfileSectionOverrides` for the home page (`204`): the complete set replaces what was saved.
    async fn home_rows_replace(&self, origin: &str, auth: &Auth, overrides: &[HomeRowOverrideWrite]) -> Result<()>;
    /// `resetProfileSectionOverrides` for the home page (`204`): back to the administrator's layout.
    async fn home_rows_reset(&self, origin: &str, auth: &Auth) -> Result<()>;
    /// `getSubtitleProviderStatus`.
    async fn subtitle_provider_status(&self, origin: &str, auth: &Auth) -> Result<SubtitleFeatureStatus>;
    /// `getSubtitleAIStatus`.
    async fn subtitle_ai_status(&self, origin: &str, auth: &Auth) -> Result<SubtitleFeatureStatus>;
    /// `searchSubtitles` for one media file.
    async fn subtitle_search(
        &self,
        origin: &str,
        auth: &Auth,
        media_file_id: &str,
        languages: &[String],
    ) -> Result<SubtitleSearchResults>;
    /// `downloadSubtitle` (`non_retryable`: sent once).
    async fn subtitle_download(
        &self,
        origin: &str,
        auth: &Auth,
        body: &SubtitleDownload,
    ) -> Result<SubtitleDownloadResult>;
    /// `createSubtitleAIJob` with `kind: translate` (`non_retryable`: sent once).
    async fn subtitle_translate(
        &self,
        origin: &str,
        auth: &Auth,
        body: &SubtitleTranslate,
    ) -> Result<SubtitleAiJobEnvelope>;
    /// `getSubtitleAIJob`.
    async fn subtitle_ai_job(&self, origin: &str, auth: &Auth, job_id: &str) -> Result<SubtitleAiJobEnvelope>;
    /// `getEffectiveSubtitleAppearance` (device headers name this computer).
    async fn subtitle_appearance(&self, origin: &str, auth: &Auth) -> Result<SubtitleAppearanceSetting>;
    /// `updateSubtitleAppearanceDeviceOverride`: `value` is the appearance as a JSON document.
    async fn subtitle_appearance_set(
        &self,
        origin: &str,
        auth: &Auth,
        value: &str,
    ) -> Result<SubtitleAppearanceSetting>;
    /// `deleteSubtitleAppearanceDeviceOverride` (`204`): the profile's appearance applies again.
    async fn subtitle_appearance_reset(&self, origin: &str, auth: &Auth) -> Result<()>;
    /// `listEffectiveSettings`: `keys` resolved for this profile on this device. A key the server does not define is
    /// a `422 validation_failed`.
    async fn settings_effective(&self, origin: &str, auth: &Auth, keys: &[&str]) -> Result<EffectiveSettingCollection>;
    /// `updateSettingValue` at `scope` (this device's row for `profile_device`).
    async fn setting_set(
        &self,
        origin: &str,
        auth: &Auth,
        key: &str,
        scope: &str,
        value: &serde_json::Value,
    ) -> Result<()>;
    /// `deleteSettingValue` (`204`): the next scope in the key's resolution order applies again.
    async fn setting_delete(&self, origin: &str, auth: &Auth, key: &str, scope: &str) -> Result<()>;
    /// `getPlaybackConnection`: this device on the connection it uses now (`buffering_quality_v1`).
    async fn playback_connection(&self, origin: &str, auth: &Auth) -> Result<PlaybackConnection>;
    /// `resetPlaybackLearnedLimit` (`204`).
    async fn playback_learned_limit_reset(&self, origin: &str, auth: &Auth) -> Result<()>;
    /// `getItemMarkers`: the stored intro, recap, credits and preview segments of an item's file.
    async fn item_markers(&self, origin: &str, auth: &Auth, item_id: &str) -> Result<FileMarkers>;
    /// `setItemMarkers` (`permission_gated: marker_edit`, `non_retryable`); edits are stored as `manual`.
    async fn item_markers_set(
        &self,
        origin: &str,
        auth: &Auth,
        item_id: &str,
        update: &MarkerUpdate,
    ) -> Result<FileMarkers>;
    /// `getEbookProgress`: the profile's place in a book.
    async fn ebook_progress(&self, origin: &str, auth: &Auth, content_id: &str) -> Result<EbookProgressBody>;
    /// `saveEbookProgress`: kept only when `updated_at` is newer than the stored event; returns what is stored.
    async fn ebook_progress_save(
        &self,
        origin: &str,
        auth: &Auth,
        content_id: &str,
        save: &EbookProgressSave,
    ) -> Result<EbookProgressBody>;
    /// `readEbookFile`: the whole file, up to [`MAX_EBOOK_BYTES`].
    async fn ebook_file(&self, origin: &str, auth: &Auth, content_id: &str, file_id: &str) -> Result<EbookBytes>;
    /// `syncProgress` (`non_retryable`): titles' positions, applied in order.
    async fn sync_progress(&self, origin: &str, auth: &Auth, items: &[ProgressSyncItem]) -> Result<ProgressSyncResult>;
    /// `getWatchTogetherCapabilities` (account level).
    async fn together_capabilities(&self, origin: &str, auth: &Auth) -> Result<TogetherCapabilities>;
    /// `createWatchTogetherRoom` (`201`): `room_id` is a client UUID, so a replay returns the same room.
    async fn together_create(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        selection_mode: &str,
    ) -> Result<RoomGrant>;
    /// `joinWatchTogetherRoom` with a room code.
    async fn together_join(&self, origin: &str, auth: &Auth, code: &str) -> Result<RoomGrant>;
    /// `selectWatchTogetherRoomItem` (host, `non_retryable`): the room starts this title from the beginning.
    async fn together_select(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        selection: &RoomSelection,
    ) -> Result<RoomGrant>;
    /// `closeWatchTogetherRoom` (host, `204`).
    async fn together_close(&self, origin: &str, auth: &Auth, room_id: &str) -> Result<()>;
    /// `createWatchTogetherSocketTicket` with the room proof in `X-Room-Token`.
    async fn together_ticket(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        room_token: &Secret,
    ) -> Result<EventsSocketTicket>;
    /// `listNotifications`: the newest 100 of the profile's inbox.
    async fn notifications(&self, origin: &str, auth: &Auth) -> Result<NotificationList>;
    /// `getNotificationUnreadCount`.
    async fn notification_unread_count(&self, origin: &str, auth: &Auth) -> Result<NotificationCount>;
    /// `markNotificationRead` (`204`).
    async fn notification_read(&self, origin: &str, auth: &Auth, id: &str) -> Result<()>;
    /// `markNotificationsRead` (`204`): everything up to `through`, the `read_cutoff` of a list read.
    async fn notifications_read_all(&self, origin: &str, auth: &Auth, through: &str) -> Result<()>;
    /// `getRequestStatus`.
    async fn request_status(&self, origin: &str, auth: &Auth) -> Result<RequestFeatureStatus>;
    /// `listDiscoverSections`: the rows of titles to request, each with its first page.
    async fn request_discover(&self, origin: &str, auth: &Auth) -> Result<RequestDiscoverSectionCollection>;
    /// `searchRequestMedia`; `media_type` is `movie`, `series` or `all`.
    async fn request_search(
        &self,
        origin: &str,
        auth: &Auth,
        q: &str,
        media_type: &str,
        page: u32,
    ) -> Result<RequestMediaPage>;
    /// `listMyRequests`: the newest 50.
    async fn my_requests(&self, origin: &str, auth: &Auth) -> Result<TitleRequestCollection>;
    /// `createRequest`.
    async fn create_request(&self, origin: &str, auth: &Auth, body: &TitleRequestCreate) -> Result<TitleRequest>;
    /// `cancelRequest`.
    async fn cancel_request(&self, origin: &str, auth: &Auth, id: &str) -> Result<TitleRequest>;
    /// `getDiscover`: the profile's recommendation rows with their cards.
    async fn discover(&self, origin: &str, auth: &Auth) -> Result<RecommendationRowCollection>;
    /// `getRecommendationSection`: one row in full; `cluster` and `genre` need a `key`.
    async fn recommendation_section(
        &self,
        origin: &str,
        auth: &Auth,
        kind: &str,
        key: Option<&str>,
    ) -> Result<RecommendationRow>;
    /// `getTasteProfile`.
    async fn taste_profile(&self, origin: &str, auth: &Auth) -> Result<TasteProfile>;
    /// `getCalendar`: airings and releases in a window of local days.
    async fn calendar(&self, origin: &str, auth: &Auth, query: &CalendarQuery) -> Result<Calendar>;
}

/// A per-profile flag on a catalog item, each a pair of idempotent `204` operations (docs/research/silo-api.md §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UserFlag {
    Favorite,
    Watchlist,
    Watched,
    NotInterested,
}

impl UserFlag {
    /// Path prefix and the method that sets the flag (`DELETE` clears every one).
    pub fn route(self) -> (&'static str, Method) {
        match self {
            Self::Favorite => ("/api/v2/favorites", Method::PUT),
            Self::Watchlist => ("/api/v2/watchlist", Method::PUT),
            Self::Watched => ("/api/v2/watched", Method::POST),
            Self::NotInterested => ("/api/v2/not-interested", Method::PUT),
        }
    }
}

/// Cursor paging for list operations (`limit`, `cursor`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct PageQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

/// `reqwest` + rustls with the platform verifier (Windows certificate store, ADR-0004 L3).
pub struct HttpSiloApi {
    http: reqwest::Client,
    /// Media client: no total timeout (a film streams for hours), but a read timeout so a stalled connection fails
    /// and the player reconnects.
    media: reqwest::Client,
    device: DeviceIdentity,
}

impl HttpSiloApi {
    pub fn new(device: DeviceIdentity) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(format!("Granary/{} (Windows)", device.client_version))
            // Never follow redirects: authenticated /api/v2 routes do not redirect (api-contract.md@6791aac
            // "Discovery and capability detection"), and following one could carry Bearer/profile headers to another
            // origin. A redirect is reported to the user instead (ADR-0004 M4).
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(6))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        let media = reqwest::Client::builder()
            .user_agent(format!("Granary/{} (Windows)", device.client_version))
            .redirect(reqwest::redirect::Policy::none())
            // One TCP connection per media request (as mpv and other players do): over HTTP/2 the video stream, seek
            // previews and subtitle downloads would share one connection and its flow-control window, so a burst of
            // preview reads could stall a large stream.
            .http1_only()
            .pool_max_idle_per_host(4)
            .connect_timeout(Duration::from_secs(6))
            .read_timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| ApiError::Internal(e.to_string()))?;
        Ok(Self { http, media, device })
    }

    fn device_headers(&self, rb: RequestBuilder) -> RequestBuilder {
        let d = &self.device;
        rb.header("X-Silo-Device-Id", &d.id)
            .header("X-Silo-Device-Name", &d.name)
            .header("X-Silo-Device-Platform", DEVICE_PLATFORM)
            .header("X-Silo-Client", CLIENT_NAME)
            .header("X-Silo-Client-Version", &d.client_version)
            .header("X-Silo-Client-Family", CLIENT_FAMILY)
    }

    fn with_auth(rb: RequestBuilder, auth: &Auth) -> RequestBuilder {
        let mut rb = rb.bearer_auth(auth.access.expose());
        if let Some(p) = &auth.profile {
            rb = rb.header("X-Profile-Id", &p.id);
            if let Some(t) = &p.token {
                rb = rb.header("X-Profile-Token", t.expose());
            }
        }
        rb
    }

    fn request(&self, method: Method, origin: &str, path: &str) -> RequestBuilder {
        self.device_headers(
            self.http
                .request(method, format!("{origin}{path}"))
                .header("Accept", "application/json, application/problem+json"),
        )
    }

    fn authed(&self, method: Method, origin: &str, path: &str, auth: &Auth) -> RequestBuilder {
        Self::with_auth(self.request(method, origin, path), auth)
    }
}

async fn send_raw(rb: RequestBuilder) -> Result<reqwest::Response> {
    let resp = rb
        .send()
        .await
        .map_err(|e| ApiError::from_transport(&e.without_url()))?;
    let status = resp.status();
    if status.is_redirection() {
        let location = resp
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        return Err(ApiError::Redirected { location });
    }
    if status.is_success() {
        return Ok(resp);
    }
    Err(error_from_response(resp).await)
}

async fn send_json<T: DeserializeOwned>(rb: RequestBuilder) -> Result<T> {
    let resp = send_raw(rb).await?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::from_transport(&e.without_url()))?;
    serde_json::from_slice(&bytes).map_err(|e| ApiError::Decode(e.to_string()))
}

async fn error_from_response(resp: reqwest::Response) -> ApiError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let problem = resp
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<Problem>(&b).ok())
        .map(Box::new);
    ApiError::Http {
        status,
        problem,
        retry_after,
    }
}

#[async_trait]
impl SiloApi for HttpSiloApi {
    async fn system_info(&self, origin: &str) -> Result<SystemInfo> {
        send_json(self.request(Method::GET, origin, "/api/v2/system/info")).await
    }

    async fn server_identity(&self, origin: &str) -> Result<ServerIdentity> {
        send_json(self.request(Method::GET, origin, "/api/v2/system/identity")).await
    }

    async fn login(&self, origin: &str, username: &str, password: &Secret) -> Result<TokenPair> {
        let rb = self
            .request(Method::POST, origin, "/api/v2/auth/login")
            .json(&LoginRequest { username, password });
        match send_json(rb).await {
            Err(ApiError::Http { status, .. }) if status == StatusCode::UNAUTHORIZED.as_u16() => {
                Err(ApiError::InvalidCredentials)
            }
            other => other,
        }
    }

    async fn refresh(&self, origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens> {
        let rb = self
            .request(Method::POST, origin, "/api/v2/auth/refresh")
            .json(&RefreshRequest { refresh_token });
        send_json(rb).await
    }

    async fn logout(&self, origin: &str, auth: &Auth) -> Result<()> {
        send_raw(self.authed(Method::POST, origin, "/api/v2/auth/logout", auth))
            .await
            .map(drop)
    }

    async fn current_user(&self, origin: &str, auth: &Auth) -> Result<Account> {
        send_json(self.authed(Method::GET, origin, "/api/v2/account/me", auth)).await
    }

    async fn list_profiles(&self, origin: &str, auth: &Auth) -> Result<ProfileCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/profiles", auth)).await
    }

    async fn verify_profile_pin(
        &self,
        origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification> {
        let path = format!("/api/v2/profiles/{}/verify-pin", url_segment(profile_id));
        send_json(self.authed(Method::POST, origin, &path, auth).json(&PinRequest { pin })).await
    }

    async fn list_user_libraries(&self, origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>> {
        let c: UserLibraryCollection =
            send_json(self.authed(Method::GET, origin, "/api/v2/user/libraries", auth)).await?;
        Ok(c.items)
    }

    async fn home_sections(&self, origin: &str, auth: &Auth) -> Result<SectionCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/home/sections", auth)).await
    }

    async fn catalog_browse(&self, origin: &str, auth: &Auth, query: &CatalogQuery) -> Result<CatalogBrowseCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/catalog", auth).query(query)).await
    }

    async fn catalog_item(&self, origin: &str, auth: &Auth, id: &str) -> Result<CatalogItemDetail> {
        let path = format!("/api/v2/catalog/items/{}", url_segment(id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn series_seasons(&self, origin: &str, auth: &Auth, series_id: &str) -> Result<SeasonCollection> {
        let path = format!("/api/v2/catalog/series/{}/seasons", url_segment(series_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn season_episodes(
        &self,
        origin: &str,
        auth: &Auth,
        series_id: &str,
        season: i64,
    ) -> Result<EpisodeCollection> {
        let path = format!(
            "/api/v2/catalog/series/{}/seasons/{season}/episodes",
            url_segment(series_id)
        );
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn watch_state(&self, origin: &str, auth: &Auth, id: &str) -> Result<WatchDetail> {
        let path = format!("/api/v2/watch/{}", url_segment(id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn fetch_image(&self, url: &str) -> Result<ImageData> {
        // Deliberately not `self.request`: no device identity, no bearer, no profile headers.
        let resp = send_raw(self.http.get(url).header("Accept", "image/*")).await?;
        let header = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default();
        let Some(content_type) = raster_image_type(header) else {
            return Err(ApiError::BadArtworkUrl(format!("not a raster image ({header})")));
        };
        let too_large = || ApiError::BadArtworkUrl("image too large".into());
        if resp.content_length().is_some_and(|n| n > MAX_IMAGE_BYTES as u64) {
            return Err(too_large());
        }
        // Read chunk by chunk so a body without Content-Length (chunked, HTTP/2) is cut off at the cap instead of
        // being buffered whole (phase-5 review M4).
        let mut resp = resp;
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| ApiError::from_transport(&e.without_url()))?
        {
            if bytes.len() + chunk.len() > MAX_IMAGE_BYTES {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(ImageData {
            content_type: content_type.to_owned(),
            bytes,
        })
    }

    async fn playback_capabilities(&self, origin: &str, auth: &Auth) -> Result<PlaybackCapabilities> {
        send_json(self.authed(Method::GET, origin, "/api/v2/playback/capabilities", auth)).await
    }

    async fn start_playback(&self, origin: &str, auth: &Auth, body: &PlaybackStartBody) -> Result<PlaybackDecision> {
        send_json(
            self.authed(Method::POST, origin, "/api/v2/playback/start", auth)
                .json(body),
        )
        .await
    }

    async fn playback_progress(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackProgressBody,
    ) -> Result<PlaybackMutation> {
        let path = format!("/api/v2/playback/{}/progress", url_segment(session_id));
        send_json(self.authed(Method::POST, origin, &path, auth).json(body)).await
    }

    async fn stop_playback(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackStopBody,
    ) -> Result<PlaybackMutation> {
        let path = format!("/api/v2/playback/{}", url_segment(session_id));
        send_json(self.authed(Method::DELETE, origin, &path, auth).json(body)).await
    }

    async fn replan_playback(
        &self,
        origin: &str,
        auth: &Auth,
        session_id: &str,
        body: &PlaybackReplanBody,
    ) -> Result<PlaybackDecision> {
        let path = format!("/api/v2/playback/{}/replan", url_segment(session_id));
        send_json(self.authed(Method::POST, origin, &path, auth).json(body)).await
    }

    async fn watch_trickplay(&self, origin: &str, auth: &Auth, id: &str, file_id: &str) -> Result<WatchTrickplay> {
        let path = format!("/api/v2/watch/{}/trickplay", url_segment(id));
        send_json(
            self.authed(Method::GET, origin, &path, auth)
                .query(&[("file_id", file_id)]),
        )
        .await
    }

    async fn events_ticket(&self, origin: &str, auth: &Auth) -> Result<EventsSocketTicket> {
        send_json(self.authed(Method::POST, origin, "/api/v2/events/ws-ticket", auth)).await
    }

    async fn sync_progress(&self, origin: &str, auth: &Auth, items: &[ProgressSyncItem]) -> Result<ProgressSyncResult> {
        let body = serde_json::json!({ "items": items });
        send_json(
            self.authed(Method::POST, origin, "/api/v2/sync/progress", auth)
                .json(&body),
        )
        .await
    }

    async fn ebook_progress(&self, origin: &str, auth: &Auth, content_id: &str) -> Result<EbookProgressBody> {
        let path = format!("/api/v2/ebooks/{}/progress", url_segment(content_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn ebook_progress_save(
        &self,
        origin: &str,
        auth: &Auth,
        content_id: &str,
        save: &EbookProgressSave,
    ) -> Result<EbookProgressBody> {
        let path = format!("/api/v2/ebooks/{}/progress", url_segment(content_id));
        send_json(self.authed(Method::PUT, origin, &path, auth).json(save)).await
    }

    async fn ebook_file(&self, origin: &str, auth: &Auth, content_id: &str, file_id: &str) -> Result<EbookBytes> {
        let path = format!(
            "/api/v2/ebooks/{}/files/{}/read",
            url_segment(content_id),
            url_segment(file_id)
        );
        let mut resp = send_raw(self.authed(Method::GET, origin, &path, auth)).await?;
        let too_large = || ApiError::Internal("ebook too large to open".into());
        if resp.content_length().is_some_and(|n| n > MAX_EBOOK_BYTES as u64) {
            return Err(too_large());
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("application/octet-stream")
            .to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| ApiError::from_transport(&e.without_url()))?
        {
            if bytes.len() + chunk.len() > MAX_EBOOK_BYTES {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(EbookBytes { content_type, bytes })
    }

    async fn together_capabilities(&self, origin: &str, auth: &Auth) -> Result<TogetherCapabilities> {
        send_json(self.authed(Method::GET, origin, "/api/v2/watch-together/capabilities", auth)).await
    }

    async fn together_create(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        selection_mode: &str,
    ) -> Result<RoomGrant> {
        let body = serde_json::json!({ "room_id": room_id, "selection_mode": selection_mode });
        send_json(
            self.authed(Method::POST, origin, "/api/v2/watch-together/rooms", auth)
                .json(&body),
        )
        .await
    }

    async fn together_join(&self, origin: &str, auth: &Auth, code: &str) -> Result<RoomGrant> {
        let body = serde_json::json!({ "code": code });
        send_json(
            self.authed(Method::POST, origin, "/api/v2/watch-together/join", auth)
                .json(&body),
        )
        .await
    }

    async fn together_select(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        selection: &RoomSelection,
    ) -> Result<RoomGrant> {
        let path = format!("/api/v2/watch-together/rooms/{}/selection", url_segment(room_id));
        send_json(self.authed(Method::PUT, origin, &path, auth).json(selection)).await
    }

    async fn together_close(&self, origin: &str, auth: &Auth, room_id: &str) -> Result<()> {
        let path = format!("/api/v2/watch-together/rooms/{}", url_segment(room_id));
        send_raw(self.authed(Method::DELETE, origin, &path, auth))
            .await
            .map(drop)
    }

    async fn together_ticket(
        &self,
        origin: &str,
        auth: &Auth,
        room_id: &str,
        room_token: &Secret,
    ) -> Result<EventsSocketTicket> {
        let path = format!("/api/v2/watch-together/rooms/{}/ws-ticket", url_segment(room_id));
        send_json(
            self.authed(Method::POST, origin, &path, auth)
                .header("X-Room-Token", room_token.expose()),
        )
        .await
    }

    async fn set_flag(&self, origin: &str, auth: &Auth, flag: UserFlag, id: &str, on: bool) -> Result<()> {
        let (prefix, set) = flag.route();
        let method = if on { set } else { Method::DELETE };
        let path = format!("{prefix}/{}", url_segment(id));
        send_raw(self.authed(method, origin, &path, auth)).await.map(drop)
    }

    async fn dismiss_home(&self, origin: &str, auth: &Auth, d: &HomeDismissal) -> Result<()> {
        let path = format!(
            "/api/v2/home/dismissals/{}/{}",
            url_segment(d.surface.as_str()),
            url_segment(&d.item_id)
        );
        let mut body = serde_json::Map::new();
        if let Some(s) = &d.series_id {
            body.insert("series_id".into(), s.clone().into());
        }
        if let Some(p) = &d.progress_updated_at {
            body.insert("progress_updated_at".into(), p.clone().into());
        }
        send_raw(self.authed(Method::PUT, origin, &path, auth).json(&body))
            .await
            .map(drop)
    }

    async fn set_rating(&self, origin: &str, auth: &Auth, id: &str, rating: Option<i64>) -> Result<()> {
        let path = format!("/api/v2/ratings/{}", url_segment(id));
        let rb = match rating {
            Some(r) => self
                .authed(Method::PUT, origin, &path, auth)
                .json(&serde_json::json!({ "rating": r.clamp(1, 5) })),
            None => self.authed(Method::DELETE, origin, &path, auth),
        };
        send_raw(rb).await.map(drop)
    }

    async fn person(&self, origin: &str, auth: &Auth, id: &str) -> Result<Person> {
        let path = format!("/api/v2/catalog/people/{}", url_segment(id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn library_sections(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<SectionCollection> {
        let path = format!("/api/v2/library/{}/sections", url_segment(library_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn library_collections(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<LibraryCollectionTab> {
        let path = format!("/api/v2/library/{}/collections", url_segment(library_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn library_collection_items(
        &self,
        origin: &str,
        auth: &Auth,
        library_id: &str,
        collection_id: &str,
        page: &PageQuery,
    ) -> Result<CatalogItemCollection> {
        let path = format!(
            "/api/v2/library/{}/collections/{}/items",
            url_segment(library_id),
            url_segment(collection_id)
        );
        send_json(self.authed(Method::GET, origin, &path, auth).query(page)).await
    }

    async fn similar(&self, origin: &str, auth: &Auth, id: &str) -> Result<CatalogItemCollection> {
        let path = format!("/api/v2/recommendations/similar/{}", url_segment(id));
        send_json(self.authed(Method::GET, origin, &path, auth).query(&[("limit", "20")])).await
    }

    async fn collections(&self, origin: &str, auth: &Auth) -> Result<PersonalCollectionCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/collections", auth)).await
    }

    async fn server_collections(&self, origin: &str, auth: &Auth) -> Result<ServerCollections> {
        send_json(self.authed(Method::GET, origin, "/api/v2/collections/server", auth)).await
    }

    async fn collection(&self, origin: &str, auth: &Auth, id: &str) -> Result<PersonalCollection> {
        let path = format!("/api/v2/collections/{}", url_segment(id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn box_set(&self, origin: &str, auth: &Auth, id: &str, library_id: &str) -> Result<BoxSet> {
        let path = format!("/api/v2/catalog/box-sets/{}", url_segment(id));
        send_json(
            self.authed(Method::GET, origin, &path, auth)
                .query(&[("library_id", library_id)]),
        )
        .await
    }

    async fn item_box_set(&self, origin: &str, auth: &Auth, item_id: &str) -> Result<CatalogItemBoxSet> {
        let path = format!("/api/v2/catalog/items/{}/box-set", url_segment(item_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn theme_playback(&self, origin: &str, auth: &Auth, owner_id: &str, theme_id: &str) -> Result<ThemeGrant> {
        let path = format!(
            "/api/v2/catalog/items/{}/themes/{}/playback",
            url_segment(owner_id),
            url_segment(theme_id)
        );
        // What the WebView decodes; anything else the server converts to AAC in MP4.
        let formats: Vec<serde_json::Value> = THEME_FORMATS
            .iter()
            .map(|(container, codec)| serde_json::json!({ "container": container, "audio_codec": codec }))
            .collect();
        let body = serde_json::json!({ "accepted_formats": formats });
        send_json(self.authed(Method::POST, origin, &path, auth).json(&body)).await
    }

    async fn fetch_audio(&self, url: &str) -> Result<Vec<u8>> {
        // As for artwork: no device identity, no bearer, no profile headers.
        let mut resp = send_raw(self.http.get(url).header("Accept", "audio/*, video/mp4")).await?;
        let too_large = || ApiError::Internal("theme audio too large".into());
        if resp.content_length().is_some_and(|n| n > MAX_AUDIO_BYTES as u64) {
            return Err(too_large());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| ApiError::from_transport(&e.without_url()))?
        {
            if bytes.len() + chunk.len() > MAX_AUDIO_BYTES {
                return Err(too_large());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn home_row_settings(&self, origin: &str, auth: &Auth) -> Result<HomeRowSettingCollection> {
        send_json(
            self.authed(Method::GET, origin, "/api/v2/profile/sections/settings", auth)
                .query(&[("scope", "home")]),
        )
        .await
    }

    async fn home_row_overrides(&self, origin: &str, auth: &Auth) -> Result<HomeRowOverrideCollection> {
        send_json(
            self.authed(Method::GET, origin, "/api/v2/profile/sections", auth)
                .query(&[("scope", "home")]),
        )
        .await
    }

    async fn home_rows_replace(&self, origin: &str, auth: &Auth, overrides: &[HomeRowOverrideWrite]) -> Result<()> {
        let body = serde_json::json!({ "overrides": overrides });
        send_raw(
            self.authed(Method::PUT, origin, "/api/v2/profile/sections", auth)
                .query(&[("scope", "home")])
                .json(&body),
        )
        .await
        .map(drop)
    }

    async fn home_rows_reset(&self, origin: &str, auth: &Auth) -> Result<()> {
        send_raw(
            self.authed(Method::DELETE, origin, "/api/v2/profile/sections", auth)
                .query(&[("scope", "home")]),
        )
        .await
        .map(drop)
    }

    async fn subtitle_provider_status(&self, origin: &str, auth: &Auth) -> Result<SubtitleFeatureStatus> {
        send_json(self.authed(Method::GET, origin, "/api/v2/subtitles/providers/status", auth)).await
    }

    async fn subtitle_ai_status(&self, origin: &str, auth: &Auth) -> Result<SubtitleFeatureStatus> {
        send_json(self.authed(Method::GET, origin, "/api/v2/subtitles/ai/status", auth)).await
    }

    async fn subtitle_search(
        &self,
        origin: &str,
        auth: &Auth,
        media_file_id: &str,
        languages: &[String],
    ) -> Result<SubtitleSearchResults> {
        let body = serde_json::json!({ "media_file_id": media_file_id, "languages": languages });
        send_json(
            self.authed(Method::POST, origin, "/api/v2/subtitles/search", auth)
                .json(&body),
        )
        .await
    }

    async fn subtitle_download(
        &self,
        origin: &str,
        auth: &Auth,
        body: &SubtitleDownload,
    ) -> Result<SubtitleDownloadResult> {
        send_json(
            self.authed(Method::POST, origin, "/api/v2/subtitles/download", auth)
                .json(body),
        )
        .await
    }

    async fn subtitle_translate(
        &self,
        origin: &str,
        auth: &Auth,
        body: &SubtitleTranslate,
    ) -> Result<SubtitleAiJobEnvelope> {
        let mut json = serde_json::to_value(body).map_err(|e| ApiError::Internal(e.to_string()))?;
        json["kind"] = serde_json::Value::from("translate");
        send_json(
            self.authed(Method::POST, origin, "/api/v2/subtitles/ai/translate", auth)
                .json(&json),
        )
        .await
    }

    async fn subtitle_ai_job(&self, origin: &str, auth: &Auth, job_id: &str) -> Result<SubtitleAiJobEnvelope> {
        let path = format!("/api/v2/subtitles/ai/jobs/{}", url_segment(job_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn item_markers(&self, origin: &str, auth: &Auth, item_id: &str) -> Result<FileMarkers> {
        let path = format!("/api/v2/markers/items/{}", url_segment(item_id));
        send_json(self.authed(Method::GET, origin, &path, auth)).await
    }

    async fn item_markers_set(
        &self,
        origin: &str,
        auth: &Auth,
        item_id: &str,
        update: &MarkerUpdate,
    ) -> Result<FileMarkers> {
        let path = format!("/api/v2/markers/items/{}", url_segment(item_id));
        send_json(self.authed(Method::PUT, origin, &path, auth).json(update)).await
    }

    async fn subtitle_appearance(&self, origin: &str, auth: &Auth) -> Result<SubtitleAppearanceSetting> {
        send_json(self.authed(
            Method::GET,
            origin,
            "/api/v2/settings/subtitle-appearance/effective",
            auth,
        ))
        .await
    }

    async fn subtitle_appearance_set(
        &self,
        origin: &str,
        auth: &Auth,
        value: &str,
    ) -> Result<SubtitleAppearanceSetting> {
        let body = serde_json::json!({ "value": value });
        send_json(
            self.authed(Method::PUT, origin, "/api/v2/settings/device/subtitle-appearance", auth)
                .json(&body),
        )
        .await
    }

    async fn subtitle_appearance_reset(&self, origin: &str, auth: &Auth) -> Result<()> {
        send_raw(self.authed(
            Method::DELETE,
            origin,
            "/api/v2/settings/device/subtitle-appearance",
            auth,
        ))
        .await
        .map(drop)
    }

    async fn settings_effective(&self, origin: &str, auth: &Auth, keys: &[&str]) -> Result<EffectiveSettingCollection> {
        let query: Vec<(&str, &str)> = keys.iter().map(|k| ("keys", *k)).collect();
        send_json(
            self.authed(Method::GET, origin, "/api/v2/settings/values/effective", auth)
                .query(&query),
        )
        .await
    }

    async fn setting_set(
        &self,
        origin: &str,
        auth: &Auth,
        key: &str,
        scope: &str,
        value: &serde_json::Value,
    ) -> Result<()> {
        let path = format!("/api/v2/settings/values/{}", url_segment(key));
        let body = serde_json::json!({ "value": value });
        send_raw(
            self.authed(Method::PUT, origin, &path, auth)
                .query(&[("scope", scope)])
                .json(&body),
        )
        .await
        .map(drop)
    }

    async fn setting_delete(&self, origin: &str, auth: &Auth, key: &str, scope: &str) -> Result<()> {
        let path = format!("/api/v2/settings/values/{}", url_segment(key));
        send_raw(
            self.authed(Method::DELETE, origin, &path, auth)
                .query(&[("scope", scope)]),
        )
        .await
        .map(drop)
    }

    async fn playback_connection(&self, origin: &str, auth: &Auth) -> Result<PlaybackConnection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/playback/connection", auth)).await
    }

    async fn playback_learned_limit_reset(&self, origin: &str, auth: &Auth) -> Result<()> {
        send_raw(self.authed(
            Method::DELETE,
            origin,
            "/api/v2/playback/connection/learned-limit",
            auth,
        ))
        .await
        .map(drop)
    }

    async fn notifications(&self, origin: &str, auth: &Auth) -> Result<NotificationList> {
        send_json(
            self.authed(Method::GET, origin, "/api/v2/notifications", auth)
                .query(&[("limit", "100")]),
        )
        .await
    }

    async fn notification_unread_count(&self, origin: &str, auth: &Auth) -> Result<NotificationCount> {
        send_json(self.authed(Method::GET, origin, "/api/v2/notifications/unread-count", auth)).await
    }

    async fn notification_read(&self, origin: &str, auth: &Auth, id: &str) -> Result<()> {
        let path = format!("/api/v2/notifications/{}/read", url_segment(id));
        send_raw(self.authed(Method::POST, origin, &path, auth)).await.map(drop)
    }

    async fn notifications_read_all(&self, origin: &str, auth: &Auth, through: &str) -> Result<()> {
        let body = serde_json::json!({ "through": through });
        send_raw(
            self.authed(Method::POST, origin, "/api/v2/notifications/read-all", auth)
                .json(&body),
        )
        .await
        .map(drop)
    }

    async fn request_status(&self, origin: &str, auth: &Auth) -> Result<RequestFeatureStatus> {
        send_json(self.authed(Method::GET, origin, "/api/v2/requests/status", auth)).await
    }

    async fn request_discover(&self, origin: &str, auth: &Auth) -> Result<RequestDiscoverSectionCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/requests/discover", auth)).await
    }

    async fn request_search(
        &self,
        origin: &str,
        auth: &Auth,
        q: &str,
        media_type: &str,
        page: u32,
    ) -> Result<RequestMediaPage> {
        let page = page.to_string();
        send_json(
            self.authed(Method::GET, origin, "/api/v2/requests/search", auth)
                .query(&[("q", q), ("media_type", media_type), ("page", page.as_str())]),
        )
        .await
    }

    async fn my_requests(&self, origin: &str, auth: &Auth) -> Result<TitleRequestCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/requests/mine", auth)).await
    }

    async fn create_request(&self, origin: &str, auth: &Auth, body: &TitleRequestCreate) -> Result<TitleRequest> {
        send_json(self.authed(Method::POST, origin, "/api/v2/requests", auth).json(body)).await
    }

    async fn cancel_request(&self, origin: &str, auth: &Auth, id: &str) -> Result<TitleRequest> {
        let path = format!("/api/v2/requests/{}/cancel", url_segment(id));
        send_json(self.authed(Method::POST, origin, &path, auth)).await
    }

    async fn discover(&self, origin: &str, auth: &Auth) -> Result<RecommendationRowCollection> {
        send_json(self.authed(Method::GET, origin, "/api/v2/recommendations/discover", auth)).await
    }

    async fn recommendation_section(
        &self,
        origin: &str,
        auth: &Auth,
        kind: &str,
        key: Option<&str>,
    ) -> Result<RecommendationRow> {
        let path = format!("/api/v2/recommendations/section/{}", url_segment(kind));
        // No `limit`: the default is the maximum (60).
        let params: Vec<(&str, &str)> = key.map(|k| ("key", k)).into_iter().collect();
        send_json(self.authed(Method::GET, origin, &path, auth).query(&params)).await
    }

    async fn taste_profile(&self, origin: &str, auth: &Auth) -> Result<TasteProfile> {
        send_json(self.authed(Method::GET, origin, "/api/v2/recommendations/taste-profile", auth)).await
    }

    async fn calendar(&self, origin: &str, auth: &Auth, query: &CalendarQuery) -> Result<Calendar> {
        let mut params = vec![("start", query.start.as_str()), ("end", query.end.as_str())];
        for (name, value) in [
            ("filter", &query.filter),
            ("timezone", &query.timezone),
            ("library_id", &query.library_id),
        ] {
            if let Some(v) = value {
                params.push((name, v.as_str()));
            }
        }
        send_json(
            self.authed(Method::GET, origin, "/api/v2/calendar", auth)
                .query(&params),
        )
        .await
    }

    async fn catalog_filters(&self, origin: &str, auth: &Auth, library_id: &str) -> Result<CatalogFilters> {
        send_json(
            self.authed(Method::GET, origin, "/api/v2/catalog/filters", auth)
                .query(&[("library_id", library_id), ("skip_technical", "true")]),
        )
        .await
    }

    async fn fetch_media(&self, request: MediaRequest) -> Result<MediaResponse> {
        use futures_util::TryStreamExt;
        let method = if request.head { Method::HEAD } else { Method::GET };
        let mut rb = self.media.request(method, &request.url);
        if let Some(auth) = &request.auth {
            rb = Self::with_auth(self.device_headers(rb), auth);
        }
        for (k, v) in &request.headers {
            if FORWARDED_REQUEST_HEADERS.contains(&k.as_str()) {
                rb = rb.header(k.as_str(), v.as_str());
            }
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| ApiError::from_transport(&e.without_url()))?;
        let status = resp.status();
        if status.is_redirection() && status != StatusCode::NOT_MODIFIED {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            return Err(ApiError::Redirected { location });
        }
        let passthrough =
            status.is_success() || status == StatusCode::NOT_MODIFIED || status == StatusCode::RANGE_NOT_SATISFIABLE;
        if !passthrough {
            return Err(error_from_response(resp).await);
        }
        let headers = resp
            .headers()
            .iter()
            .filter(|(k, _)| FORWARDED_RESPONSE_HEADERS.contains(&k.as_str()))
            .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned())))
            .collect();
        let body = resp.bytes_stream().map_err(|e| std::io::Error::other(e.without_url()));
        Ok(MediaResponse {
            status: status.as_u16(),
            headers,
            body: Box::pin(body),
        })
    }
}

/// Resolve an artwork reference from a server response to the URL Granary may fetch: a relative reference is
/// resolved against the server origin; an absolute one must be `https` or share the server's origin. Anything else
/// (other schemes, credentials in the URL, plain `http` to a foreign host, a foreign host that is `localhost` or a
/// loopback, private, link-local or otherwise local IP literal) is refused, so server data cannot point the artwork
/// fetch at this PC or the LAN (phase-5 review L12). Redirects are never followed (see `Api::new`).
pub fn resolve_artwork_url(origin: &str, reference: &str) -> Result<String> {
    let bad = || ApiError::BadArtworkUrl(String::from("unsupported reference"));
    let base = url::Url::parse(&format!("{origin}/")).map_err(|_| bad())?;
    let resolved = base.join(reference.trim()).map_err(|_| bad())?;
    if !resolved.username().is_empty() || resolved.password().is_some() {
        return Err(bad());
    }
    let same_origin = resolved.origin() == base.origin();
    match resolved.scheme() {
        "https" if same_origin || !is_local_host(&resolved) => Ok(resolved.into()),
        "http" if same_origin => Ok(resolved.into()),
        _ => Err(bad()),
    }
}

/// `localhost` names and IP literals that do not reach the public internet.
fn is_local_host(url: &url::Url) -> bool {
    use std::net::Ipv4Addr;
    fn local_v4(ip: Ipv4Addr) -> bool {
        let [a, b, ..] = ip.octets();
        ip.is_loopback()
            || ip.is_private()
            || ip.is_link_local()
            || ip.is_unspecified()
            || ip.is_broadcast()
            || (a == 100 && (64..128).contains(&b)) // shared address space (CGNAT, RFC 6598)
    }
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost")
        }
        Some(url::Host::Ipv4(ip)) => local_v4(ip),
        Some(url::Host::Ipv6(ip)) => {
            let seg0 = ip.segments()[0];
            ip.is_loopback()
                || ip.is_unspecified()
                || (seg0 & 0xfe00) == 0xfc00 // unique local
                || (seg0 & 0xffc0) == 0xfe80 // link-local
                || ip.to_ipv4_mapped().is_some_and(local_v4)
        }
        None => true,
    }
}

/// Percent-encode an opaque identifier for use as one path segment.
pub(crate) fn url_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_urls_are_resolved_and_restricted() {
        let o = "http://silo.lan:8096";
        assert_eq!(
            resolve_artwork_url(o, "/api/v2/artwork/k1?exp=1&sig=a").ok().as_deref(),
            Some("http://silo.lan:8096/api/v2/artwork/k1?exp=1&sig=a")
        );
        assert!(resolve_artwork_url(o, "https://image.example/p.jpg").is_ok());
        assert!(
            resolve_artwork_url(o, "http://other.example/p.jpg").is_err(),
            "foreign plain http"
        );
        assert!(resolve_artwork_url(o, "file:///C:/Windows/win.ini").is_err());
        assert!(resolve_artwork_url(o, "https://u:p@image.example/p.jpg").is_err());
        assert!(resolve_artwork_url(o, "javascript:alert(1)").is_err());
    }

    #[test]
    fn foreign_artwork_hosts_must_be_public() {
        let o = "https://silo.lan";
        for local in [
            "https://localhost/p.jpg",
            "https://app.localhost./p.jpg",
            "https://127.0.0.1/p.jpg",
            "https://10.0.0.5/p.jpg",
            "https://192.168.1.1/p.jpg",
            "https://172.16.0.1/p.jpg",
            "https://169.254.169.254/p.jpg",
            "https://100.64.0.1/p.jpg",
            "https://0.0.0.0/p.jpg",
            "https://[::1]/p.jpg",
            "https://[fd00::1]/p.jpg",
            "https://[fe80::1]/p.jpg",
            "https://[::ffff:192.168.1.1]/p.jpg",
        ] {
            assert!(resolve_artwork_url(o, local).is_err(), "{local}");
        }
        assert!(resolve_artwork_url(o, "https://8.8.8.8/p.jpg").is_ok());
        assert!(resolve_artwork_url(o, "https://[2606:4700::1]/p.jpg").is_ok());
        // The server itself may live on the LAN: its own origin is always fine.
        assert!(resolve_artwork_url("https://192.168.1.10:8443", "https://192.168.1.10:8443/a.jpg").is_ok());
        assert!(resolve_artwork_url("http://127.0.0.1:8096", "/api/v2/artwork/k").is_ok());
    }

    #[test]
    fn path_segments_are_encoded() {
        assert_eq!(url_segment("12"), "12");
        assert_eq!(url_segment("a/b?c"), "a%2Fb%3Fc");
    }
}
