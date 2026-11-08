//! Contract tests (Phase 4): sanitized fixtures are checked against the upstream OpenAPI subset, then served from a
//! local mock server to the real `HttpSiloApi`, whose requests (method, path, headers, body) are checked against the
//! same subset.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: a panic is the failure report

use granary_silo_api::api::{PageQuery, UserFlag};
use granary_silo_api::credentials::{CredentialStore, MemoryStore};
use granary_silo_api::device::{CLIENT_FAMILY, CLIENT_NAME, DEVICE_PLATFORM, DeviceIdentity};
use granary_silo_api::{ApiError, Auth, HttpSiloApi, ProfileAuth, Secret, Session, SiloApi, probe};
use serde_json::Value;
use std::sync::Arc;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn load(name: &str) -> Value {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}")).expect("fixture");
    serde_json::from_str(&text).expect("fixture json")
}

fn subset() -> Value {
    load("openapi-subset.json")
}

/// Minimal JSON Schema check for the subset's vocabulary: `$ref`, `type` (incl. `["x","null"]`), `required`,
/// `properties`, `additionalProperties: false`, `items`, `enum`, `minLength`.
fn validate(value: &Value, schema: &Value, schemas: &Value, at: &str, errors: &mut Vec<String>) {
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let name = r.trim_start_matches("#/components/schemas/");
        let target = &schemas[name];
        assert!(!target.is_null(), "unknown schema {name}");
        return validate(value, target, schemas, at, errors);
    }
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let ok = types.iter().any(|t| match *t {
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            "object" => value.is_object(),
            "null" => value.is_null(),
            _ => false,
        });
        if !ok {
            errors.push(format!("{at}: expected {types:?}, got {value}"));
            return;
        }
    }
    if let (Some(e), false) = (schema.get("enum").and_then(Value::as_array), value.is_null())
        && !e.contains(value)
    {
        errors.push(format!("{at}: {value} not in enum {e:?}"));
    }
    if let (Some(min), Some(s)) = (schema.get("minLength").and_then(Value::as_u64), value.as_str())
        && (s.chars().count() as u64) < min
    {
        errors.push(format!("{at}: shorter than {min}"));
    }
    if let Some(obj) = value.as_object() {
        let props = schema.get("properties").and_then(Value::as_object);
        for req in schema.get("required").and_then(Value::as_array).into_iter().flatten() {
            let req = req.as_str().unwrap_or_default();
            if !obj.contains_key(req) {
                errors.push(format!("{at}: missing required `{req}`"));
            }
        }
        for (k, v) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(s) => validate(v, s, schemas, &format!("{at}.{k}"), errors),
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    errors.push(format!("{at}: unknown property `{k}`"))
                }
                None => {}
            }
        }
    }
    if let (Some(items), Some(arr)) = (schema.get("items"), value.as_array()) {
        for (i, v) in arr.iter().enumerate() {
            validate(v, items, schemas, &format!("{at}[{i}]"), errors);
        }
    }
}

fn assert_valid(value: &Value, schema: &Value, what: &str) {
    let s = subset();
    let mut errors = Vec::new();
    validate(value, schema, &s["schemas"], what, &mut errors);
    assert!(
        errors.is_empty(),
        "{what} violates the contract:\n{}",
        errors.join("\n")
    );
}

fn op(id: &str) -> Value {
    subset()["operations"][id].clone()
}

/// (operationId, status, fixture)
const RESPONSES: &[(&str, &str, &str)] = &[
    ("getSystemInfo", "200", "system_info.json"),
    ("getServerIdentity", "200", "server_identity.json"),
    ("login", "200", "token_pair.json"),
    ("refreshSession", "200", "refreshed_tokens.json"),
    ("getCurrentUser", "200", "account.json"),
    ("listProfiles", "200", "profiles.json"),
    ("verifyProfilePIN", "200", "profile_verification.json"),
    ("listUserLibraries", "200", "user_libraries.json"),
    ("login", "401", "problem_invalid_token.json"),
    ("listUserLibraries", "403", "problem_profile_verification_required.json"),
    ("refreshSession", "503", "problem_dependency_unavailable.json"),
    // Phase 5 (generated by scripts/gen-catalog-fixtures.mjs)
    ("listHomeSections", "200", "home_sections.json"),
    ("listCatalogItems", "200", "catalog_browse.json"),
    ("getCatalogItem", "200", "catalog_item.json"),
    ("listSeriesSeasons", "200", "series_seasons.json"),
    ("listSeasonEpisodes", "200", "season_episodes.json"),
    ("getWatchState", "200", "watch_detail.json"),
    // Phase 6: upstream's own playback fixtures (contracts/api/v2/fixtures@6791aac)
    (
        "getPlaybackCapabilities",
        "200",
        "upstream_playback_capability_available.json",
    ),
    ("startPlayback", "201", "upstream_playback_start_opaque_ids.json"),
    ("startPlayback", "409", "upstream_playback_installation_changed.json"),
    (
        "updatePlaybackProgress",
        "200",
        "upstream_playback_progress_applied.json",
    ),
    ("updatePlaybackProgress", "200", "upstream_playback_progress_stale.json"),
    (
        "updatePlaybackProgress",
        "409",
        "upstream_playback_progress_conflict.json",
    ),
    ("stopPlayback", "200", "upstream_playback_stop_completed.json"),
    ("stopPlayback", "200", "upstream_playback_stop_replayed.json"),
    ("replanPlayback", "200", "upstream_playback_replan_reanchored.json"),
    ("getWatchTrickplay", "200", "upstream_get_watch_trickplay_ok.json"),
    ("createEventsSocketTicket", "200", "upstream_events_socket_ticket.json"),
    // 0.2: people, library tabs, similar titles, filters
    ("getPerson", "200", "person.json"),
    ("listLibrarySections", "200", "home_sections.json"),
    ("getLibraryCollections", "200", "library_collections.json"),
    ("getLibraryCollectionItems", "200", "catalog_item_collection.json"),
    ("listSimilar", "200", "catalog_item_collection.json"),
    ("getCatalogFilters", "200", "catalog_filters.json"),
];

