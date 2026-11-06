//! In-process [`SiloApi`] that behaves like a small Silo server: token rotation, expiry, revocation, PIN profiles,
//! profile-restricted libraries. Used by unit tests, the desktop host's tests and UI development without a server
//! (ADR-0004 "Consequences"). Data is synthetic.
use crate::api::{Auth, SiloApi};
use crate::error::{ApiError, Result};
use crate::models::*;
use crate::secret::Secret;
use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

/// Problem type URI for a problem id, as the server builds it (`apiv2/problem.go@6791aac` L22).
pub fn problem(status: u16, id: &str) -> ApiError {
    ApiError::Http {
        status,
        problem: Some(Box::new(Problem {
            type_uri: format!("https://siloserver.org/docs/api/v2/problems/{id}"),
            title: id.into(),
            status: i64::from(status),
            detail: String::new(),
            instance: "urn:silo:request:mock".into(),
        })),
        retry_after: None,
    }
}

#[derive(Default)]
struct State {
    counter: u64,
    access_lifetime: i64,
    live_access: HashSet<String>,
    /// refresh token → user id
    live_refresh: HashMap<String, String>,
    /// profile token → profile id
    profile_tokens: HashMap<String, String>,
    refresh_calls: u32,
    unauthorized: u32,
    refresh_503: bool,
}

pub struct MockSiloApi {
    pub info: SystemInfo,
    pub server_id: String,
    users: Vec<(String, String, Account)>,
    profiles: Vec<Profile>,
    pins: HashMap<String, String>,
    libraries: Vec<UserLibrary>,
    /// profile id → allowed library ids (absent: all)
    restrictions: HashMap<String, Vec<String>>,
    state: Mutex<State>,
}

fn profile(id: &str, name: &str, has_pin: bool, is_child: bool) -> Profile {
    Profile {
        id: id.into(),
        name: name.into(),
        has_pin,
        is_child,
        is_primary: id == "1",
        avatar_url: None,
        language: "en".into(),
        subtitle_language: String::new(),
        show_forced_subtitles: true,
        auto_skip_intro: false,
        auto_skip_credits: false,
        auto_skip_recap: false,
        max_playback_quality: String::new(),
    }
}

fn library(id: &str, name: &str, kind: &str, order: i64) -> UserLibrary {
    UserLibrary {
        id: id.into(),
        name: name.into(),
        kind: kind.into(),
        sort_order: order,
        box_sets_enabled: false,
        poster_url: None,
    }
}

impl MockSiloApi {
    /// A server with user `alice`/`pw`, profiles Alice (no PIN) and Kids (PIN 1234, one library), two libraries.
    pub fn demo() -> Self {
        let account = Account {
            id: "1".into(),
            username: "alice".into(),
            email: String::new(),
            role: "user".into(),
            permissions: Vec::new(),
            download_allowed: true,
            password_change_required: false,
        };
        Self {
            info: SystemInfo {
                api_major: 2,
                server_version: "mock".into(),
                contract_digest: crate::TESTED_CONTRACT_DIGESTS[0].into(),
                links: SystemInfoLinks {
                    openapi: "/api/v2/openapi.json".into(),
                    capabilities: "/api/v2/capabilities".into(),
                    identity: "/api/v2/system/identity".into(),
                },
                request_origin: None,
            },
            server_id: "3f2a9d5e-6b1c-4c7e-9a0d-2f4b8c1e7a35".into(),
            users: vec![("alice".into(), "pw".into(), account)],
            profiles: vec![profile("1", "Alice", false, false), profile("2", "Kids", true, true)],
            pins: HashMap::from([("2".to_owned(), "1234".to_owned())]),
            libraries: vec![library("1", "Movies", "movies", 0), library("2", "Shows", "series", 1)],
            restrictions: HashMap::from([("2".to_owned(), vec!["2".to_owned()])]),
            state: Mutex::new(State {
                access_lifetime: 3600,
                ..State::default()
            }),
        }
    }

