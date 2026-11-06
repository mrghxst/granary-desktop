//! Contract tests (Phase 4): sanitized fixtures are checked against the upstream OpenAPI subset, then served from a
//! local mock server to the real `HttpSiloApi`, whose requests (method, path, headers, body) are checked against the
//! same subset.
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: a panic is the failure report

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
    for (id, status, fixture) in RESPONSES.iter().filter(|r| r.1 == "200") {
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
