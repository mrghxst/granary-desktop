//! Browse, details and play hand-off commands (Phase 5) plus the `granary-img` artwork scheme. Every Silo operation
//! used here is listed in docs/research/silo-api.md §13 with its OAS line @6791aac.
use crate::silo::{CommandError, SiloState};
use granary_silo_api::api::{ImageData, raster_image_type};
use granary_silo_api::api::{PageQuery, UserFlag};
use granary_silo_api::catalog::{
    CatalogBrowseCollection, CatalogFilters, CatalogItem, CatalogItemCollection, CatalogItemDetail, CatalogQuery,
    CollectionCard, Episode, Person, Season, Section, WatchDetail,
};
use serde::{Deserialize, Serialize};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, Manager, Runtime, State};

type CmdResult<T> = Result<T, CommandError>;

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
    /// `query` (default) or `person` (filmography).
    pub source: Option<String>,
    pub person_id: Option<String>,
    /// Alphabet jump (`#` or a letter).
    pub name_prefix: Option<String>,
    pub genre: Option<String>,
    pub year_min: Option<i64>,
    pub year_max: Option<i64>,
    /// Extra filter rules, combined with "all"; unknown fields or operators are dropped.
    pub rules: Vec<FilterRule>,
}

/// One catalog filter rule (`{field, op, value}`), as the web app builds them.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct FilterRule {
    pub field: String,
    pub op: String,
    pub value: serde_json::Value,
}

/// Rule fields and operators the UI may send (web app guided editor, see `CatalogQuery::groups`).
const RULES: &[(&str, &str)] = &[
    ("genre", "is"),
    ("content_rating", "is"),
    ("studio", "is"),
    ("network", "is"),
    ("country", "is"),
    ("original_language", "is"),
    ("watched", "is"),
    ("in_progress", "is"),
    ("resolution", "is"),
    ("hdr", "is"),
    ("dolby_vision", "is"),
    ("rating_imdb", "gte"),
    ("added_at", "in_last"),
];

fn rule_groups(rules: Vec<FilterRule>) -> Option<String> {
    let rules: Vec<FilterRule> = rules
        .into_iter()
        .filter(|r| RULES.contains(&(r.field.as_str(), r.op.as_str())))
        .filter(|r| r.value.is_string() || r.value.is_number() || r.value.is_boolean())
        .take(32)
        .collect();
    if rules.is_empty() {
        return None;
    }
    serde_json::to_string(&serde_json::json!([{ "match": "all", "rules": rules }])).ok()
}

/// The sources the UI may browse; anything else is refused before a request is sent.
const SOURCES: &[&str] = &["query", "person"];

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
            source: clean(a.source).filter(|s| SOURCES.contains(&s.as_str())),
            person_id: clean(a.person_id),
            name_prefix: clean(a.name_prefix).filter(|p| p.chars().count() == 1),
            genre: clean(a.genre),
            year_min: a.year_min.filter(|y| (1800..=3000).contains(y)),
            year_max: a.year_max.filter(|y| (1800..=3000).contains(y)),
            groups: rule_groups(a.rules),
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
    /// For the Windows media flyout.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Previous/next episode exist (media keys).
    #[serde(default)]
    pub has_previous: bool,
    #[serde(default)]
    pub has_next: bool,
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

/// Seek-bar previews (`getWatchTrickplay`); only called when the version has `trickplay_available`.
#[tauri::command]
pub async fn watch_trickplay(
    state: State<'_, SiloState>,
    server_id: String,
    item_id: String,
    file_id: String,
) -> CmdResult<granary_silo_api::playback::WatchTrickplay> {
    state
        .with_session(
            &server_id,
            |s| async move { s.watch_trickplay(&item_id, &file_id).await },
        )
        .await
}

/// Favourite / watchlist / watched / not-interested toggle on an item (all `204`).
#[tauri::command]
pub async fn catalog_set_flag(
    state: State<'_, SiloState>,
    server_id: String,
    item_id: String,
    flag: UserFlag,
    on: bool,
) -> CmdResult<()> {
    state
        .with_session(&server_id, |s| async move { s.set_flag(flag, &item_id, on).await })
        .await
}

/// Star rating 1–5, or `null` to clear it.
#[tauri::command]
pub async fn catalog_set_rating(
    state: State<'_, SiloState>,
    server_id: String,
    item_id: String,
    rating: Option<i64>,
) -> CmdResult<()> {
    if rating.is_some_and(|r| !(1..=5).contains(&r)) {
        return Err(CommandError::invalid("rating must be 1 to 5".into()));
    }
    state
        .with_session(&server_id, |s| async move { s.set_rating(&item_id, rating).await })
        .await
}

#[tauri::command]
pub async fn person_get(state: State<'_, SiloState>, server_id: String, person_id: String) -> CmdResult<Person> {
    state
        .with_session(&server_id, |s| async move { s.person(&person_id).await })
        .await
}

/// A library's Recommended tab (`listLibrarySections`).
#[tauri::command]
pub async fn library_sections(
    state: State<'_, SiloState>,
    server_id: String,
    library_id: String,
) -> CmdResult<Vec<Section>> {
    state
        .with_session(&server_id, |s| async move { s.library_sections(&library_id).await })
        .await
}