/// 0.2 profile flags: (operationId, flag, on). Each answers `204` with no body.
const FLAG_OPS: &[(&str, UserFlag, bool)] = &[
    ("addFavorite", UserFlag::Favorite, true),
    ("deleteFavorite", UserFlag::Favorite, false),
    ("addToWatchlist", UserFlag::Watchlist, true),
    ("deleteWatchlistEntry", UserFlag::Watchlist, false),
    ("markWatched", UserFlag::Watched, true),
    ("unmarkWatched", UserFlag::Watched, false),
    ("markNotInterested", UserFlag::NotInterested, true),
    ("unmarkNotInterested", UserFlag::NotInterested, false),
];

/// Phase 6 operations, served by `http_client_speaks_the_playback_contract`.
const PLAYBACK_OPS: &[&str] = &[
    "getPlaybackCapabilities",
    "startPlayback",
    "updatePlaybackProgress",
    "stopPlayback",
    "replanPlayback",
    "getWatchTrickplay",
    "createEventsSocketTicket",
];

/// Phase 5 operations served to the catalog contract test.
const CATALOG_OPS: &[&str] = &[
    "listHomeSections",
    "listCatalogItems",
    "getCatalogItem",
    "listSeriesSeasons",
    "listSeasonEpisodes",
    "getWatchState",
];

#[test]
fn fixtures_match_the_upstream_schemas() {
    for (id, status, fixture) in RESPONSES {
        let schema = op(id)["responses"][status]["schema"].clone();
        assert!(!schema.is_null(), "{id} has no {status} response in the subset");
        assert_valid(&load(fixture), &schema, &format!("{fixture} ({id} {status})"));
    }
}

#[test]
fn validator_rejects_contract_violations() {
    let schema = op("getServerIdentity")["responses"]["200"]["schema"].clone();
    let s = subset();
    let mut errors = Vec::new();
    validate(
        &serde_json::json!({"server_id": 5, "extra": true}),
        &schema,
        &s["schemas"],
        "x",
        &mut errors,
    );
    assert_eq!(errors.len(), 2, "{errors:?}");
}

fn device() -> DeviceIdentity {
    DeviceIdentity::fixed("8a1e5c2b-0d3f-4e7a-9b6c-1f2e3d4c5b6a")
}

fn json(fixture: &str, status: u16) -> ResponseTemplate {
    let ct = if status >= 400 {
        "application/problem+json"
    } else {
        "application/json"
    };
    ResponseTemplate::new(status).set_body_raw(serde_json::to_vec(&load(fixture)).unwrap(), ct)
}

/// Mount the fixture for `id` at the subset's method + path (path parameters filled from `params`).
async fn mount(server: &MockServer, id: &str, status: u16, fixture: &str, params: &[(&str, &str)]) {
    let o = op(id);
    let mut p = o["path"].as_str().unwrap().to_owned();
    for (k, v) in params {
        p = p.replace(&format!("{{{k}}}"), v);
    }
    Mock::given(method(o["method"].as_str().unwrap()))
        .and(path(p))
        .and(header("X-Silo-Device-Id", device().id.as_str()))
        .and(header("X-Silo-Client", CLIENT_NAME))
        .and(header("X-Silo-Client-Family", CLIENT_FAMILY))
        .and(header("X-Silo-Device-Platform", DEVICE_PLATFORM))
        .respond_with(json(fixture, status))
        .mount(server)
        .await;
}

fn auth(profile: Option<(&str, Option<&str>)>) -> Auth {
    Auth {
        access: Secret::new("fixture-access-1"),
        profile: profile.map(|(id, t)| ProfileAuth {
            id: id.into(),
            token: t.map(Secret::new),
        }),
    }
}

