//! Browse/detail models (Phase 5). Lenient subsets of the upstream schemas: only the fields Granary renders, every
//! optional field defaulted, unknown fields ignored. Schema names and OAS lines refer to
//! `silo-server/contracts/api/v2/openapi.json@6791aac` (docs/research/silo-api.md §4, §5).

use serde::{Deserialize, Serialize};

/// `PageInfo`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PageInfo {
    pub has_more: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// `CatalogItemUserState` (OAS:L28026).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItemUserState {
    pub played: bool,
    pub is_favorite: bool,
    pub in_watchlist: bool,
    pub not_interested: bool,
    pub watched_count: Option<i64>,
    pub unplayed_count: Option<i64>,
}

/// `WatchRollup` / `WatchUserData` (OAS:L57950): resume position and watched counts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchRollup {
    pub played: bool,
    pub position_seconds: Option<f64>,
    pub duration_seconds: Option<f64>,
    pub is_in_progress: Option<bool>,
    pub last_file_id: Option<String>,
    pub last_edition_key: Option<String>,
    pub watched_count: i64,
    pub unplayed_count: i64,
    pub in_progress_count: i64,
}

/// `CatalogItem` card (OAS:L26983).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItem {
    pub content_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub year: Option<i64>,
    pub overview: Option<String>,
    pub runtime: Option<i64>,
    pub duration_seconds: Option<f64>,
    pub content_rating: Option<String>,
    pub rating_tmdb: Option<f64>,
    pub genres: Vec<String>,
    pub poster_url: Option<String>,
    pub poster_thumbhash: Option<String>,
    pub backdrop_url: Option<String>,
    pub backdrop_thumbhash: Option<String>,
    pub logo_url: Option<String>,
    pub series_id: Option<String>,
    pub series_title: Option<String>,
    pub series_poster_url: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub play_content_id: Option<String>,
    pub position_seconds: Option<f64>,
    pub item_source: Option<String>,
    /// Continue-watching cards: when the progress last changed; anchors a dismissal (`HomeDismissal`,
    /// `internal/apiv2/home.go@6791aac` L72-75).
    pub progress_updated_at: Option<String>,
    /// Audiobook and ebook cards: up to three author names in credit order.
    pub authors: Vec<String>,
    pub user_state: Option<CatalogItemUserState>,
    /// Technical badges of the best file (poster tags).
    pub overlay_summary: Option<CatalogItemOverlay>,
}

/// `CatalogItemOverlay`: technical badges of an item's best file (all display strings, free-form upstream).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItemOverlay {
    pub resolution: Option<String>,
    pub hdr: Option<String>,
    pub audio: Option<String>,
    pub audio_channels: Option<String>,
    pub video_codec: Option<String>,
    pub edition: Option<String>,
    pub release_type: Option<String>,
    pub multi_audio: Option<bool>,
    pub multi_sub: Option<bool>,
}

/// `Section` (home and library rows).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub section_type: String,
    pub featured: bool,
    pub items: Vec<CatalogItem>,
    pub has_more: Option<bool>,
    pub total_count: i64,
}

/// `SectionCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SectionCollection {
    pub sections: Vec<Section>,
}

/// `CatalogBrowseCollection` (`listCatalogItems`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogBrowseCollection {
    pub items: Vec<CatalogItem>,
    pub page: Option<PageInfo>,
    pub total: i64,
    pub total_exact: bool,
}

/// `CastCredit`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CastCredit {
    pub name: String,
    pub character: String,
    pub order: i64,
    pub person_id: Option<String>,
    pub photo_url: Option<String>,
}

/// `CrewCredit`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrewCredit {
    pub name: String,
    pub job: String,
    pub person_id: Option<String>,
    pub photo_url: Option<String>,
}

/// `ItemVideoInfo`: a remote trailer or clip (`site` is the provider, e.g. YouTube; `site_key` its video id).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemVideo {
    pub kind: String,
    pub name: Option<String>,
    pub site: String,
    pub site_key: String,
    pub is_official: bool,
    pub language: Option<String>,
}

/// `ItemExtraInfo`: a local extra (featurette, behind the scenes, …) that plays like any item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemExtra {
    pub content_id: String,
    pub kind: String,
    pub title: Option<String>,
    pub duration_seconds: Option<i64>,
    pub still_url: Option<String>,
}