/// A library's Collections tab, every card once.
#[tauri::command]
pub async fn library_collections(
    state: State<'_, SiloState>,
    server_id: String,
    library_id: String,
) -> CmdResult<Vec<CollectionCard>> {
    state
        .with_session(&server_id, |s| async move { s.library_collections(&library_id).await })
        .await
}

#[tauri::command]
pub async fn collection_items(
    state: State<'_, SiloState>,
    server_id: String,
    library_id: String,
    collection_id: String,
    cursor: Option<String>,
) -> CmdResult<CatalogItemCollection> {
    let page = PageQuery {
        limit: Some(60),
        cursor: cursor.filter(|c| !c.is_empty()),
    };
    state
        .with_session(&server_id, |s| async move {
            s.library_collection_items(&library_id, &collection_id, &page).await
        })
        .await
}

/// "More like this" (`listSimilar`).
#[tauri::command]
pub async fn catalog_similar(
    state: State<'_, SiloState>,
    server_id: String,
    item_id: String,
) -> CmdResult<Vec<CatalogItem>> {
    state
        .with_session(&server_id, |s| async move { s.similar(&item_id).await })
        .await
}

/// Genre facet for a library's filter menu.
#[tauri::command]
pub async fn catalog_filters(
    state: State<'_, SiloState>,
    server_id: String,
    library_id: String,
) -> CmdResult<CatalogFilters> {
    state
        .with_session(&server_id, |s| async move { s.catalog_filters(&library_id).await })
        .await
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

/// Headers on every scheme response. Tauri treats `http://granary-img.localhost` as a local origin, so a response
/// must never be able to act as a document: no script, no plugins, sandboxed, not embeddable elsewhere (phase-5
/// review M3).
const SCHEME_HEADERS: &[(&str, &str)] = &[
    ("Content-Security-Policy", "default-src 'none'; sandbox"),
    ("X-Content-Type-Options", "nosniff"),
];

fn image_response(r: Result<ImageData, CommandError>) -> Response<Vec<u8>> {
    // Defence in depth: only raster types are ever served, whatever the API layer returned.
    let r = r.and_then(|img| match raster_image_type(&img.content_type) {
        Some(t) => Ok((t, img.bytes)),
        None => Err(CommandError::invalid("artwork type".into())),
    });
    let mut builder = Response::builder();
    for (k, v) in SCHEME_HEADERS {
        builder = builder.header(*k, *v);
    }
    let built = match r {
        Ok((content_type, bytes)) => builder
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(header::CACHE_CONTROL, "private, max-age=3600")
            .body(bytes),
        Err(e) => {
            tracing::debug!(code = %e.code, "artwork not served");
            builder.status(StatusCode::NOT_FOUND).body(Vec::new())
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

    #[test]
    fn filter_rules_are_allowlisted() {
        let rule = |field: &str, op: &str, value: serde_json::Value| FilterRule {
            field: field.into(),
            op: op.into(),
            value,
        };
        let q = CatalogQuery::from(CatalogQueryArgs {
            rules: vec![
                rule("studio", "is", serde_json::json!("A24")),
                rule("hdr", "is", serde_json::json!(true)),
                rule("path", "is", serde_json::json!("x")),
                rule("studio", "regex", serde_json::json!(".*")),
                rule("genre", "is", serde_json::json!({"x": 1})),
            ],
            ..CatalogQueryArgs::default()
        });
        assert_eq!(
            q.groups.as_deref(),
            Some(
                r#"[{"match":"all","rules":[{"field":"studio","op":"is","value":"A24"},{"field":"hdr","op":"is","value":true}]}]"#
            )
        );
        assert_eq!(CatalogQuery::from(CatalogQueryArgs::default()).groups, None);
    }

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
            title: Some(detail.title.clone()),
            subtitle: None,
            has_previous: false,
            has_next: false,
        };
        request.validate()?;
        let poster = detail.poster_url.unwrap_or_default();
        let response = image_response(s.artwork(&id, &poster).await);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'none'; sandbox"
        );
        let _ = std::fs::remove_dir_all(&dir);
        Ok(())
    }

    #[test]
    fn scheme_never_serves_svg_and_always_sandboxes() {
        let svg = image_response(Ok(ImageData {
            content_type: "image/svg+xml".into(),
            bytes: b"<svg/>".to_vec(),
        }));
        assert_eq!(svg.status(), StatusCode::NOT_FOUND);
        assert!(svg.body().is_empty());
        for r in [&svg, &image_response(Err(CommandError::invalid("x".into())))] {
            assert_eq!(r.headers()["content-security-policy"], "default-src 'none'; sandbox");
            assert_eq!(r.headers()["x-content-type-options"], "nosniff");
        }
        let jpeg = image_response(Ok(ImageData {
            content_type: "Image/JPEG; charset=binary".into(),
            bytes: vec![0xFF, 0xD8],
        }));
        assert_eq!(jpeg.headers()["content-type"], "image/jpeg");
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
            title: None,
            subtitle: None,
            has_previous: false,
            has_next: false,
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
