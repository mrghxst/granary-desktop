//! Browse, details and play hand-off commands (Phase 5) plus the `granary-img` artwork scheme. Every Silo operation
//! used here is listed in docs/research/silo-api.md §13 with its OAS line @6791aac.
use crate::silo::{CommandError, SiloState};
use granary_silo_api::api::ImageData;
use granary_silo_api::catalog::{
    CatalogBrowseCollection, CatalogItemDetail, CatalogQuery, Episode, Season, Section, WatchDetail,
};
use serde::{Deserialize, Serialize};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

type CmdResult<T> = Result<T, CommandError>;

/// Event emitted by the Phase 5 play stub; Phase 6 replaces the stub with the native player.
pub const EVENT_PLAY_HANDOFF: &str = "player://handoff";
/// Custom scheme serving artwork to the WebView (`http://granary-img.localhost/<serverId>?u=<reference>` on Windows).
pub const IMAGE_SCHEME: &str = "granary-img";

/// Browse query from the UI (camelCase); mapped onto the OAS query parameters.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CatalogQueryArgs {
    pub library_id: Option<String>,
    pub q: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub sort: Option<String>,
    pub cursor: Option<String>,
    pub limit: Option<i64>,
}

impl From<CatalogQueryArgs> for CatalogQuery {
    fn from(a: CatalogQueryArgs) -> Self {
        let clean = |s: Option<String>| s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty());
        Self {
            library_id: clean(a.library_id),
            q: clean(a.q),
            kind: clean(a.kind),
            sort: clean(a.sort),
            cursor: clean(a.cursor),
            limit: a.limit.map(|l| l.clamp(1, 200)),
        }
    }
}

/// What the native player needs to start an item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub server_id: String,
    pub item_id: String,
    pub file_id: String,
    pub start_seconds: f64,
    /// Position in the version's `audio_tracks`.
    pub audio_index: Option<u32>,
    /// `SubtitleTrack.index` of the chosen subtitle; `None` = off.
    pub subtitle_index: Option<i64>,
}