/// `Person` (`getPerson`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Person {
    pub id: String,
    pub name: String,
    pub bio: Option<String>,
    pub birth_date: Option<String>,
    pub death_date: Option<String>,
    pub birthplace: Option<String>,
    pub photo_url: Option<String>,
}

/// A collection card on a library's Collections tab: `LibraryCollectionCard`, also read from `CuratedCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollectionCard {
    pub id: String,
    pub title: String,
    pub item_count: i64,
    pub poster_url: Option<String>,
}

/// `LibraryCollectionGroup` / `LibraryCollectionUngrouped` (only the cards).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CollectionGroup {
    pub name: Option<String>,
    pub collections: Vec<CollectionCard>,
}

/// `LibraryCollectionTab` (`getLibraryCollections`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryCollectionTab {
    pub collections: Vec<CollectionCard>,
    pub groups: Vec<CollectionGroup>,
    pub ungrouped: Option<CollectionGroup>,
}

impl LibraryCollectionTab {
    /// Every card once, in display order: groups, then ungrouped, then curated collections not seen yet.
    pub fn cards(&self) -> Vec<CollectionCard> {
        let mut seen = std::collections::HashSet::new();
        self.groups
            .iter()
            .flat_map(|g| g.collections.iter())
            .chain(self.ungrouped.iter().flat_map(|g| g.collections.iter()))
            .chain(self.collections.iter())
            .filter(|c| seen.insert(c.id.clone()))
            .cloned()
            .collect()
    }
}

/// `PersonalCollection` (`listCollections`, `getCollection`): what a card and a page header show.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonalCollection {
    pub id: String,
    pub name: String,
    pub description: String,
    pub item_count: i64,
    /// Empty when the collection has neither a poster nor titles for a collage.
    pub poster_url: String,
    /// The profile that made it; another profile's collection is shared with the acting one.
    pub creator_profile_id: String,
}

/// `PersonalCollectionCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonalCollectionCollection {
    pub items: Vec<PersonalCollection>,
}

/// `ServerCollectionLibrary`: a library's server collections.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerCollectionLibrary {
    pub library_id: String,
    pub library_name: String,
    pub collections: Vec<CollectionCard>,
}

/// `ServerCollectionsOutputBody` (`listServerCollections`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerCollections {
    pub libraries: Vec<ServerCollectionLibrary>,
}

/// The Collections page: the acting profile's own collections, those other profiles of the login share with it,
/// and the server's, by library.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CollectionsOverview {
    pub mine: Vec<PersonalCollection>,
    pub shared: Vec<PersonalCollection>,
    pub server: Vec<ServerCollectionLibrary>,
}

/// `CatalogBoxSetDetail`: a box set of a library with its movies, in collection order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxSet {
    pub id: String,
    pub library_id: String,
    pub title: String,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub first_year: Option<i64>,
    pub last_year: Option<i64>,
    pub movie_count: i64,
    pub played_count: i64,
    pub items: Vec<CatalogItem>,
}

/// `CatalogItemBoxSet` (`getCatalogItemBoxSet`): the box set a movie belongs to, if any.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItemBoxSet {
    pub box_set: Option<BoxSet>,
}

/// `ThemeSong`: a theme tune of a movie or series.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSong {
    pub id: String,
}

/// `ThemeSongSet` (`CatalogItemDetail.themes`): the themes of the title a detail page belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeSongSet {
    /// The item the themes are granted for (a season or episode shares its series' themes).
    pub owner_id: String,
    pub items: Vec<ThemeSong>,
}

/// The `createThemeSongPlayback` answer: a short-lived URL of the audio.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeGrant {
    pub url: String,
    /// `original` or `converted` (AAC in MP4).
    pub delivery: Option<String>,
}

/// `NotificationItem`: one entry of the profile's notification inbox.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationItem {
    pub id: String,
    /// `episode.available`, `request.approved`, `request.declined`, `request.fulfilled`; the set is open.
    #[serde(rename = "type")]
    pub kind: String,
    pub created_at: String,
    /// `None` while unread.
    pub read_at: Option<String>,
    pub series_id: Option<String>,
    pub series_title: Option<String>,
    pub episode_id: Option<String>,
    pub episode_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub poster_url: Option<String>,
    /// Event-specific data; its keys depend on the type (`title`, `media_type`, `reason`, …).
    pub reason_flags: serde_json::Value,
}