#[tokio::test]
async fn http_client_speaks_the_contract() {
    let server = MockServer::start().await;
    for (id, status, fixture) in RESPONSES
        .iter()
        .filter(|r| r.1 == "200" && !CATALOG_OPS.contains(&r.0) && !PLAYBACK_OPS.contains(&r.0))
    {
        mount(&server, id, status.parse().unwrap(), fixture, &[("id", "2")]).await;
    }
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/logout"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;
    let api = HttpSiloApi::new(device()).unwrap();
    let o = server.uri();

    let info = api.system_info(&o).await.unwrap();
    assert_eq!(info.api_major, 2);
    assert_eq!(
        api.server_identity(&o).await.unwrap().server_id,
        "3f2a9d5e-6b1c-4c7e-9a0d-2f4b8c1e7a35"
    );
    let pair = api.login(&o, "alice", &Secret::new("pw")).await.unwrap();
    assert_eq!((pair.user.username.as_str(), pair.expires_in), ("alice", 28800));
    let r = api.refresh(&o, &pair.refresh_token).await.unwrap();
    assert_eq!(r.refresh_token.expose(), "fixture-refresh-2");
    assert_eq!(api.current_user(&o, &auth(None)).await.unwrap().id, "1");
    let profiles = api.list_profiles(&o, &auth(None)).await.unwrap();
    assert_eq!(profiles.items.len(), 2);
    assert!(profiles.items[1].has_pin && profiles.items[1].is_child);
    let v = api
        .verify_profile_pin(&o, &auth(None), "2", &Secret::new("1234"))
        .await
        .unwrap();
    assert!(v.valid && v.profile_token.is_some());
    let libs = api
        .list_user_libraries(&o, &auth(Some(("2", Some("pvt_fixture")))))
        .await
        .unwrap();
    assert_eq!(
        libs.iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(),
        ["movies", "series"]
    );
    api.logout(&o, &auth(None)).await.unwrap();

    // Every request: known operation, bearer only where the contract has security, bodies match request schemas.
    let s = subset();
    for req in server.received_requests().await.unwrap() {
        let (m, p) = (req.method.as_str().to_owned(), req.url.path().to_owned());
        let (id, o) = s["operations"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, o)| {
                let tpl = o["path"].as_str().unwrap().replace("{id}", "2");
                o["method"] == m && tpl == p
            })
            .unwrap_or_else(|| panic!("request {m} {p} is not a documented operation"));
        let bearer = req
            .headers
            .get("authorization")
            .map(|v| v.to_str().unwrap_or_default().to_owned());
        assert_eq!(
            bearer.is_some(),
            !o["security"].is_null(),
            "{id}: bearer only on secured operations"
        );
        if let Some(b) = bearer {
            assert_eq!(b, "Bearer fixture-access-1");
        }
        if !o["requestBody"].is_null() {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            assert_valid(&body, &o["requestBody"], &format!("{id} request body"));
        }
        if p.ends_with("/user/libraries") {
            assert_eq!(req.headers.get("x-profile-id").unwrap(), "2");
            assert_eq!(req.headers.get("x-profile-token").unwrap(), "pvt_fixture");
        }
    }
}

