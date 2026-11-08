//! One signed-in account on one server: in-memory access token, refresh token in the credential store, selected
//! profile. Implements the client rules of silo-api.md Â§1.3:
//! - refresh proactively [`REFRESH_MARGIN`] before expiry;
//! - on a 401 refresh **once** (single-flight: concurrent requests share one refresh) and retry the request once;
//! - a refresh rejected with 401/403 means the session is revoked: forget the credential, report [`ApiError::SignedOut`];
//! - a refresh that fails for any other reason (503 + `Retry-After`, network) keeps the tokens.
use crate::api::{Auth, PageQuery, ProfileAuth, SiloApi, UserFlag};
use crate::catalog::*;
use crate::credentials::{CredentialStore, account_key};
use crate::error::{ApiError, Result};
use crate::media::{MediaRequest, MediaResponse, MediaTarget, resolve_media_url};
use crate::models::{Account, Profile, UserLibrary};
use crate::playback::*;
use crate::registry::AccountRecord;
use crate::secret::Secret;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex as AsyncMutex, RwLock};

/// Android's `PROACTIVE_REFRESH_MARGIN_MS` (`AuthInterceptorImpl.kt@ee178d8` L25).
pub const REFRESH_MARGIN: Duration = Duration::from_secs(60);

struct Tokens {
    access: Option<Secret>,
    refresh: Secret,
    expires_at: Option<Instant>,
    /// Bumped on every successful refresh; lets a waiting request see that another one already refreshed.
    generation: u64,
}

pub struct Session {
    api: Arc<dyn SiloApi>,
    store: Arc<dyn CredentialStore>,
    origin: String,
    credential_key: String,
    account: AccountRecord,
    tokens: RwLock<Tokens>,
    refresh_lock: AsyncMutex<()>,
    profile: std::sync::RwLock<Option<ProfileAuth>>,
    signed_out: AtomicBool,
}

fn expiry(expires_in: i64) -> Option<Instant> {
    let secs = u64::try_from(expires_in).ok()?;
    Instant::now().checked_add(Duration::from_secs(secs))
}

impl Session {
    fn new(
        api: Arc<dyn SiloApi>,
        store: Arc<dyn CredentialStore>,
        origin: &str,
        server_id: &str,
        account: AccountRecord,
        tokens: Tokens,
    ) -> Self {
        Self {
            credential_key: account_key(server_id, &account.user_id),
            api,
            store,
            origin: origin.to_owned(),
            account,
            tokens: RwLock::new(tokens),
            refresh_lock: AsyncMutex::new(()),
            profile: std::sync::RwLock::new(None),
            signed_out: AtomicBool::new(false),
        }
    }

    /// Password login; persists the refresh token.
    pub async fn login(
        api: Arc<dyn SiloApi>,
        store: Arc<dyn CredentialStore>,
        origin: &str,
        server_id: &str,
        username: &str,
        password: &Secret,
    ) -> Result<(Self, Account)> {
        let pair = api.login(origin, username, password).await?;
        let account = AccountRecord {
            user_id: pair.user.id.clone(),
            username: pair.user.username.clone(),
        };
        store.set(&account_key(server_id, &account.user_id), &pair.refresh_token)?;
        let tokens = Tokens {
            access: Some(pair.access_token),
            refresh: pair.refresh_token,
            expires_at: expiry(pair.expires_in),
            generation: 0,
        };
        Ok((Self::new(api, store, origin, server_id, account, tokens), pair.user))
    }