/// `NotificationListOutputBody` (`listNotifications`): newest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationList {
    pub items: Vec<NotificationItem>,
    /// What `markNotificationsRead` takes as `through`: everything up to the newest entry of this read.
    pub read_cutoff: String,
}

/// `NotificationCountOutputBody` (`getNotificationUnreadCount`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationCount {
    pub count: i64,
}

/// `FeatureStatus` (`getRequestStatus`): whether the viewer can request titles.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestFeatureStatus {
    /// `available`, `disabled`, `not_configured` or `unsupported`.
    pub state: String,
    /// Whether this viewer may make requests.
    pub allowed: bool,
    pub requests_enabled: bool,
}

/// `RequestDownload`: how far a request's download is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestDownload {
    pub phase: String,
    pub percent: Option<i64>,
}

/// `RequestMediaState`: what the viewer can do with a title, and the active request if there is one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestMediaState {
    pub requestable: bool,
    pub requested_by_viewer: bool,
    /// Why the title is not requestable.
    pub reason: Option<String>,
    pub request_id: Option<String>,
    /// `pending`, `approved` or `processing`, when a request is active.
    pub state: Option<String>,
}

/// `RequestMediaResult`: a movie or series of the request provider (TMDB), with its state on this server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestMediaResult {
    pub tmdb_id: i64,
    /// `movie` or `series`.
    pub media_type: String,
    pub title: String,
    pub year: Option<i64>,
    pub overview: Option<String>,
    /// TMDB image path (`/abc.jpg`).
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    /// `missing` or `available` in this server's catalog.
    pub availability: String,
    pub library_content_id: Option<String>,
    pub request: RequestMediaState,
}

/// `RequestMediaPage` (`searchRequestMedia`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestMediaPage {
    pub page: i64,
    pub total_pages: i64,
    pub results: Vec<RequestMediaResult>,
}

/// `DiscoverSection`: one row of titles to request, with its first page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestDiscoverSection {
    pub key: String,
    pub title: String,
    pub results: Vec<RequestMediaResult>,
}

/// `DiscoverSectionCollection` (`listDiscoverSections`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestDiscoverSectionCollection {
    pub items: Vec<RequestDiscoverSection>,
}

/// `MediaRequest`: a request for a movie or series.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleRequest {
    pub id: String,
    pub tmdb_id: i64,
    pub media_type: String,
    pub title: String,
    pub year: Option<i64>,
    pub poster_path: Option<String>,
    /// The one state to show: `pending`, `approved`, `processing`, `partially_available`, `available`, `declined`,
    /// `cancelled` or `failed`.
    pub state: String,
    /// `active`, `declined`, `cancelled`, `failed`.
    pub outcome: String,
    pub outcome_reason: Option<String>,
    pub created_at: String,
    pub library_content_id: Option<String>,
    pub seasons: Vec<i64>,
    pub download: Option<RequestDownload>,
}

/// `TitleRequestCollection` (`listMyRequests`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TitleRequestCollection {
    pub items: Vec<TitleRequest>,
}

/// `TitleRequestCreate`: the `createRequest` body. Without `seasons` a series request covers every aired season
/// that is not complete in the library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleRequestCreate {
    pub media_type: String,
    pub tmdb_id: i64,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poster_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backdrop_path: Option<String>,
}

/// `RecommendationRow`: one row of the Discover page, or one section in full.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecommendationRow {
    /// `cluster`, `similar_users_liked`, `popular`, `recently_added`, `top_rated`, `genre_sampler`.
    #[serde(rename = "type")]
    pub row_type: String,
    /// Display label, as the server words it.
    pub title: String,
    /// Section kind for `getRecommendationSection`; absent when the row has no page of its own.
    pub kind: Option<String>,
    /// Section key paired with `kind` (cluster index or genre name).
    pub key: Option<String>,
    pub items: Vec<CatalogItem>,
}

/// `RecommendationRowCollection` (`getDiscover`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecommendationRowCollection {
    pub items: Vec<RecommendationRow>,
}

/// `TasteProfile`: the profile's taste summary; empty until the server has computed one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TasteProfile {
    pub top_genres: Vec<String>,
    pub favorite_directors: Vec<String>,
}