#[tokio::test]
async fn http_client_speaks_the_catalog_contract() {
    use granary_silo_api::catalog::CatalogQuery;
    let server = MockServer::start().await;
    for (id, status, fixture) in RESPONSES.iter().filter(|r| CATALOG_OPS.contains(&r.0)) {
        mount(
            &server,
            id,
            status.parse().unwrap(),
            fixture,
            &[("id", "m1"), ("num", "1")],
        )
        .await;
    }
    Mock::given(method("GET"))
        .and(path("/api/v2/artwork/poster-m1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(b"\x89PNG".to_vec(), "image/png"))
        .mount(&server)
        .await;
    let api = HttpSiloApi::new(device()).unwrap();
    let o = server.uri();
    let a = auth(Some(("2", Some("pvt_fixture"))));

    let sections = api.home_sections(&o, &a).await.unwrap();
    assert_eq!(sections.sections[0].items[0].content_id, "m1");
    let query = CatalogQuery {
        library_id: Some("lib-1".into()),
        q: Some("north".into()),
        kind: Some("video_with_episodes".into()),
        sort: Some("title".into()),
        cursor: Some("cursor-1".into()),
        limit: Some(60),
        ..CatalogQuery::default()
    };
    let page = api.catalog_browse(&o, &a, &query).await.unwrap();
    assert_eq!(page.page.and_then(|p| p.next_cursor).as_deref(), Some("cursor-2"));
    let detail = api.catalog_item(&o, &a, "m1").await.unwrap();
    assert_eq!(detail.versions[0].duration_seconds, 7080);
    assert_eq!(detail.versions[0].subtitle_tracks[1].index, Some(3));
    assert_eq!(api.series_seasons(&o, &a, "m1").await.unwrap().items.len(), 1);
    assert_eq!(api.season_episodes(&o, &a, "m1", 1).await.unwrap().items.len(), 3);
    let watch = api.watch_state(&o, &a, "m1").await.unwrap();
    assert_eq!(watch.versions[0].duration_seconds, 7080);
    assert_eq!(watch.effective_subtitle_mode.as_deref(), Some("forced_only"));
    let img = api
        .fetch_image(&format!("{o}/api/v2/artwork/poster-m1?exp=1&sig=a"))
        .await
        .unwrap();
    assert_eq!(img.content_type, "image/png");

    let s = subset();
    for req in server.received_requests().await.unwrap() {
        let (m, p) = (req.method.as_str().to_owned(), req.url.path().to_owned());
        let (id, o) = s["operations"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, o)| {
                let tpl = o["path"]
                    .as_str()
                    .unwrap()
                    .replace("{id}", "m1")
                    .replace("{num}", "1")
                    .replace("{key}", "poster-m1");
                o["method"] == m && tpl == p
            })
            .unwrap_or_else(|| panic!("request {m} {p} is not a documented operation"));
        let has = |h: &str| req.headers.get(h).is_some();
        if id == "getArtwork" {
            // Presigned: no credential and no device identity may travel with an image request.
            for h in ["authorization", "x-profile-id", "x-profile-token", "x-silo-device-id"] {
                assert!(!has(h), "artwork request carries {h}");
            }
            continue;
        }
        assert_eq!(req.headers.get("authorization").unwrap(), "Bearer fixture-access-1");
        assert_eq!(req.headers.get("x-profile-id").unwrap(), "2", "{id}: profile-scoped");
        assert_eq!(req.headers.get("x-profile-token").unwrap(), "pvt_fixture");
        // Every query parameter sent is documented for the operation.
        let documented: Vec<&str> = o["params"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["in"] == "query")
            .filter_map(|p| p["name"].as_str())
            .collect();
        for (k, _) in req.url.query_pairs() {
            assert!(
                documented.contains(&k.as_ref()),
                "{id}: undocumented query parameter {k}"
            );
        }
        if id == "listCatalogItems" {
            let q: Vec<(String, String)> = req.url.query_pairs().map(|(k, v)| (k.into(), v.into())).collect();
            assert!(q.contains(&("type".into(), "video_with_episodes".into())));
            assert!(q.contains(&("library_id".into(), "lib-1".into())));
        }
    }
}

/// One-shot HTTP/1.1 server on a plain thread that answers with a chunked body (no Content-Length).
fn chunked_server(content_type: &'static str, total: usize) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut sock, _)) = listener.accept() else { return };
        let mut buf = [0u8; 4096];
        let _ = sock.read(&mut buf);
        let head = format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nTransfer-Encoding: chunked\r\n\r\n");
        if sock.write_all(head.as_bytes()).is_err() {
            return;
        }
        let chunk = vec![0u8; 1024 * 1024];
        let mut sent = 0;
        while sent < total {
            let n = chunk.len().min(total - sent);
            let frame = [format!("{n:x}\r\n").as_bytes(), &chunk[..n], b"\r\n"].concat();
            // The client hangs up once it has seen enough: that is the behaviour under test.
            if sock.write_all(&frame).is_err() {
                return;
            }
            sent += n;
        }
        let _ = sock.write_all(b"0\r\n\r\n");
    });
    format!("http://{addr}/api/v2/artwork/big")
}

#[tokio::test]
async fn artwork_is_capped_without_content_length_and_must_be_raster() {
    use granary_silo_api::api::MAX_IMAGE_BYTES;
    let api = HttpSiloApi::new(device()).unwrap();
    // 64 MiB streamed without Content-Length: refused once the 16 MiB cap is crossed, not buffered whole.
    let err = api
        .fetch_image(&chunked_server("image/jpeg", 4 * MAX_IMAGE_BYTES))
        .await
        .unwrap_err();
    assert!(
        matches!(err, ApiError::BadArtworkUrl(ref m) if m.contains("too large")),
        "{err:?}"
    );
    // A small chunked body is read completely.
    let ok = api.fetch_image(&chunked_server("image/png", 3000)).await.unwrap();
    assert_eq!((ok.content_type.as_str(), ok.bytes.len()), ("image/png", 3000));
    // SVG can carry script: never accepted, whatever the transfer encoding.
    let svg = api.fetch_image(&chunked_server("image/svg+xml", 10)).await.unwrap_err();
    assert!(matches!(svg, ApiError::BadArtworkUrl(_)), "{svg:?}");
}