    fn with_state<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        match self.state.lock() {
            Ok(mut s) => f(&mut s),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    pub fn set_access_lifetime(&self, secs: i64) {
        self.with_state(|s| s.access_lifetime = secs);
    }
    /// Every issued access token becomes invalid (as after expiry).
    pub fn expire_access_tokens(&self) {
        self.with_state(|s| s.live_access.clear());
    }
    /// Every login session is revoked: refresh answers 401.
    pub fn revoke_all_sessions(&self) {
        self.with_state(|s| {
            s.live_access.clear();
            s.live_refresh.clear();
        });
    }
    pub fn fail_refresh_with_503(&self, on: bool) {
        self.with_state(|s| s.refresh_503 = on);
    }
    pub fn refresh_calls(&self) -> u32 {
        self.with_state(|s| s.refresh_calls)
    }
    pub fn unauthorized_responses(&self) -> u32 {
        self.with_state(|s| s.unauthorized)
    }
    pub fn live_sessions(&self) -> usize {
        self.with_state(|s| s.live_refresh.len())
    }
    pub fn is_live_refresh(&self, token: &str) -> bool {
        self.with_state(|s| s.live_refresh.contains_key(token))
    }

    fn issue(&self, user_id: &str) -> (Secret, Secret, i64) {
        self.with_state(|s| {
            s.counter += 1;
            let access = format!("mock-access-{}", s.counter);
            let refresh = format!("mock-refresh-{}", s.counter);
            s.live_access.insert(access.clone());
            s.live_refresh.insert(refresh.clone(), user_id.to_owned());
            (Secret::new(access), Secret::new(refresh), s.access_lifetime)
        })
    }

    fn check(&self, auth: &Auth) -> Result<()> {
        self.with_state(|s| {
            if s.live_access.contains(auth.access.expose()) {
                Ok(())
            } else {
                s.unauthorized += 1;
                Err(problem(401, "invalid_token"))
            }
        })
    }

    /// Profile gate: a PIN profile needs a valid profile token.
    fn check_profile(&self, auth: &Auth) -> Result<Option<String>> {
        let Some(p) = &auth.profile else { return Ok(None) };
        if !self.profiles.iter().any(|x| x.id == p.id) {
            return Err(problem(404, "not_found"));
        }
        if self.pins.contains_key(&p.id) {
            let ok = p
                .token
                .as_ref()
                .is_some_and(|t| self.with_state(|s| s.profile_tokens.get(t.expose()) == Some(&p.id)));
            if !ok {
                return Err(problem(403, "profile_verification_required"));
            }
        }
        Ok(Some(p.id.clone()))
    }
}

#[async_trait]
impl SiloApi for MockSiloApi {
    async fn system_info(&self, _origin: &str) -> Result<SystemInfo> {
        Ok(self.info.clone())
    }

    async fn server_identity(&self, _origin: &str) -> Result<ServerIdentity> {
        Ok(ServerIdentity {
            server_id: self.server_id.clone(),
        })
    }

    async fn login(&self, _origin: &str, username: &str, password: &Secret) -> Result<TokenPair> {
        let Some((_, _, user)) = self
            .users
            .iter()
            .find(|(u, p, _)| u == username && p == password.expose())
        else {
            return Err(ApiError::InvalidCredentials);
        };
        let (access_token, refresh_token, expires_in) = self.issue(&user.id);
        Ok(TokenPair {
            access_token,
            refresh_token,
            expires_in,
            user: user.clone(),
        })
    }

    async fn refresh(&self, _origin: &str, refresh_token: &Secret) -> Result<RefreshedTokens> {
        // Widen the race window so single-flight tests are meaningful.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let user = self.with_state(|s| {
            s.refresh_calls += 1;
            if s.refresh_503 {
                return Err(match problem(503, "dependency_unavailable") {
                    ApiError::Http { status, problem, .. } => ApiError::Http {
                        status,
                        problem,
                        retry_after: Some(5),
                    },
                    other => other,
                });
            }
            s.live_refresh
                .remove(refresh_token.expose())
                .ok_or_else(|| problem(401, "session_expired"))
        })?;
        let (access_token, refresh_token, expires_in) = self.issue(&user);
        Ok(RefreshedTokens {
            access_token,
            refresh_token,
            expires_in,
        })
    }

    async fn logout(&self, _origin: &str, auth: &Auth) -> Result<()> {
        self.check(auth)?;
        self.with_state(|s| {
            s.live_access.remove(auth.access.expose());
            s.live_refresh.clear();
        });
        Ok(())
    }

    async fn current_user(&self, _origin: &str, auth: &Auth) -> Result<Account> {
        self.check(auth)?;
        self.users
            .first()
            .map(|(_, _, a)| a.clone())
            .ok_or_else(|| problem(404, "not_found"))
    }

    async fn list_profiles(&self, _origin: &str, auth: &Auth) -> Result<ProfileCollection> {
        self.check(auth)?;
        Ok(ProfileCollection {
            items: self.profiles.clone(),
        })
    }

    async fn verify_profile_pin(
        &self,
        _origin: &str,
        auth: &Auth,
        profile_id: &str,
        pin: &Secret,
    ) -> Result<ProfileVerification> {
        self.check(auth)?;
        let Some(expected) = self.pins.get(profile_id) else {
            return Err(problem(404, "not_found"));
        };
        if expected != pin.expose() {
            return Ok(ProfileVerification {
                valid: false,
                profile_token: None,
                expires_at: None,
            });
        }
        let token = self.with_state(|s| {
            s.counter += 1;
            let t = format!("pvt_mock{}", s.counter);
            s.profile_tokens.insert(t.clone(), profile_id.to_owned());
            t
        });
        Ok(ProfileVerification {
            valid: true,
            profile_token: Some(Secret::new(token)),
            expires_at: None,
        })
    }

    async fn list_user_libraries(&self, _origin: &str, auth: &Auth) -> Result<Vec<UserLibrary>> {
        self.check(auth)?;
        let profile = self.check_profile(auth)?;
        let allowed = profile.and_then(|p| self.restrictions.get(&p));
        Ok(self
            .libraries
            .iter()
            .filter(|l| allowed.is_none_or(|ids| ids.contains(&l.id)))
            .cloned()
            .collect())
    }
}