/// The `getCalendar` query: an inclusive window of the viewer's local days, at most 31 days wide.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CalendarQuery {
    /// First local day, `YYYY-MM-DD`.
    pub start: String,
    /// Last local day, `YYYY-MM-DD`.
    pub end: String,
    /// `following`, `trending`, `everything`, …; absent is the server's default (`all`).
    pub filter: Option<String>,
    /// IANA zone the days are reckoned in; absent is UTC.
    pub timezone: Option<String>,
    pub library_id: Option<String>,
}

/// `CalendarEvent`: one airing or release.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarEvent {
    pub content_id: String,
    /// `movie`, `episode` or `season_premiere`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The movie or series title.
    pub title: String,
    pub episode_title: Option<String>,
    pub series_id: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub air_date: String,
    /// Wall-clock time in the network's zone, `HH:MM`.
    pub air_time: Option<String>,
    /// The airing as an instant, when the network's time and zone are known.
    pub air_at: Option<String>,
    pub local_air_date: String,
    pub poster_url: Option<String>,
    pub watched: bool,
    /// `series_premiere`, `season_premiere`, `finale`; on a movie `digital` or `physical`.
    pub badges: Vec<String>,
}

/// `CalendarDay`: one local day with its events in airing order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarDay {
    pub date: String,
    pub items: Vec<CalendarEvent>,
}

/// `Calendar`: the window's days, ascending; days without events are absent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Calendar {
    pub events: Vec<CalendarDay>,
}

/// `CatalogItemCollection` (similar titles, collection items).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItemCollection {
    pub items: Vec<CatalogItem>,
    pub page: Option<PageInfo>,
}

/// `AudioTrack` / `WatchAudioTrack` (OAS:L25578). Identified by its position in the version's list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioTrack {
    pub codec: Option<String>,
    pub profile: Option<String>,
    pub channels: Option<i64>,
    pub layout: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub embedded_title: Option<String>,
    pub default: bool,
    pub sample_rate: Option<i64>,
    pub bit_depth: Option<i64>,
    pub bitrate: Option<i64>,
}

/// `VersionSubtitleTrack` / `WatchSubtitleTrack`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleTrack {
    pub index: Option<i64>,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub embedded_title: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
    pub default: bool,
    pub external: bool,
}

/// `FileVersion` (OAS:L35329) / `WatchFileVersion` (OAS:L55124) subset. `duration` (FileVersion, seconds) and
/// `duration_seconds` (WatchFileVersion) are both accepted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileVersion {
    pub file_id: String,
    pub container: String,
    pub codec_video: String,
    pub codec_audio: String,
    pub resolution: String,
    pub hdr: bool,
    #[serde(alias = "duration")]
    pub duration_seconds: i64,
    pub file_size: i64,
    pub edition_key: Option<String>,
    pub edition_raw: Option<String>,
    pub effective_audio_track_index: Option<i64>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    /// `WatchChapter`/`VersionChapter` (OAS:L54617).
    pub chapters: Vec<Chapter>,
    /// `WatchFileVersion.marker_segments`: every intro/credits/recap/preview occurrence in source-time order.
    pub marker_segments: Vec<MarkerOccurrence>,
    /// Seek-bar previews are published; read them with `getWatchTrickplay`.
    pub trickplay_available: bool,
}

/// `WatchChapter` / `VersionChapter`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Chapter {
    pub index: i64,
    pub title: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub source: String,
}

/// `MarkerOccurrence` (OAS:L39442): `kind` is `intro | credits | recap | preview`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkerOccurrence {
    pub kind: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `MarkerSegment`: one stored marker of a file; both bounds are absent while the segment is unset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarkerSegment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_seconds: Option<f64>,
    /// Where it came from (`manual`, a detector or provider); empty when unset.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub provider: String,
}

/// `FileMarkers` (`getItemMarkers`, `setItemMarkers`): the stored segments of an item's file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FileMarkers {
    pub file_id: String,
    pub intro: MarkerSegment,
    pub credits: MarkerSegment,
    pub recap: MarkerSegment,
    pub preview: MarkerSegment,
    pub marker_segments: Vec<MarkerOccurrence>,
}

/// A segment to store (a `MarkerUpdate` member), in source seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarkerRange {
    pub start_seconds: f64,
    pub end_seconds: f64,
}

/// `MarkerUpdate`: an absent member leaves the segment as it is, `Some(None)` (JSON `null`) clears it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MarkerUpdate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intro: Option<Option<MarkerRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credits: Option<Option<MarkerRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recap: Option<Option<MarkerRange>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<Option<MarkerRange>>,
}