#[tokio::test]
async fn http_errors_are_mapped() {
    let server = MockServer::start().await;
    mount(&server, "login", 401, "problem_invalid_token.json", &[]).await;
    mount(
        &server,
        "listUserLibraries",
        403,
        "problem_profile_verification_required.json",
        &[],
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/api/v2/auth/refresh"))
        .respond_with(json("problem_dependency_unavailable.json", 503).insert_header("Retry-After", "5"))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/account/me"))
        .respond_with(
            ResponseTemplate::new(307).insert_header("Location", "https://elsewhere.example/api/v2/account/me"),
        )
        .mount(&server)
        .await;
    let api = HttpSiloApi::new(device()).unwrap();
    let o = server.uri();

    assert!(matches!(
        api.login(&o, "alice", &Secret::new("bad")).await,
        Err(ApiError::InvalidCredentials)
    ));
    let e = api.list_user_libraries(&o, &auth(Some(("2", None)))).await.unwrap_err();
    assert_eq!(e.problem_id(), Some("profile_verification_required"));
    let e = api.refresh(&o, &Secret::new("r")).await.unwrap_err();
    assert!(
        matches!(
            e,
            ApiError::Http {
                status: 503,
                retry_after: Some(5),
                ..
            }
        ),
        "{e:?}"
    );
    assert_eq!(e.code(), "unavailable");
    // Redirects are never followed, so credentials cannot reach another origin.
    let e = api.current_user(&o, &auth(None)).await.unwrap_err();
    assert!(
        matches!(&e, ApiError::Redirected { location } if location.starts_with("https://elsewhere")),
        "{e:?}"
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/api/v2/account/me")
            .count(),
        1
    );
}

#[tokio::test]
async fn probe_rejects_non_silo_servers() {
    let server = MockServer::start().await;
    let api = HttpSiloApi::new(device()).unwrap();
    // Nothing mounted: 404 for /api/v2/system/info.
    assert!(matches!(
        probe(&api, &server.uri()).await,
        Err(ApiError::NotSilo { status: 404 })
    ));
    Mock::given(method("GET"))
        .and(path("/api/v2/system/info"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html>router login</html>", "text/html"))
        .mount(&server)
        .await;
    assert!(matches!(
        probe(&api, &server.uri()).await,
        Err(ApiError::NotSilo { status: 200 })
    ));
    // Nothing listening.
    let e = probe(&api, "http://127.0.0.1:9").await.unwrap_err();
    assert!(matches!(e, ApiError::Unreachable(_) | ApiError::Timeout), "{e:?}");
}

#[tokio::test]
async fn probe_reports_a_real_v2_server() {
    let server = MockServer::start().await;
    mount(&server, "getSystemInfo", 200, "system_info.json", &[]).await;
    mount(&server, "getServerIdentity", 200, "server_identity.json", &[]).await;
    let api = HttpSiloApi::new(device()).unwrap();
    let p = probe(&api, &server.uri()).await.unwrap();
    assert!(p.insecure && !p.untested);
    assert_eq!(p.info.server_version, "1.0.0-dev");
}

#[tokio::test]
async fn session_refreshes_on_401_and_retries_once() {
    let server = MockServer::start().await;
    mount(&server, "login", 200, "token_pair.json", &[]).await;
    mount(&server, "refreshSession", 200, "refreshed_tokens.json", &[]).await;
    // The first access token is rejected, the refreshed one accepted.
    Mock::given(method("GET"))
        .and(path("/api/v2/account/me"))
        .and(header("Authorization", "Bearer fixture-access-1"))
        .respond_with(json("problem_invalid_token.json", 401))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v2/account/me"))
        .and(header("Authorization", "Bearer fixture-access-2"))
        .respond_with(json("account.json", 200))
        .mount(&server)
        .await;
    let api: Arc<dyn SiloApi> = Arc::new(HttpSiloApi::new(device()).unwrap());
    let store = Arc::new(MemoryStore::default());
    let (session, _) = Session::login(api, store.clone(), &server.uri(), "srv", "alice", &Secret::new("pw"))
        .await
        .unwrap();
    assert_eq!(session.current_user().await.unwrap().username, "alice");
    assert_eq!(
        store.get("srv/1").unwrap().unwrap().expose(),
        "fixture-refresh-2",
        "rotated token persisted"
    );
    let refreshes = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.url.path() == "/api/v2/auth/refresh")
        .count();
    assert_eq!(refreshes, 1);
}

// ---------------------------------------------------------------------------------------------------------------
// Phase 6: playback protocol v3 and the media relay

use granary_silo_api::caps::{HardwareDecoders, OutputFacts, client_capabilities, client_features, playback_context};
use granary_silo_api::media::MediaRequest;
use granary_silo_api::playback::*;

const SESSION: &str = "11111111-1111-4111-8111-111111111111";

fn schema_of(name: &str) -> Value {
    serde_json::json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn granary_start_body(caps: &PlaybackCapabilities) -> PlaybackStartBody {
    PlaybackStartBody {
        installation_id: caps.installation_id.clone().unwrap(),
        protocol_version: PROTOCOL_VERSION,
        client_features: client_features(caps),
        file_id: "42".into(),
        profile_id: "p-primary".into(),
        playback_attempt_id: mint_id(),
        quality_preference: "auto".into(),
        subtitle_fidelity_preference: "preserve".into(),
        client_capabilities: client_capabilities(&HardwareDecoders {
            codecs: vec!["h264".into(), "hevc".into()],
            ten_bit: vec!["hevc".into()],
        }),
        client_playback_context: playback_context(
            "0.1.0",
            "10.0.26200",
            &OutputFacts {
                display_id: Some(r"\\.\DISPLAY1".into()),
                hdr_active: Some(true),
                hdr_switchable: false,
            },
        ),
        start_position: Some(12.5),
        audio_track_index: Some(0),
        subtitle_track_index: None,
        allow_alternate_versions: Some(false),
    }
}

#[test]
fn upstream_playback_fixtures_decode_with_granary_types() {
    let caps: PlaybackCapabilities =
        serde_json::from_value(load("upstream_playback_capability_available.json")).unwrap();
    assert!(caps.allowed && caps.protocol_versions.contains(&PROTOCOL_VERSION));
    assert!(caps.installation_id.is_some());
    for fixture in [
        "upstream_playback_start_opaque_ids.json",
        "upstream_playback_replan_reanchored.json",
    ] {
        let d: PlaybackDecision = serde_json::from_value(load(fixture)).unwrap();
        let (sid, plan) = d.playable().unwrap_or_else(|| panic!("{fixture} not playable"));
        assert_eq!(sid, SESSION);
        assert!(plan.plan_attempt_key.starts_with("v3:"));
        assert!(!plan.stream.url.is_empty());
        assert!(plan.timeline.player_start_seconds >= 0.0);
    }
    for (fixture, outcome) in [
        ("upstream_playback_progress_applied.json", "applied"),
        ("upstream_playback_progress_stale.json", "stale_sample"),
        ("upstream_playback_stop_completed.json", "stopped"),
        ("upstream_playback_stop_replayed.json", "replayed"),
    ] {
        let m: PlaybackMutation = serde_json::from_value(load(fixture)).unwrap();
        assert_eq!(m.outcome, outcome);
        assert_eq!(m.accepted.map(|a| a.sequence), Some(42));
    }
    let t: WatchTrickplay = serde_json::from_value(load("upstream_get_watch_trickplay_ok.json")).unwrap();
    assert!(t.tile_columns > 0 && !t.sheets.is_empty());
    let e: EventsSocketTicket = serde_json::from_value(load("upstream_events_socket_ticket.json")).unwrap();
    assert_eq!(e.protocol, "silo.events.v2");
}

#[test]
fn granary_playback_request_bodies_match_the_upstream_schemas() {
    let caps: PlaybackCapabilities =
        serde_json::from_value(load("upstream_playback_capability_available.json")).unwrap();
    let start = granary_start_body(&caps);
    assert_valid(
        &serde_json::to_value(&start).unwrap(),
        &schema_of("PlaybackStartBody"),
        "start body",
    );
    // Unknown output HDR state is valid too.
    let unknown = PlaybackStartBody {
        client_playback_context: playback_context("0.1.0", "10", &OutputFacts::default()),
        ..start.clone()
    };
    assert_valid(
        &serde_json::to_value(&unknown).unwrap(),
        &schema_of("PlaybackStartBody"),
        "start body (unknown HDR)",
    );
    let progress = PlaybackProgressBody {
        installation_id: start.installation_id.clone(),
        sequence: 1,
        position: 12.5,
        is_paused: false,
    };
    assert_valid(
        &serde_json::to_value(&progress).unwrap(),
        &schema_of("PlaybackProgressBody"),
        "progress body",
    );
    for stop in [
        PlaybackStopBody {
            installation_id: start.installation_id.clone(),
            stop_id: mint_id(),
            sequence: Some(2),
            position: Some(20.0),
            is_paused: Some(true),
        },
        PlaybackStopBody {
            installation_id: start.installation_id.clone(),
            stop_id: mint_id(),
            sequence: None,
            position: None,
            is_paused: None,
        },
    ] {
        assert_valid(
            &serde_json::to_value(&stop).unwrap(),
            &schema_of("PlaybackStopBody"),
            "stop body",
        );
    }
    let replan = PlaybackReplanBody {
        installation_id: start.installation_id.clone(),
        protocol_version: PROTOCOL_VERSION,
        operation: "track_change".into(),
        playback_attempt_id: start.playback_attempt_id.clone(),
        replan_request_id: mint_id(),
        failed_plan_id: "plan:478677870860e5e5108c18bff749b34b".into(),
        plan_attempt_id: mint_id(),
        plan_attempt_key: "v3:f0144c47fa349e3e".into(),
        attempted_plan_keys: vec!["v3:f0144c47fa349e3e".into()],
        attempt_count: 1,
        quality_preference: "auto".into(),
        position_seconds: 30.0,
        selected_tracks: SelectedTracks {
            audio: Some(TrackIdentity {
                id: "file:42:audio:1".into(),
                index: Some(1),
            }),
            subtitle: None,
        },
        client_capabilities: start.client_capabilities.clone(),
        client_playback_context: start.client_playback_context.clone(),
        client_features: start.client_features.clone(),
        failure: None,
    };
    assert_valid(
        &serde_json::to_value(&replan).unwrap(),
        &schema_of("PlaybackReplanBody"),
        "replan body",
    );
}

#[tokio::test]
async fn http_client_speaks_the_playback_contract() {
    let server = MockServer::start().await;
    for (id, status, fixture) in RESPONSES.iter().filter(|r| {
        PLAYBACK_OPS.contains(&r.0)
            && (r.1 == "200" || r.1 == "201")
            && !r.2.contains("stale")
            && !r.2.contains("replayed")
    }) {
        mount(
            &server,
            id,
            status.parse().unwrap(),
            fixture,
            &[("session_id", SESSION), ("id", "42")],
        )
        .await;
    }
    let api = HttpSiloApi::new(device()).unwrap();
    let o = server.uri();
    let a = auth(Some(("p-primary", None)));
    let caps = api.playback_capabilities(&o, &a).await.unwrap();
    let start = granary_start_body(&caps);
    let d = api.start_playback(&o, &a, &start).await.unwrap();
    assert_eq!(d.session_id.as_deref(), Some(SESSION));
    let p = PlaybackProgressBody {
        installation_id: start.installation_id.clone(),
        sequence: 42,
        position: 120.0,
        is_paused: false,
    };
    assert_eq!(
        api.playback_progress(&o, &a, SESSION, &p).await.unwrap().outcome,
        "applied"
    );
    let stop = PlaybackStopBody {
        installation_id: start.installation_id.clone(),
        stop_id: mint_id(),
        sequence: None,
        position: None,
        is_paused: None,
    };
    assert_eq!(
        api.stop_playback(&o, &a, SESSION, &stop).await.unwrap().outcome,
        "stopped"
    );
    let replan = PlaybackReplanBody {
        installation_id: start.installation_id.clone(),
        protocol_version: PROTOCOL_VERSION,
        operation: "seek_reanchor".into(),
        playback_attempt_id: start.playback_attempt_id.clone(),
        replan_request_id: mint_id(),
        failed_plan_id: "plan:478677870860e5e5108c18bff749b34b".into(),
        plan_attempt_id: mint_id(),
        plan_attempt_key: "v3:f0144c47fa349e3e".into(),
        attempted_plan_keys: vec![],
        attempt_count: 1,
        quality_preference: "auto".into(),
        position_seconds: 30.0,
        client_capabilities: start.client_capabilities.clone(),
        client_playback_context: start.client_playback_context.clone(),
        client_features: start.client_features.clone(),
        ..PlaybackReplanBody::default()
    };
    assert!(
        api.replan_playback(&o, &a, SESSION, &replan)
            .await
            .unwrap()
            .playable()
            .is_some()
    );
    let t = api.watch_trickplay(&o, &a, "42", "42").await.unwrap();
    assert_eq!(t.file_id, "42");
    assert_eq!(api.events_ticket(&o, &a).await.unwrap().protocol, "silo.events.v2");

    // Every request carried the profile header and a body that matches the upstream request schema.
    let reqs = server.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 7);
    for req in &reqs {
        assert_eq!(req.headers.get("x-profile-id").unwrap(), "p-primary");
        assert_eq!(req.headers.get("authorization").unwrap(), "Bearer fixture-access-1");
        let id = PLAYBACK_OPS
            .iter()
            .find(|id| {
                let o = op(id);
                let re = o["path"]
                    .as_str()
                    .unwrap()
                    .replace("{session_id}", SESSION)
                    .replace("{id}", "42");
                o["method"].as_str().unwrap() == req.method.as_str() && re == req.url.path()
            })
            .unwrap_or_else(|| panic!("unexpected request {} {}", req.method, req.url));
        let o = op(id);
        if !o["requestBody"].is_null() {
            let body: Value = serde_json::from_slice(&req.body).unwrap();
            assert_valid(&body, &o["requestBody"], &format!("{id} request body"));
        } else {
            assert!(req.body.is_empty(), "{id} sends no body");
        }
        if *id == "getWatchTrickplay" {
            assert_eq!(req.url.query(), Some("file_id=42"));
        }
    }
}

#[tokio::test]
async fn media_requests_forward_ranges_and_never_follow_redirects() {
    use futures_util::TryStreamExt;
    let server = MockServer::start().await;
    let media_path = format!("/api/v2/stream/{SESSION}");
    Mock::given(method("GET"))
        .and(path(media_path.clone()))
        .and(header("range", "bytes=10-19"))
        .and(header("authorization", "Bearer fixture-access-1"))
        .and(header("x-profile-id", "p-primary"))
        .and(header("X-Silo-Device-Id", device().id.as_str()))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-range", "bytes 10-19/100")
                .insert_header("accept-ranges", "bytes")
                .insert_header("set-cookie", "leak=1")
                .insert_header("x-internal", "nope")
                .set_body_raw(vec![7u8; 10], "video/x-matroska"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/redirect"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "https://evil.test/x"))
        .mount(&server)
        .await;
    Mock::given(method("HEAD"))
        .and(path(media_path.clone()))
        .respond_with(ResponseTemplate::new(410))
        .mount(&server)
        .await;
    let api = HttpSiloApi::new(device()).unwrap();
    let a = auth(Some(("p-primary", None)));
    let req = |url: String, head: bool, headers: Vec<(String, String)>, auth: Option<Auth>| MediaRequest {
        url,
        auth,
        head,
        headers,
    };
    let r = api
        .fetch_media(req(
            format!("{}{media_path}", server.uri()),
            false,
            vec![("range".into(), "bytes=10-19".into()), ("cookie".into(), "no=1".into())],
            Some(a.clone()),
        ))
        .await
        .unwrap();
    assert_eq!(r.status, 206);
    assert_eq!(r.header("content-range"), Some("bytes 10-19/100"));
    assert_eq!(r.header("set-cookie"), None);
    assert_eq!(r.header("x-internal"), None);
    let body: Vec<bytes::Bytes> = r.body.try_collect().await.unwrap();
    assert_eq!(body.concat(), vec![7u8; 10]);
    // The cookie header was not forwarded.
    let reqs = server.received_requests().await.unwrap();
    assert!(reqs[0].headers.get("cookie").is_none());

    let redirect = api
        .fetch_media(req(
            format!("{}/redirect", server.uri()),
            false,
            vec![],
            Some(a.clone()),
        ))
        .await;
    assert!(matches!(redirect, Err(ApiError::Redirected { .. })), "{redirect:?}");
    let gone = api
        .fetch_media(req(format!("{}{media_path}", server.uri()), true, vec![], Some(a)))
        .await;
    assert!(matches!(gone, Err(ApiError::Http { status: 410, .. })), "{gone:?}");
    // Without auth no credential or device header is sent (foreign origins).
    let _ = api
        .fetch_media(req(format!("{}/redirect", server.uri()), false, vec![], None))
        .await;
    let last = server.received_requests().await.unwrap().pop().unwrap();
    assert!(last.headers.get("authorization").is_none() && last.headers.get("x-silo-device-id").is_none());
}

/// 0.2 operations: flags, rating, person, library tabs, similar titles and filters, with the method, path, profile
/// headers and (for `setRating`) a body that validates against `RatingSet`.
#[tokio::test]
async fn http_client_speaks_the_detail_and_library_contract() {
    use wiremock::matchers::body_json;
    let server = MockServer::start().await;
    let params = [("id", "m1"), ("item_id", "m1"), ("collection_id", "c1")];
    for (id, _, _) in FLAG_OPS {
        let o = op(id);
        assert!(
            o["responses"]["204"].is_null() && o["responses"].get("204").is_some(),
            "{id} answers 204"
        );
        let p = o["path"]
            .as_str()
            .unwrap()
            .replace("{item_id}", "m1")
            .replace("{id}", "m1");
        Mock::given(method(o["method"].as_str().unwrap()))
            .and(path(p))
            .and(header("X-Profile-Id", "2"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
    }
    let rating = serde_json::json!({ "rating": 4 });
    assert_valid(&rating, &op("setRating")["requestBody"], "setRating body");
    Mock::given(method("PUT"))
        .and(path("/api/v2/ratings/m1"))
        .and(body_json(&rating))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/v2/ratings/m1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    for (id, status, fixture) in RESPONSES.iter().filter(|r| {
        [
            "getPerson",
            "listLibrarySections",
            "getLibraryCollections",
            "getLibraryCollectionItems",
            "listSimilar",
            "getCatalogFilters",
        ]
        .contains(&r.0)
    }) {
        let id_param = if *id == "getPerson" {
            "p1"
        } else if id.starts_with("listSimilar") {
            "m1"
        } else {
            "lib-1"
        };
        let mut ps = params.to_vec();
        ps.retain(|(k, _)| *k != "id");
        ps.push(("id", id_param));
        mount(&server, id, status.parse().unwrap(), fixture, &ps).await;
    }
    let api = HttpSiloApi::new(device()).unwrap();
    let o = server.uri();
    let a = auth(Some(("2", None)));
    for (_, flag, on) in FLAG_OPS {
        api.set_flag(&o, &a, *flag, "m1", *on).await.unwrap();
    }
    api.set_rating(&o, &a, "m1", Some(4)).await.unwrap();
    api.set_rating(&o, &a, "m1", None).await.unwrap();
    assert_eq!(api.person(&o, &a, "p1").await.unwrap().name, "Ada Brook");
    assert!(!api.library_sections(&o, &a, "lib-1").await.unwrap().sections.is_empty());
    let tab = api.library_collections(&o, &a, "lib-1").await.unwrap();
    assert_eq!(
        tab.cards().iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["c1", "c2"]
    );
    let page = PageQuery {
        limit: Some(60),
        cursor: None,
    };
    let items = api
        .library_collection_items(&o, &a, "lib-1", "c1", &page)
        .await
        .unwrap();
    assert_eq!(items.page.and_then(|p| p.next_cursor).as_deref(), Some("cursor-2"));
    let similar = api.similar(&o, &a, "m1").await.unwrap();
    assert_eq!(
        similar.items[0].overlay_summary.as_ref().and_then(|b| b.hdr.as_deref()),
        Some("DV HDR10")
    );
    assert_eq!(
        api.catalog_filters(&o, &a, "lib-1").await.unwrap().genres,
        ["Comedy", "Drama"]
    );
    for req in server.received_requests().await.unwrap() {
        let url = req.url.path().to_owned();
        let has = |h: &str| req.headers.contains_key(h);
        assert!(
            has("authorization") && has("x-profile-id"),
            "{url} carries bearer and profile headers"
        );
    }
}