impl PlayRequest {
    pub(crate) fn validate(&self) -> CmdResult<()> {
        let bad = |what: &str| CommandError::invalid(format!("play request: {what}"));
        if self.server_id.is_empty() || self.item_id.is_empty() || self.file_id.is_empty() {
            return Err(bad("missing id"));
        }
        if !self.start_seconds.is_finite() || self.start_seconds < 0.0 {
            return Err(bad("start position"));
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn home_sections(state: State<'_, SiloState>, server_id: String) -> CmdResult<Vec<Section>> {
    state
        .with_session(&server_id, |s| async move { s.home_sections().await })
        .await
}

#[tauri::command]
pub async fn catalog_browse(
    state: State<'_, SiloState>,
    server_id: String,
    query: CatalogQueryArgs,
) -> CmdResult<CatalogBrowseCollection> {
    let query = CatalogQuery::from(query);
    state
        .with_session(&server_id, |s| async move { s.catalog_browse(&query).await })
        .await
}

#[tauri::command]
pub async fn catalog_item(
    state: State<'_, SiloState>,
    server_id: String,
    item_id: String,
) -> CmdResult<CatalogItemDetail> {
    state
        .with_session(&server_id, |s| async move { s.catalog_item(&item_id).await })
        .await
}

#[tauri::command]
pub async fn series_seasons(
    state: State<'_, SiloState>,
    server_id: String,
    series_id: String,
) -> CmdResult<Vec<Season>> {
    state
        .with_session(&server_id, |s| async move { s.series_seasons(&series_id).await })
        .await
}

#[tauri::command]
pub async fn season_episodes(
    state: State<'_, SiloState>,
    server_id: String,
    series_id: String,
    season: i64,
) -> CmdResult<Vec<Episode>> {
    state
        .with_session(
            &server_id,
            |s| async move { s.season_episodes(&series_id, season).await },
        )
        .await
}

#[tauri::command]
pub async fn watch_state(state: State<'_, SiloState>, server_id: String, item_id: String) -> CmdResult<WatchDetail> {
    state
        .with_session(&server_id, |s| async move { s.watch_state(&item_id).await })
        .await
}

/// Phase 5 stub: validates and logs the request and emits [`EVENT_PLAY_HANDOFF`]. Phase 6 starts libmpv here.
#[tauri::command]
pub async fn player_play<R: Runtime>(app: AppHandle<R>, request: PlayRequest) -> CmdResult<()> {
    request.validate()?;
    tracing::info!(
        server_id = %request.server_id,
        item_id = %request.item_id,
        file_id = %request.file_id,
        start = request.start_seconds,
        audio = ?request.audio_index,
        subtitle = ?request.subtitle_index,
        "play requested (native player arrives in Phase 6)"
    );
    app.emit(EVENT_PLAY_HANDOFF, &request)
        .map_err(|e| CommandError::internal(e.to_string()))
}

/// Parse `http://granary-img.localhost/<serverId>?u=<reference>` (or `granary-img://localhost/...`).
fn parse_image_uri(uri: &str) -> Option<(String, String)> {
    let url = url::Url::parse(uri).ok()?;
    let server_id = url.path_segments()?.next()?;
    let server_id = percent_decode(server_id)?;
    let reference = url.query_pairs().find(|(k, _)| k == "u")?.1.into_owned();
    (!server_id.is_empty() && !reference.is_empty()).then_some((server_id, reference))
}

fn percent_decode(s: &str) -> Option<String> {
    url::form_urlencoded::parse(format!("x={s}").as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())
}

fn image_response(r: Result<ImageData, CommandError>) -> Response<Vec<u8>> {
    let built = match r {
        Ok(img) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, img.content_type)
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .header("X-Content-Type-Options", "nosniff")
            .body(img.bytes),
        Err(e) => {
            tracing::debug!(code = %e.code, "artwork not served");
            Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new())
        }
    };
    built.unwrap_or_else(|_| Response::new(Vec::new()))
}

/// Serve one artwork request: the reference is resolved against the saved server origin and fetched without
/// credentials (ADR-0004 amendment 3).
pub async fn serve_image<R: Runtime>(app: &AppHandle<R>, uri: &str) -> Response<Vec<u8>> {
    let Some((server_id, reference)) = parse_image_uri(uri) else {
        return image_response(Err(CommandError::invalid("artwork URI".into())));
    };
    let state = app.state::<SiloState>();
    image_response(state.artwork(&server_id, &reference).await)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    /// Host-side end-to-end path (Gate 5): sign in → profile → home → library → details → watch state → a valid
    /// play request for the chosen file and tracks → artwork through the image path.
    #[tokio::test(flavor = "multi_thread")]
    async fn login_browse_details_play_handoff() -> Result<(), CommandError> {
        use granary_silo_api::Secret;
        use granary_silo_api::credentials::MemoryStore;
        use granary_silo_api::mock::MockSiloApi;
        use std::sync::Arc;
        let dir = std::env::temp_dir().join(format!("granary-e2e-{}", std::process::id()));
        let path = dir.join("servers.json");
        let _ = std::fs::remove_file(&path);
        let s = SiloState::new(Arc::new(MockSiloApi::demo()), Arc::new(MemoryStore::default()), path);
        let id = s.add("https://mock.example").await?.server_id;
        s.login(&id, "alice", Secret::new("pw")).await?;
        s.select_profile(&id, "1", None).await?;
        let home = s.with_session(&id, |x| async move { x.home_sections().await }).await?;
        assert!(
            home.iter()
                .any(|h| h.section_type == "continue_watching" && !h.items.is_empty())
        );
        let query = CatalogQuery::from(CatalogQueryArgs {
            library_id: Some("1".into()),
            ..CatalogQueryArgs::default()
        });
        let page = s
            .with_session(&id, |x| async move { x.catalog_browse(&query).await })
            .await?;
        let first = page.items.first().map(|i| i.content_id.clone()).unwrap_or_default();
        let detail = s
            .with_session(&id, |x| {
                let f = first.clone();
                async move { x.catalog_item(&f).await }
            })
            .await?;
        assert!(!detail.versions.is_empty());
        let watch = s
            .with_session(&id, |x| {
                let f = first.clone();
                async move { x.watch_state(&f).await }
            })
            .await?;
        let version = &watch.versions[0];
        let request = PlayRequest {
            server_id: id.clone(),
            item_id: watch.content_id.clone(),
            file_id: version.file_id.clone(),
            start_seconds: watch.user_data.as_ref().and_then(|u| u.position_seconds).unwrap_or(0.0),
            audio_index: Some(0),
            subtitle_index: version.subtitle_tracks.first().and_then(|t| t.index),
        };
        request.validate()?;
        let poster = detail.poster_url.unwrap_or_default();
        let response = image_response(s.artwork(&id, &poster).await);
        assert_eq!(response.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn image_uris_parse() {
        let (id, u) =
            parse_image_uri("http://granary-img.localhost/3f2a%2D9d?u=%2Fapi%2Fv2%2Fartwork%2Fk%3Fexp%3D1%26sig%3Da")
                .unwrap();
        assert_eq!((id.as_str(), u.as_str()), ("3f2a-9d", "/api/v2/artwork/k?exp=1&sig=a"));
        assert!(parse_image_uri("http://granary-img.localhost/abc").is_none());
        assert!(parse_image_uri("not a uri").is_none());
    }

    #[test]
    fn query_args_are_cleaned_and_limited() {
        let q = CatalogQuery::from(CatalogQueryArgs {
            q: Some("  ".into()),
            library_id: Some("1".into()),
            limit: Some(10_000),
            ..CatalogQueryArgs::default()
        });
        assert_eq!((q.q, q.library_id.as_deref(), q.limit), (None, Some("1"), Some(200)));
    }

    #[test]
    fn play_requests_are_validated() {
        let ok = PlayRequest {
            server_id: "s".into(),
            item_id: "m1".into(),
            file_id: "f".into(),
            start_seconds: 12.5,
            audio_index: Some(0),
            subtitle_index: None,
        };
        assert!(ok.validate().is_ok());
        let neg = PlayRequest {
            start_seconds: -1.0,
            ..ok.clone()
        };
        assert_eq!(neg.validate().unwrap_err().code, "invalid_request");
        let v: PlayRequest = serde_json::from_str(
            r#"{"serverId":"s","itemId":"i","fileId":"f","startSeconds":0,"audioIndex":null,"subtitleIndex":3}"#,
        )
        .unwrap();
        assert_eq!(v.subtitle_index, Some(3));
    }
}