impl MarkerUpdate {
    /// The member for `kind` (`intro`, `credits`, `recap`, `preview`); `None` for anything else.
    pub fn slot(&mut self, kind: &str) -> Option<&mut Option<Option<MarkerRange>>> {
        match kind {
            "intro" => Some(&mut self.intro),
            "credits" => Some(&mut self.credits),
            "recap" => Some(&mut self.recap),
            "preview" => Some(&mut self.preview),
            _ => None,
        }
    }
}

/// `AudiobookPerson`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookPerson {
    pub name: String,
    pub person_id: Option<String>,
    pub photo_url: Option<String>,
}

/// `AudiobookRelatedItem`: a book in the series, by the same author, or similar.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookRelatedItem {
    pub content_id: String,
    pub title: String,
    pub poster_url: Option<String>,
    pub duration_seconds: Option<i64>,
    pub played: bool,
    pub progress_ratio: Option<f64>,
    pub series_index: Option<i64>,
    pub year: Option<i64>,
}

/// `AudiobookRelatedContent`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookRelated {
    pub also_by_author: Vec<AudiobookRelatedItem>,
    pub similar: Vec<AudiobookRelatedItem>,
}

/// `AudiobookSeriesGroup`: the series the book belongs to, with its books in order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookSeries {
    pub name: Option<String>,
    pub entries: Vec<AudiobookRelatedItem>,
}

/// `AudiobookNarration`: the same book read by someone else.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookNarration {
    pub content_id: String,
    pub title: String,
    pub narrators: Vec<String>,
    pub year: Option<i64>,
}

/// `AudiobookDetailExtension` subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudiobookDetail {
    pub authors: Vec<AudiobookPerson>,
    pub narrators: Vec<AudiobookPerson>,
    pub total_duration_seconds: Option<i64>,
    pub publisher: Option<String>,
    pub series: Option<AudiobookSeries>,
    pub related: AudiobookRelated,
    pub other_narrations: Vec<AudiobookNarration>,
}

/// `EbookDetailExtension` subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EbookDetail {
    pub authors: Vec<AudiobookPerson>,
    pub publisher: Option<String>,
}

/// `MangaChapter`: one chapter of a manga series, itself an ebook item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MangaChapter {
    pub content_id: String,
    pub title: String,
    pub chapter_index: Option<f64>,
    pub volume: Option<String>,
    pub read: bool,
    /// Reading position, 0..1, when the chapter was opened.
    pub progress: Option<f64>,
}

/// `MangaDetailExtension`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MangaDetail {
    pub chapters: Vec<MangaChapter>,
}

/// `EbookProgress`: a profile's place in a book. `location` is a CFI or a `fraction:<n>` value.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EbookProgress {
    pub content_id: String,
    pub file_id: String,
    pub location: String,
    pub progress: f64,
    pub updated_at: String,
}

/// `EbookProgressOutputBody`: `progress` is absent when nothing is saved.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EbookProgressBody {
    pub progress: Option<EbookProgress>,
}

/// `SaveEbookProgressInputBody`: `updated_at` is the client event time, kept exactly on a retry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EbookProgressSave {
    pub file_id: String,
    pub location: String,
    pub progress: f64,
    pub updated_at: String,
}

/// An ebook file's bytes (`readEbookFile`), with the MIME type the server served.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbookBytes {
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// `ProgressSyncItem`: a position for a whole title (an audiobook across its parts), in milliseconds.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgressSyncItem {
    pub media_item_id: String,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub force_overwrite: bool,
}

/// One result of `syncProgress`: `status` is `success` or `failure` (with `failure`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProgressSyncOutcome {
    pub index: i64,
    pub media_item_id: String,
    pub status: String,
    pub failure: Option<serde_json::Value>,
}

/// `ProgressSyncBatchResult`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ProgressSyncResult {
    pub items: Vec<ProgressSyncOutcome>,
}

/// `CatalogRating`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogRating {
    pub source: String,
    pub name: String,
    pub display: String,
    pub score: f64,
}