    /// Restore a session from the stored refresh token; `None` when no credential is stored. The first request
    /// refreshes (there is no access token yet).
    pub fn resume(
        api: Arc<dyn SiloApi>,
        store: Arc<dyn CredentialStore>,
        origin: &str,
        server_id: &str,
        account: AccountRecord,
    ) -> Result<Option<Self>> {
        let Some(refresh) = store.get(&account_key(server_id, &account.user_id))? else {
            return Ok(None);
        };
        let tokens = Tokens {
            access: None,
            refresh,
            expires_at: None,
            generation: 0,
        };
        Ok(Some(Self::new(api, store, origin, server_id, account, tokens)))
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn account(&self) -> &AccountRecord {
        &self.account
    }

    pub fn is_signed_out(&self) -> bool {
        self.signed_out.load(Ordering::SeqCst)
    }

    pub fn profile(&self) -> Option<ProfileAuth> {
        self.profile.read().ok().and_then(|p| p.clone())
    }

    pub fn set_profile(&self, profile: Option<ProfileAuth>) {
        if let Ok(mut p) = self.profile.write() {
            *p = profile;
        }
    }

    /// Tokens for the next request. Account-level calls (`with_profile == false`) never carry the profile headers:
    /// upstream validates a supplied profile token on every route, so a stale one would lock the user out of the
    /// profile list and PIN unlock (review phase-4 M1).
    async fn current(&self, with_profile: bool) -> Result<(Auth, u64)> {
        if self.is_signed_out() {
            return Err(ApiError::SignedOut);
        }
        let (stale, generation) = {
            let t = self.tokens.read().await;
            let stale = t.access.is_none()
                || t.expires_at
                    .is_some_and(|e| e.saturating_duration_since(Instant::now()) < REFRESH_MARGIN);
            (stale, t.generation)
        };
        if stale {
            self.refresh(generation).await?;
        }
        let t = self.tokens.read().await;
        let access = t.access.clone().ok_or(ApiError::SignedOut)?;
        Ok((
            Auth {
                access,
                profile: if with_profile { self.profile() } else { None },
            },
            t.generation,
        ))
    }

    /// Single-flight refresh. `seen` is the generation the caller's failed/stale token belonged to; if another task
    /// refreshed meanwhile, return at once.
    async fn refresh(&self, seen: u64) -> Result<()> {
        let _guard = self.refresh_lock.lock().await;
        if self.is_signed_out() {
            return Err(ApiError::SignedOut);
        }
        let refresh = {
            let t = self.tokens.read().await;
            if t.generation != seen {
                return Ok(());
            }
            t.refresh.clone()
        };
        match self.api.refresh(&self.origin, &refresh).await {
            Ok(new) => {
                if let Err(e) = self.store.set(&self.credential_key, &new.refresh_token) {
                    // The session continues in memory; the next app start will need a new login.
                    tracing::warn!(error = %e, "could not persist the rotated refresh token");
                }
                let mut t = self.tokens.write().await;
                t.access = Some(new.access_token);
                t.refresh = new.refresh_token;
                t.expires_at = expiry(new.expires_in);
                t.generation += 1;
                tracing::debug!(generation = t.generation, "session refreshed");
                Ok(())
            }
            Err(ApiError::Http {
                status: 401 | 403,
                ref problem,
                ..
            }) => {
                tracing::warn!(
                    problem = problem.as_ref().map(|p| p.id()).unwrap_or_default(),
                    "refresh rejected; signing out"
                );
                self.forget().await;
                Err(ApiError::SignedOut)
            }
            Err(e) => Err(e),
        }
    }

    async fn forget(&self) {
        self.signed_out.store(true, Ordering::SeqCst);
        self.tokens.write().await.access = None;
        self.set_profile(None);
        if let Err(e) = self.store.delete(&self.credential_key) {
            tracing::warn!(error = %e, "could not delete the stored credential");
        }
    }

    /// Run a profile-scoped request (selected profile's headers attached) with the refresh rules above.
    pub async fn call<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: Fn(Auth) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.call_scoped(true, f).await
    }

