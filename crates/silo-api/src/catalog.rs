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
    pub user_state: Option<CatalogItemUserState>,
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
    pub series_title: Option<String>,
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

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