/// `CatalogItemDetail` (`getCatalogItem`) subset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogItemDetail {
    /// Theme tunes, when the server has any for the title.
    pub themes: Option<ThemeSongSet>,
    /// Authors, narrators and length of an audiobook (`AudiobookDetailExtension`).
    pub audiobook: Option<AudiobookDetail>,
    /// Authors and publisher of an ebook (`EbookDetailExtension`).
    pub ebook: Option<EbookDetail>,
    /// The chapters of a manga series (`MangaDetailExtension`).
    pub manga: Option<MangaDetail>,
    pub content_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub original_title: Option<String>,
    pub tagline: Option<String>,
    pub overview: Option<String>,
    pub year: Option<i64>,
    pub release_date: Option<String>,
    pub runtime: Option<i64>,
    pub content_rating: Option<String>,
    pub genres: Vec<String>,
    pub studios: Vec<String>,
    pub ratings: Vec<CatalogRating>,
    pub cast: Vec<CastCredit>,
    pub crew: Vec<CrewCredit>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub logo_url: Option<String>,
    pub series_id: Option<String>,
    pub series_title: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub season_count: Option<i64>,
    pub episode_count: Option<i64>,
    pub play_content_id: Option<String>,
    pub play_season_number: Option<i64>,
    pub user_data: Option<WatchRollup>,
    pub user_state: Option<CatalogItemUserState>,
    pub versions: Vec<FileVersion>,
    pub videos: Vec<ItemVideo>,
    pub extras: Vec<ItemExtra>,
    pub user_rating: Option<i64>,
    pub overlay_summary: Option<CatalogItemOverlay>,
    /// External ids (OAS `CatalogItemDetail.tmdb_id` / `imdb_id`, strings): for an episode they are the episode's
    /// own, the show's come from the series detail. Used for third-party seek previews (ADR-0011).
    pub tmdb_id: Option<String>,
    pub imdb_id: Option<String>,
}

/// `Season`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Season {
    pub content_id: String,
    pub title: String,
    pub season_number: i64,
    pub episode_count: i64,
    pub is_specials: Option<bool>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub play_content_id: Option<String>,
    pub user_data: Option<WatchRollup>,
}

/// `SeasonCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SeasonCollection {
    pub items: Vec<Season>,
    pub page: Option<PageInfo>,
}

/// `Episode`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Episode {
    pub content_id: String,
    pub title: String,
    pub season_number: i64,
    pub episode_number: i64,
    pub runtime: i64,
    pub air_date: Option<String>,
    pub overview: Option<String>,
    pub still_url: Option<String>,
    pub user_data: Option<WatchRollup>,
}

/// `EpisodeCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EpisodeCollection {
    pub items: Vec<Episode>,
    pub page: Option<PageInfo>,
}

/// `WatchDetail` (`getWatchState`, OAS:L54986) subset: what the play hand-off needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchDetail {
    pub content_id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    /// OAS `WatchDetail.series_id` (L54986): next/previous episode in the player.
    pub series_id: Option<String>,
    pub series_title: Option<String>,
    pub backdrop_url: Option<String>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub versions: Vec<FileVersion>,
    pub user_data: Option<WatchRollup>,
    pub effective_subtitle_language: Option<String>,
    pub effective_subtitle_mode: Option<String>,
    pub effective_show_forced_subtitles: Option<bool>,
    pub effective_subtitle_track_signature: Option<WatchSubtitleSignature>,
    pub effective_version_edition_key: Option<String>,
}

/// `WatchSubtitleSignature`: the subtitle track the profile last chose on an item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatchSubtitleSignature {
    pub codec: Option<String>,
    pub forced: bool,
    pub hearing_impaired: bool,
    pub label: Option<String>,
    pub language: Option<String>,
    pub source: Option<String>,
}

/// Query for `listCatalogItems` (only the parameters Granary sets).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CatalogQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_id: Option<String>,
    /// With `source=user_collection` or `library_collection`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name_prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year_min: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year_max: Option<i64>,
    /// JSON array of rule groups (`groups`, listCatalogItems; rule shape `{field, op, value}` from
    /// `silo-server/web/src/components/collections/CollectionGuidedRulesEditor.tsx@6791aac` L259-370).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<String>,
}

/// `CatalogFilters` (`getCatalogFilters`) subset: the facets Granary offers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CatalogFilters {
    pub genres: Vec<String>,
    pub content_ratings: Vec<String>,
    pub studios: Vec<String>,
    pub networks: Vec<String>,
    pub countries: Vec<String>,
    pub original_languages: Vec<String>,
}