    /// Run an account-level request: bearer token only, no profile headers.
    pub async fn call_account<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: Fn(Auth) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        self.call_scoped(false, f).await
    }

    async fn call_scoped<T, F, Fut>(&self, with_profile: bool, f: F) -> Result<T>
    where
        F: Fn(Auth) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let (auth, generation) = self.current(with_profile).await?;
        let first = f(auth).await;
        let result = match first {
            Err(ApiError::Http { status: 401, .. }) => {
                self.refresh(generation).await?;
                let (auth, _) = self.current(with_profile).await?;
                f(auth).await
            }
            other => other,
        };
        match result {
            Err(e) if e.problem_id() == Some("profile_verification_required") => {
                // The profile token expired or the PIN changed: drop it so the UI asks for the profile again.
                self.set_profile(None);
                Err(ApiError::ProfileVerificationRequired)
            }
            other => other,
        }
    }

    /// Revoke the server session (best effort) and forget the credential.
    pub async fn logout(&self) {
        if let Ok((auth, _)) = self.current(false).await
            && let Err(e) = self.api.logout(&self.origin, &auth).await
        {
            tracing::warn!(
                code = e.code(),
                "logout request failed; forgetting the credential anyway"
            );
        }
        self.forget().await;
    }

    pub async fn current_user(&self) -> Result<Account> {
        self.call_account(|auth| async move { self.api.current_user(&self.origin, &auth).await })
            .await
    }

    pub async fn profiles(&self) -> Result<Vec<Profile>> {
        self.call_account(|auth| async move { self.api.list_profiles(&self.origin, &auth).await })
            .await
            .map(|c| c.items)
    }

    /// Select a profile. A PIN-locked profile needs `pin`; the returned profile token is kept in memory only.
    pub async fn select_profile(&self, profile_id: &str, pin: Option<&Secret>) -> Result<()> {
        let token = match pin {
            None => None,
            Some(pin) => {
                let v = self
                    .call_account(|auth| async move {
                        self.api.verify_profile_pin(&self.origin, &auth, profile_id, pin).await
                    })
                    .await?;
                if !v.valid {
                    return Err(ApiError::WrongPin);
                }
                v.profile_token
            }
        };
        self.set_profile(Some(ProfileAuth {
            id: profile_id.to_owned(),
            token,
        }));
        Ok(())
    }

    pub async fn libraries(&self) -> Result<Vec<UserLibrary>> {
        self.call(|auth| async move { self.api.list_user_libraries(&self.origin, &auth).await })
            .await
    }

    /// Profile-scoped catalog request: `X-Profile-Id` is required by these operations (OAS @6791aac), so a missing
    /// profile is reported before any request is sent.
    async fn call_profile<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: Fn(Auth) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if self.profile().is_none() {
            return Err(ApiError::ProfileRequired);
        }
        self.call(f).await
    }

    pub async fn home_sections(&self) -> Result<Vec<Section>> {
        self.call_profile(|auth| async move { self.api.home_sections(&self.origin, &auth).await })
            .await
            .map(|c| c.sections)
    }

    pub async fn catalog_browse(&self, query: &CatalogQuery) -> Result<CatalogBrowseCollection> {
        self.call_profile(|auth| async move { self.api.catalog_browse(&self.origin, &auth, query).await })
            .await
    }

    pub async fn catalog_item(&self, id: &str) -> Result<CatalogItemDetail> {
        self.call_profile(|auth| async move { self.api.catalog_item(&self.origin, &auth, id).await })
            .await
    }

    pub async fn series_seasons(&self, series_id: &str) -> Result<Vec<Season>> {
        self.call_profile(|auth| async move { self.api.series_seasons(&self.origin, &auth, series_id).await })
            .await
            .map(|c| c.items)
    }

    pub async fn season_episodes(&self, series_id: &str, season: i64) -> Result<Vec<Episode>> {
        self.call_profile(|auth| async move { self.api.season_episodes(&self.origin, &auth, series_id, season).await })
            .await
            .map(|c| c.items)
    }

    pub async fn watch_state(&self, id: &str) -> Result<WatchDetail> {
        self.call_profile(|auth| async move { self.api.watch_state(&self.origin, &auth, id).await })
            .await
    }

    // Playback protocol v3 (silo-api.md Â§13, ADR-0008).

    pub async fn playback_capabilities(&self) -> Result<PlaybackCapabilities> {
        self.call_profile(|auth| async move { self.api.playback_capabilities(&self.origin, &auth).await })
            .await
    }

    pub async fn start_playback(&self, body: &PlaybackStartBody) -> Result<PlaybackDecision> {
        self.call_profile(|auth| async move { self.api.start_playback(&self.origin, &auth, body).await })
            .await
    }

    pub async fn playback_progress(&self, session_id: &str, body: &PlaybackProgressBody) -> Result<PlaybackMutation> {
        self.call_profile(|auth| async move { self.api.playback_progress(&self.origin, &auth, session_id, body).await })
            .await
    }

    pub async fn stop_playback(&self, session_id: &str, body: &PlaybackStopBody) -> Result<PlaybackMutation> {
        self.call_profile(|auth| async move { self.api.stop_playback(&self.origin, &auth, session_id, body).await })
            .await
    }

    pub async fn replan_playback(&self, session_id: &str, body: &PlaybackReplanBody) -> Result<PlaybackDecision> {
        self.call_profile(|auth| async move { self.api.replan_playback(&self.origin, &auth, session_id, body).await })
            .await
    }

    pub async fn watch_trickplay(&self, id: &str, file_id: &str) -> Result<WatchTrickplay> {
        self.call_profile(|auth| async move { self.api.watch_trickplay(&self.origin, &auth, id, file_id).await })
            .await
    }

    pub async fn set_flag(&self, flag: UserFlag, id: &str, on: bool) -> Result<()> {
        self.call_profile(|auth| async move { self.api.set_flag(&self.origin, &auth, flag, id, on).await })
            .await
    }

    pub async fn dismiss_home(&self, dismissal: &HomeDismissal) -> Result<()> {
        self.call_profile(|auth| async move { self.api.dismiss_home(&self.origin, &auth, dismissal).await })
            .await
    }

    pub async fn set_rating(&self, id: &str, rating: Option<i64>) -> Result<()> {
        self.call_profile(|auth| async move { self.api.set_rating(&self.origin, &auth, id, rating).await })
            .await
    }

    pub async fn person(&self, id: &str) -> Result<Person> {
        self.call_profile(|auth| async move { self.api.person(&self.origin, &auth, id).await })
            .await
    }

    pub async fn library_sections(&self, library_id: &str) -> Result<Vec<Section>> {
        self.call_profile(|auth| async move { self.api.library_sections(&self.origin, &auth, library_id).await })
            .await
            .map(|c| c.sections)
    }

    pub async fn library_collections(&self, library_id: &str) -> Result<Vec<CollectionCard>> {
        self.call_profile(|auth| async move { self.api.library_collections(&self.origin, &auth, library_id).await })
            .await
            .map(|t| t.cards())
    }

    pub async fn library_collection_items(
        &self,
        library_id: &str,
        collection_id: &str,
        page: &PageQuery,
    ) -> Result<CatalogItemCollection> {
        self.call_profile(|auth| async move {
            self.api
                .library_collection_items(&self.origin, &auth, library_id, collection_id, page)
                .await
        })
        .await
    }

    pub async fn similar(&self, id: &str) -> Result<Vec<CatalogItem>> {
        self.call_profile(|auth| async move { self.api.similar(&self.origin, &auth, id).await })
            .await
            .map(|c| c.items)
    }

    pub async fn catalog_filters(&self, library_id: &str) -> Result<CatalogFilters> {
        self.call_profile(|auth| async move { self.api.catalog_filters(&self.origin, &auth, library_id).await })
            .await
    }

    pub async fn events_ticket(&self) -> Result<EventsSocketTicket> {
        self.call_profile(|auth| async move { self.api.events_ticket(&self.origin, &auth).await })
            .await
    }

    /// Mint a ticket and open the events socket (silo-api.md §9). The socket request itself carries no profile
    /// headers (`EventsSocketV2Api.kt@ee178d8` L146); the ticket is bound to the profile proof.
    pub async fn open_events(&self) -> Result<crate::events::EventsSocket> {
        let ticket = self.events_ticket().await?;
        crate::events::EventsSocket::connect(&self.origin, &ticket.ticket).await
    }

    /// Fetch a media URL the server handed out (`stream.url`, subtitle and font URLs). Only URLs on the saved
    /// server origin are allowed here, and they get the bearer/profile headers; the caller hands foreign URLs to
    /// the player directly ([`crate::media::resolve_media_url`]).
    pub async fn fetch_media(&self, raw_url: &str, head: bool, headers: &[(String, String)]) -> Result<MediaResponse> {
        let MediaTarget::Server(url) = resolve_media_url(&self.origin, raw_url)? else {
            return Err(ApiError::Internal("media URL is not on the server origin".into()));
        };
        self.call_profile(|auth| {
            let request = MediaRequest {
                url: url.clone(),
                auth: Some(auth),
                head,
                headers: headers.to_vec(),
            };
            async move { self.api.fetch_media(request).await }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::credentials::MemoryStore;
    use crate::mock::MockSiloApi;

    const ORIGIN: &str = "https://mock.invalid";

    async fn signed_in() -> (Arc<MockSiloApi>, Arc<MemoryStore>, Session) {
        let api = Arc::new(MockSiloApi::demo());
        let store = Arc::new(MemoryStore::default());
        let (s, user) = Session::login(api.clone(), store.clone(), ORIGIN, "srv", "alice", &Secret::new("pw"))
            .await
            .ok()
            .unzip();
        assert_eq!(user.map(|u| u.username).as_deref(), Some("alice"));
        let Some(s) = s else { panic!("login failed") };
        (api, store, s)
    }

    #[tokio::test]
    async fn login_persists_refresh_token_and_wrong_password_is_reported() {
        let (_, store, s) = signed_in().await;
        assert_eq!(store.len(), 1);
        assert_eq!(s.account().username, "alice");
        let api = Arc::new(MockSiloApi::demo());
        let r = Session::login(api, store, ORIGIN, "srv", "alice", &Secret::new("nope")).await;
        assert!(matches!(r, Err(ApiError::InvalidCredentials)));
    }

    #[tokio::test]
    async fn expired_access_token_is_refreshed_once_for_concurrent_requests() {
        let (api, store, s) = signed_in().await;
        let s = Arc::new(s);
        api.expire_access_tokens();
        let tasks: Vec<_> = (0..8)
            .map(|_| {
                let s = s.clone();
                tokio::spawn(async move { s.current_user().await.map(|u| u.username) })
            })
            .collect();
        for t in tasks {
            assert_eq!(t.await.ok().and_then(|r| r.ok()).as_deref(), Some("alice"));
        }
        assert_eq!(api.refresh_calls(), 1, "single-flight refresh");
        // The rotated refresh token was persisted.
        let stored = store.get(&account_key("srv", "1")).ok().flatten();
        assert!(stored.is_some_and(|t| api.is_live_refresh(t.expose())));
    }

    #[tokio::test]
    async fn token_near_expiry_is_refreshed_proactively() {
        let api = Arc::new(MockSiloApi::demo());
        api.set_access_lifetime(30); // below REFRESH_MARGIN
        let store = Arc::new(MemoryStore::default());
        let s = Session::login(api.clone(), store, ORIGIN, "srv", "alice", &Secret::new("pw"))
            .await
            .ok();
        let Some((s, _)) = s else { panic!("login failed") };
        assert!(s.current_user().await.is_ok());
        assert_eq!(api.refresh_calls(), 1);
        assert_eq!(
            api.unauthorized_responses(),
            0,
            "no request was sent with the stale token"
        );
    }

    #[tokio::test]
    async fn revoked_session_signs_out_and_forgets_credential() {
        let (api, store, s) = signed_in().await;
        api.revoke_all_sessions();
        assert!(matches!(s.current_user().await, Err(ApiError::SignedOut)));
        assert!(s.is_signed_out());
        assert!(store.is_empty());
        assert!(matches!(s.libraries().await, Err(ApiError::SignedOut)));
    }

    #[tokio::test]
    async fn unavailable_refresh_keeps_tokens() {
        let (api, store, s) = signed_in().await;
        api.expire_access_tokens();
        api.fail_refresh_with_503(true);
        let r = s.current_user().await;
        assert!(matches!(
            r,
            Err(ApiError::Http {
                status: 503,
                retry_after: Some(5),
                ..
            })
        ));
        assert!(!s.is_signed_out());
        assert_eq!(store.len(), 1);
        api.fail_refresh_with_503(false);
        assert!(s.current_user().await.is_ok());
    }

    #[tokio::test]
    async fn resume_uses_stored_refresh_token() {
        let (api, store, s) = signed_in().await;
        let account = s.account().clone();
        drop(s);
        let resumed = Session::resume(api.clone(), store.clone(), ORIGIN, "srv", account.clone())
            .ok()
            .flatten();
        let Some(resumed) = resumed else {
            panic!("no stored credential")
        };
        assert!(resumed.current_user().await.is_ok());
        assert_eq!(api.refresh_calls(), 1);
        let none = Session::resume(api, Arc::new(MemoryStore::default()), ORIGIN, "srv", account);
        assert!(matches!(none, Ok(None)));
    }

    #[tokio::test]
    async fn pin_profiles_need_verification() {
        let (_, _, s) = signed_in().await;
        let profiles = s.profiles().await.unwrap_or_default();
        let kid = profiles
            .iter()
            .find(|p| p.has_pin)
            .map(|p| p.id.clone())
            .unwrap_or_default();
        // Selecting a PIN profile without verifying: the server answers profile_verification_required.
        s.set_profile(Some(ProfileAuth {
            id: kid.clone(),
            token: None,
        }));
        assert!(matches!(
            s.libraries().await,
            Err(ApiError::ProfileVerificationRequired)
        ));
        assert!(matches!(
            s.select_profile(&kid, Some(&Secret::new("0000"))).await,
            Err(ApiError::WrongPin)
        ));
        assert!(s.select_profile(&kid, Some(&Secret::new("1234"))).await.is_ok());
        assert_eq!(
            s.libraries().await.map(|l| l.len()).ok(),
            Some(1),
            "restricted profile sees one library"
        );
    }

    #[tokio::test]
    async fn stale_profile_token_does_not_block_account_calls() {
        let (api, _, s) = signed_in().await;
        assert!(s.select_profile("2", Some(&Secret::new("1234"))).await.is_ok());
        api.revoke_profile_tokens();
        // Account-level calls carry no profile headers, so the stale token cannot lock the profile picker.
        assert_eq!(s.profiles().await.map(|p| p.len()).ok(), Some(2));
        assert!(matches!(
            s.libraries().await,
            Err(ApiError::ProfileVerificationRequired)
        ));
        assert!(s.profile().is_none(), "stale profile token dropped");
        assert!(s.select_profile("2", Some(&Secret::new("1234"))).await.is_ok());
        assert_eq!(s.libraries().await.map(|l| l.len()).ok(), Some(1));
    }

    #[tokio::test]
    async fn catalog_needs_a_profile_and_respects_restrictions() {
        let (_, _, s) = signed_in().await;
        assert!(matches!(s.home_sections().await, Err(ApiError::ProfileRequired)));
        s.set_profile(Some(ProfileAuth {
            id: "1".into(),
            token: None,
        }));
        let sections = s.home_sections().await.unwrap_or_default();
        assert!(sections.iter().any(|x| x.featured && !x.items.is_empty()));
        let all = s.catalog_browse(&CatalogQuery::default()).await.map(|c| c.total).ok();
        let movies = s
            .catalog_browse(&CatalogQuery {
                library_id: Some("1".into()),
                ..CatalogQuery::default()
            })
            .await
            .map(|c| c.items.len())
            .ok();
        assert_eq!((all, movies), (Some(4), Some(3)));
        let found = s
            .catalog_browse(&CatalogQuery {
                q: Some("harbor".into()),
                ..CatalogQuery::default()
            })
            .await
            .map(|c| c.items.into_iter().map(|i| i.content_id).collect::<Vec<_>>())
            .unwrap_or_default();
        assert_eq!(found, ["m2"]);
        let detail = s.catalog_item("s1").await.ok();
        assert_eq!(detail.map(|d| d.kind), Some("series".to_owned()));
        let seasons = s.series_seasons("s1").await.unwrap_or_default();
        assert_eq!(seasons.len(), 1);
        assert_eq!(s.season_episodes("s1", 1).await.map(|e| e.len()).ok(), Some(3));
        let watch = s.watch_state("m1").await.ok();
        assert_eq!(watch.map(|w| w.versions.len()), Some(2));
        assert!(matches!(
            s.catalog_item("nope").await,
            Err(ApiError::Http { status: 404, .. })
        ));
    }

    #[tokio::test]
    async fn logout_revokes_and_forgets() {
        let (api, store, s) = signed_in().await;
        s.logout().await;
        assert!(s.is_signed_out());
        assert!(store.is_empty());
        assert_eq!(api.live_sessions(), 0);
    }

    fn start_body(file_id: &str, attempt: &str) -> PlaybackStartBody {
        PlaybackStartBody {
            installation_id: crate::mock::MOCK_INSTALLATION_ID.into(),
            protocol_version: PROTOCOL_VERSION,
            file_id: file_id.into(),
            profile_id: "1".into(),
            playback_attempt_id: attempt.into(),
            quality_preference: "auto".into(),
            subtitle_fidelity_preference: "preserve".into(),
            start_position: Some(42.0),
            audio_track_index: Some(0),
            ..PlaybackStartBody::default()
        }
    }

    fn progress(sequence: i64, position: f64) -> PlaybackProgressBody {
        PlaybackProgressBody {
            installation_id: crate::mock::MOCK_INSTALLATION_ID.into(),
            sequence,
            position,
            is_paused: false,
        }
    }

    #[tokio::test]
    #[allow(clippy::unwrap_used)]
    async fn playback_protocol_round_trip_against_the_mock() {
        let dir = std::env::temp_dir().join(format!("granary-mock-media-{}", mint_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let clip: Vec<u8> = (0..1000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(dir.join("hdr10_hevc_main10_2160p.mkv"), &clip).unwrap();
        let api = Arc::new(MockSiloApi::demo().with_media_dir(&dir));
        let store = Arc::new(MemoryStore::default());
        let (s, _) = Session::login(api.clone(), store, ORIGIN, "srv", "alice", &Secret::new("pw"))
            .await
            .unwrap();

        // Profile-scoped: nothing is sent before a profile is chosen.
        assert!(matches!(
            s.playback_capabilities().await,
            Err(ApiError::ProfileRequired)
        ));
        s.select_profile("1", None).await.unwrap();
        let caps = s.playback_capabilities().await.unwrap();
        assert!(caps.allowed && caps.protocol_versions.contains(&PROTOCOL_VERSION));

        // Start is idempotent per attempt id.
        let d = s.start_playback(&start_body("m1-f1", "attempt-0001")).await.unwrap();
        let again = s.start_playback(&start_body("m1-f1", "attempt-0001")).await.unwrap();
        assert_eq!(d, again);
        let (sid, plan) = d.playable().unwrap();
        let sid = sid.to_owned();
        assert_eq!(plan.delivery, DELIVERY_ORIGINAL_HTTP);
        assert_eq!(plan.timeline.player_start_seconds, 42.0);
        assert_eq!(plan.stream.url, format!("/api/v2/stream/{sid}"));
        assert!(matches!(
            s.start_playback(&start_body("m1-f2", "attempt-0001")).await,
            Err(e) if e.problem_id() == Some("playback_attempt_conflict")
        ));

        // Media: ranges and HEAD through the session's credentials.
        let r = s
            .fetch_media(&plan.stream.url, false, &[("range".into(), "bytes=100-199".into())])
            .await
            .unwrap();
        assert_eq!(r.status, 206);
        assert_eq!(r.header("content-range"), Some("bytes 100-199/1000"));
        let body: Vec<u8> = {
            use futures_util::TryStreamExt;
            let chunks: Vec<bytes::Bytes> = r.body.try_collect().await.unwrap();
            chunks.concat()
        };
        assert_eq!(body, clip[100..200]);
        let h = s.fetch_media(&plan.stream.url, true, &[]).await.unwrap();
        assert_eq!((h.status, h.header("content-length")), (200, Some("1000")));
        let bad = s
            .fetch_media(&plan.stream.url, false, &[("range".into(), "bytes=5000-".into())])
            .await
            .unwrap();
        assert_eq!(bad.status, 416);
        assert!(s.fetch_media("https://cdn.example.test/x", false, &[]).await.is_err());

        // Progress: newer applied, exact repeat replayed, older stale.
        let outcome = |m: PlaybackMutation| m.outcome;
        assert_eq!(
            outcome(s.playback_progress(&sid, &progress(1, 50.0)).await.unwrap()),
            "applied"
        );
        assert_eq!(
            outcome(s.playback_progress(&sid, &progress(1, 50.0)).await.unwrap()),
            "replayed"
        );
        assert_eq!(
            outcome(s.playback_progress(&sid, &progress(3, 70.0)).await.unwrap()),
            "applied"
        );
        let stale = s.playback_progress(&sid, &progress(2, 60.0)).await.unwrap();
        assert_eq!(stale.outcome, "stale_sample");
        assert_eq!(stale.accepted.map(|a| a.sequence), Some(3));

        // Replan (intent: no failure) keeps the session and moves the timeline.
        let replan = PlaybackReplanBody {
            installation_id: crate::mock::MOCK_INSTALLATION_ID.into(),
            protocol_version: PROTOCOL_VERSION,
            operation: "track_change".into(),
            playback_attempt_id: "attempt-0001".into(),
            replan_request_id: mint_id(),
            failed_plan_id: plan.plan_id.clone(),
            plan_attempt_id: mint_id(),
            plan_attempt_key: "v3:x".into(),
            attempt_count: 1,
            position_seconds: 70.0,
            ..PlaybackReplanBody::default()
        };
        let d2 = s.replan_playback(&sid, &replan).await.unwrap();
        assert_eq!(d2.session_id.as_deref(), Some(sid.as_str()));
        assert_eq!(d2.playback_plan.map(|p| p.timeline.player_start_seconds), Some(70.0));
        let with_failure = PlaybackReplanBody {
            failure: Some(Failure {
                classification: "decode".into(),
                message: None,
            }),
            ..replan
        };
        assert!(s.replan_playback(&sid, &with_failure).await.is_err());

        // Stop with a final sample, then replay; the session is gone afterwards.
        let stop = PlaybackStopBody {
            installation_id: crate::mock::MOCK_INSTALLATION_ID.into(),
            stop_id: mint_id(),
            sequence: Some(4),
            position: Some(80.0),
            is_paused: Some(true),
        };
        let first = s.stop_playback(&sid, &stop).await.unwrap();
        assert_eq!(first.outcome, "stopped");
        assert_eq!(first.accepted.map(|a| (a.sequence, a.position)), Some((4, 80.0)));
        let replay = s.stop_playback(&sid, &stop).await.unwrap();
        assert_eq!(
            (replay.outcome.as_str(), replay.stop_id),
            ("replayed", Some(stop.stop_id.clone()))
        );
        assert!(matches!(
            s.playback_progress(&sid, &progress(5, 90.0)).await,
            Err(ApiError::Http { status: 410, .. })
        ));
        assert!(s.fetch_media(&plan.stream.url, true, &[]).await.is_err());
        let not_uuid = PlaybackStopBody {
            stop_id: "stop-1".into(),
            ..stop
        };
        assert!(s.stop_playback(&sid, &not_uuid).await.is_err());

        let log = api.playback_log();
        assert_eq!(log.starts.len(), 3);
        assert_eq!(log.progress.len(), 4);
        assert_eq!(log.media_requests.len(), 3);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