/// `ProfileSectionSetting` (`getProfileSectionSettings`): one home row as the profile sees it, hidden ones included.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeRowSetting {
    pub id: String,
    pub section_type: String,
    /// The title the profile sees: its own rename, or the administrator's title.
    pub title: String,
    pub default_title: String,
    pub featured: bool,
    pub item_limit: i64,
    pub hidden: bool,
    /// Built by the profile (in the web app) rather than by an administrator.
    pub is_custom: bool,
    pub customized: bool,
    pub position: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

/// `ProfileSectionSettingCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeRowSettingCollection {
    pub items: Vec<HomeRowSetting>,
}

/// `SectionOverride` (`listProfileSectionOverrides`): one saved customisation of the home page.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeRowOverride {
    pub id: String,
    /// The administrator's row this customises; empty for a row the profile built.
    pub section_id: String,
    pub position: Option<i64>,
    pub hidden: bool,
    pub removed: bool,
    pub section_type: String,
    pub title: String,
    pub featured: Option<bool>,
    pub item_limit: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    pub is_user_added: bool,
    pub user_section_type: String,
    pub user_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_config: Option<serde_json::Value>,
}

/// `SectionOverrideCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeRowOverrideCollection {
    pub items: Vec<HomeRowOverride>,
}

/// `SectionOverrideWrite`: members that are empty, false or absent are left out, since the server refuses `null`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeRowOverrideWrite {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<i64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub removed: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub featured: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_user_added: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_section_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub user_title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_config: Option<serde_json::Value>,
}

/// `EffectiveSubtitleAppearance`: the subtitle appearance that applies to the profile on this device. The values are
/// JSON documents (`playback.subtitle_appearance`); empty when unset.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleAppearanceSetting {
    pub effective_value: String,
    pub global_value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_value: Option<String>,
    pub has_device_override: bool,
}

/// `EffectiveSettingValue` subset: a setting resolved for this profile on this device (`listEffectiveSettings`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectiveSetting {
    pub key: String,
    /// The resolved value, typed by the key's `value_schema` (a string, a number or `null`).
    pub value: serde_json::Value,
    /// The scope that won: `profile_device`, `profile`, `account` or `default`.
    pub source: String,
}

/// `EffectiveSettingCollection`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectiveSettingCollection {
    pub items: Vec<EffectiveSetting>,
    pub revision: i64,
}

/// `SubtitleProviderStatus` / `SubtitleAIStatus`: whether the viewer may search providers or start AI jobs.
/// `state` is `available | disabled | not_configured | unsupported`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleFeatureStatus {
    pub state: String,
    pub allowed: bool,
    pub enabled: bool,
    /// Provider names (provider status only).
    pub providers: Vec<String>,
}

/// What the player may offer for finding subtitles on the server.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SubtitleFeatures {
    pub search: SubtitleFeatureStatus,
    pub translate: SubtitleFeatureStatus,
}

/// `SubtitleSearchResult`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleSearchResult {
    pub id: String,
    pub provider: String,
    pub language: String,
    pub release_name: String,
    pub format: String,
    pub score: f64,
    pub downloads: i64,
    pub hearing_impaired: bool,
    pub upload_date: Option<String>,
}

/// `SubtitleSearchResults`: `warnings` names providers that could not finish.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleSearchResults {
    pub results: Vec<SubtitleSearchResult>,
    pub warnings: Vec<String>,
}

/// `StoredSubtitle`: a subtitle kept on the server for one media file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoredSubtitle {
    pub id: String,
    pub media_file_id: String,
    pub provider: String,
    pub language: String,
    pub format: String,
    pub release_name: String,
    pub hearing_impaired: bool,
}

/// `SubtitleDownloadResult`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleDownloadResult {
    pub subtitle: StoredSubtitle,
}

/// `SubtitleDownloadBody`: the chosen search result, as `searchSubtitles` returned it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleDownload {
    pub media_file_id: String,
    pub provider: String,
    pub subtitle_id: String,
    pub language: String,
    pub release_name: String,
    pub score: f64,
    pub hearing_impaired: bool,
}

/// `SubtitleAICreateBody` for a translation (`kind: translate`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubtitleTranslate {
    pub media_file_id: String,
    /// The combined subtitle ordinal of the track to translate (`combined_index` in the plan's inventory).
    pub source_index: i64,
    /// Empty: the server detects it.
    pub source_language: String,
    pub target_language: String,
    pub start_position: f64,
}

/// `SubtitleAIJob`: `status` is `pending`, then running states, then `completed | failed | cancelled`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleAiJob {
    pub id: String,
    pub kind: String,
    pub status: String,
    pub progress: f64,
    pub progress_message: String,
    pub error_message: Option<String>,
    pub result_subtitle_id: Option<String>,
    pub target_language: String,
}

/// `SubtitleAIJobOutputBody` / `SubtitleAICreateOutputBody`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleAiJobEnvelope {
    pub job: SubtitleAiJob,
}

/// The home page's rows as the profile sees them and the overrides it saved, read together for editing.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeRows {
    pub rows: Vec<HomeRowSetting>,
    pub overrides: Vec<HomeRowOverride>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// `AudiobookDetailExtension` and `CatalogItem.authors` in the shapes of the OAS schemas @749db96a.
    #[test]
    fn audiobook_page_fields_decode() {
        let d: CatalogItemDetail = serde_json::from_str(
            r#"{"content_id":"b2","title":"Winter Grafts","type":"audiobook","audiobook":{
                "authors":[{"name":"Ada Fenwick","person_id":"p1","photo_url":"/a.jpg"}],
                "narrators":[{"name":"Tom Ellery"}],"total_duration_seconds":41520,"publisher":"Fenwick & Vale",
                "series":{"name":"Orchard Letters","entries":[{"content_id":"b1","title":"The Quiet Orchard",
                  "series_index":1,"played":true},{"content_id":"b2","title":"Winter Grafts","series_index":2,
                  "progress_ratio":0.42,"duration_seconds":41520}]},
                "related":{"also_by_author":[{"content_id":"b1","title":"The Quiet Orchard","year":2022}],"similar":[]},
                "other_narrations":[{"content_id":"b9","title":"Winter Grafts","narrators":["Ines Vale"],"year":2024}]}}"#,
        )
        .unwrap();
        let b = d.audiobook.unwrap();
        assert_eq!(b.publisher.as_deref(), Some("Fenwick & Vale"));
        let series = b.series.unwrap();
        assert_eq!(series.name.as_deref(), Some("Orchard Letters"));
        assert!(series.entries[0].played);
        assert_eq!(series.entries[1].progress_ratio, Some(0.42));
        assert_eq!(b.related.also_by_author[0].year, Some(2022));
        assert_eq!(b.other_narrations[0].narrators, ["Ines Vale"]);
        let card: CatalogItem =
            serde_json::from_str(r#"{"content_id":"b2","title":"W","type":"audiobook","authors":["Ada Fenwick"]}"#)
                .unwrap();
        assert_eq!(card.authors, ["Ada Fenwick"]);
    }

    #[test]
    fn lenient_card_and_aliases() {
        let item: CatalogItem = serde_json::from_str(
            r#"{"content_id":"m1","title":"A","type":"movie","status":"available","genres":["Drama"],"keywords":[],
                "unknown_future_field":{"x":1}}"#,
        )
        .unwrap();
        assert_eq!((item.kind.as_str(), item.genres.len()), ("movie", 1));
        let v: FileVersion =
            serde_json::from_str(r#"{"file_id":"f1","duration":7200,"audio_tracks":[{"default":true}]}"#).unwrap();
        assert_eq!((v.duration_seconds, v.audio_tracks.len()), (7200, 1));
        let q = serde_json::to_value(CatalogQuery {
            q: Some("x".into()),
            kind: Some("movie".into()),
            ..CatalogQuery::default()
        })
        .unwrap();
        assert_eq!(q, serde_json::json!({"q":"x","type":"movie"}));
    }
}

/// The home surface a card is hidden from (`HomeDismissalInput.surface`, `internal/apiv2/home.go@6791aac` L67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HomeSurface {
    ContinueWatching,
    NextUp,
}

impl HomeSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContinueWatching => "continue_watching",
            Self::NextUp => "next_up",
        }
    }
}

/// One `dismissHomeItem` request: the card and what the dismissal is anchored to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeDismissal {
    pub surface: HomeSurface,
    pub item_id: String,
    #[serde(default)]
    pub series_id: Option<String>,
    #[serde(default)]
    pub progress_updated_at: Option<String>,
}
