//! Server, account and profile management for the UI (Phase 4, ADR-0004). The WebView only ever sees the views
//! below: no access, refresh or profile token crosses the IPC boundary. Passwords and PINs arrive as command
//! arguments and are wrapped in [`Secret`] immediately.
use granary_silo_api::credentials::CredentialStore;
use granary_silo_api::registry::{Registry, ServerRecord};
use granary_silo_api::{ApiError, ProfileAuth, Secret, Session, SiloApi, probe};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

/// Event sent when the server rejects a stored session (refresh 401/403); payload [`RevokedEvent`].
pub const EVENT_REVOKED: &str = "auth://revoked";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RevokedEvent {
    pub server_id: String,
}

/// Error shape every command returns; the UI translates `code` (see `src/lib/errors.ts`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
    /// Problem identifier (final segment of the problem `type`), for diagnostics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl From<ApiError> for CommandError {
    fn from(e: ApiError) -> Self {
        let retry_after = match &e {
            ApiError::Http { retry_after, .. } => *retry_after,
            _ => None,
        };
        Self {
            code: e.code().to_owned(),
            message: e.to_string(),
            status: e.status(),
            retry_after,
            problem: e.problem_id().map(str::to_owned),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub user_id: String,
    pub username: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerView {
    pub server_id: String,
    pub origin: String,
    pub server_version: String,
    pub untested: bool,
    pub insecure: bool,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<AccountView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbeView {
    pub origin: String,
    pub server_id: String,
    pub server_version: String,
    pub api_major: i64,
    pub untested: bool,
    pub insecure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_origin: Option<String>,
    /// Already saved (same `server_id`); adding it again updates the address.
    pub known: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LoginView {
    pub account: AccountView,
    pub password_change_required: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    pub has_pin: bool,
    pub is_child: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub id: String,
    pub name: String,
    pub kind: String,
}

fn untested(digest: &str) -> bool {
    !granary_silo_api::TESTED_CONTRACT_DIGESTS.contains(&digest)
}

pub struct SiloState {
    api: Arc<dyn SiloApi>,
    store: Arc<dyn CredentialStore>,
    registry_path: PathBuf,
    registry: Mutex<Registry>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
}

impl SiloState {
    pub fn new(api: Arc<dyn SiloApi>, store: Arc<dyn CredentialStore>, registry_path: PathBuf) -> Self {
        let registry = Registry::load(&registry_path);
        Self {
            api,
            store,
            registry_path,
            registry: Mutex::new(registry),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn view(r: &ServerRecord, active: Option<&str>) -> ServerView {
        ServerView {
            server_id: r.server_id.clone(),
            origin: r.origin.clone(),
            server_version: r.server_version.clone(),
            untested: untested(&r.contract_digest),
            insecure: r.origin.starts_with("http://"),
            active: active == Some(r.server_id.as_str()),
            account: r.account.as_ref().map(|a| AccountView {
                user_id: a.user_id.clone(),
                username: a.username.clone(),
            }),
            last_profile_id: r.last_profile_id.clone(),
        }
    }

    async fn update_registry<T>(&self, f: impl FnOnce(&mut Registry) -> T) -> CmdResult<T> {
        let mut reg = self.registry.lock().await;
        let out = f(&mut reg);
        reg.save(&self.registry_path)?;
        Ok(out)
    }

    pub async fn servers(&self) -> Vec<ServerView> {
        let reg = self.registry.lock().await;
        reg.servers
            .iter()
            .map(|r| Self::view(r, reg.active.as_deref()))
            .collect()
    }

    pub async fn probe(&self, url: &str) -> CmdResult<ProbeView> {
        let p = probe(self.api.as_ref(), url).await?;
        let known = self.registry.lock().await.get(&p.server_id).is_some();
        Ok(ProbeView {
            origin: p.origin,
            server_id: p.server_id,
            server_version: p.info.server_version,
            api_major: p.info.api_major,
            untested: p.untested,
            insecure: p.insecure,
            reported_origin: p.reported_origin,
            known,
        })
    }

    pub async fn add(&self, url: &str) -> CmdResult<ServerView> {
        let p = probe(self.api.as_ref(), url).await?;
        let previous = self.registry.lock().await.get(&p.server_id).cloned();
        if let Some(prev) = previous.filter(|r| r.origin != p.origin && r.account.is_some()) {
            // Same server id at a new address: the id comes from a public endpoint, so the stored refresh token must
            // not follow it. Revoke it at the old origin and forget it; the user signs in again (review phase-4 H1).
            tracing::info!(server_id = %p.server_id, "server address changed; signing out of the old address");
            self.sign_out(&prev.server_id).await?;
        }
        let record = ServerRecord {
            server_id: p.server_id.clone(),
            origin: p.origin,
            server_version: p.info.server_version,
            contract_digest: p.info.contract_digest,
            account: None,
            last_profile_id: None,
        };
        tracing::info!(server_id = %p.server_id, untested = p.untested, "server saved");
        self.update_registry(|reg| {
            let saved = reg.upsert(record).clone();
            Self::view(&saved, reg.active.as_deref())
        })
        .await
    }

    pub async fn remove(&self, server_id: &str) -> CmdResult<()> {
        if let Ok(session) = self.session(server_id).await {
            session.logout().await;
        }
        self.sessions.lock().await.remove(server_id);
        let removed = self.update_registry(|reg| reg.remove(server_id)).await?;
        if let Some(acc) = removed.and_then(|r| r.account) {
            self.store
                .delete(&granary_silo_api::credentials::account_key(server_id, &acc.user_id))?;
        }
        Ok(())
    }

    pub async fn activate(&self, server_id: &str) -> CmdResult<()> {
        self.update_registry(|reg| {
            if reg.get(server_id).is_none() {
                return Err(ApiError::UnknownServer(server_id.to_owned()));
            }
            reg.active = Some(server_id.to_owned());
            Ok(())
        })
        .await?
        .map_err(Into::into)
    }

    fn record(reg: &Registry, server_id: &str) -> Result<ServerRecord, ApiError> {
        reg.get(server_id)
            .cloned()
            .ok_or_else(|| ApiError::UnknownServer(server_id.to_owned()))
    }

    pub async fn login(&self, server_id: &str, username: &str, password: Secret) -> CmdResult<LoginView> {
        let record = Self::record(&*self.registry.lock().await, server_id)?;
        if record.account.is_some() {
            // One account per server: end the previous session first so its credential does not linger (M2).
            self.sign_out(server_id).await?;
        }
        let (session, user) = Session::login(
            self.api.clone(),
            self.store.clone(),
            &record.origin,
            server_id,
            username.trim(),
            &password,
        )
        .await?;
        let account = session.account().clone();
        self.sessions
            .lock()
            .await
            .insert(server_id.to_owned(), Arc::new(session));
        self.update_registry(|reg| {
            if let Some(r) = reg.get_mut(server_id) {
                r.account = Some(account.clone());
                r.last_profile_id = None;
            }
            reg.active = Some(server_id.to_owned());
        })
        .await?;
        tracing::info!(server_id, user_id = %account.user_id, "signed in");
        Ok(LoginView {
            account: AccountView {
                user_id: account.user_id,
                username: account.username,
            },
            password_change_required: user.password_change_required,
        })
    }

    pub async fn logout(&self, server_id: &str) -> CmdResult<()> {
        self.sign_out(server_id).await
    }

    /// Revoke the server session (best effort, at the origin it was issued for), delete the stored credential and
    /// clear the account.
    async fn sign_out(&self, server_id: &str) -> CmdResult<()> {
        if let Ok(session) = self.session(server_id).await {
            session.logout().await;
        }
        let account = self
            .registry
            .lock()
            .await
            .get(server_id)
            .and_then(|r| r.account.clone());
        if let Some(acc) = account {
            self.store
                .delete(&granary_silo_api::credentials::account_key(server_id, &acc.user_id))?;
        }
        self.forget_account(server_id).await
    }

    /// The live session for a server, resuming it from the credential store after a restart.
    async fn session(&self, server_id: &str) -> Result<Arc<Session>, ApiError> {
        if let Some(s) = self.sessions.lock().await.get(server_id) {
            return Ok(s.clone());
        }
        let record = Self::record(&*self.registry.lock().await, server_id)?;
        let account = record.account.ok_or(ApiError::NotSignedIn)?;
        let session = Session::resume(self.api.clone(), self.store.clone(), &record.origin, server_id, account)?
            .ok_or(ApiError::NotSignedIn)?;
        let session = Arc::new(session);
        self.sessions.lock().await.insert(server_id.to_owned(), session.clone());
        Ok(session)
    }

    async fn forget_account(&self, server_id: &str) -> CmdResult<()> {
        self.sessions.lock().await.remove(server_id);
        self.update_registry(|reg| {
            if let Some(r) = reg.get_mut(server_id) {
                r.account = None;
                r.last_profile_id = None;
            }
        })
        .await
    }

    /// Map session-level outcomes: a revoked or missing credential clears the account so the UI shows sign-in.
    async fn settle<T>(&self, server_id: &str, r: Result<T, ApiError>) -> CmdResult<T> {
        match r {
            Err(e @ (ApiError::SignedOut | ApiError::NotSignedIn)) => {
                tracing::warn!(server_id, code = e.code(), "session ended");
                self.forget_account(server_id).await?;
                Err(e.into())
            }
            other => other.map_err(Into::into),
        }
    }

    pub async fn profiles(&self, server_id: &str) -> CmdResult<Vec<ProfileView>> {
        let r = match self.session(server_id).await {
            Ok(s) => s.profiles().await,
            Err(e) => Err(e),
        };
        let profiles = self.settle(server_id, r).await?;
        Ok(profiles
            .into_iter()
            .map(|p| ProfileView {
                id: p.id,
                name: p.name,
                has_pin: p.has_pin,
                is_child: p.is_child,
            })
            .collect())
    }

    pub async fn select_profile(&self, server_id: &str, profile_id: &str, pin: Option<Secret>) -> CmdResult<()> {
        let r = match self.session(server_id).await {
            Ok(s) => s.select_profile(profile_id, pin.as_ref()).await,
            Err(e) => Err(e),
        };
        self.settle(server_id, r).await?;
        self.update_registry(|reg| {
            if let Some(r) = reg.get_mut(server_id) {
                r.last_profile_id = Some(profile_id.to_owned());
            }
        })
        .await
    }

    /// The profile selected in this run, if any (the PIN of a locked profile is asked again after a restart).
    pub async fn selected_profile(&self, server_id: &str) -> Option<ProfileAuth> {
        self.sessions.lock().await.get(server_id).and_then(|s| s.profile())
    }

    pub async fn libraries(&self, server_id: &str) -> CmdResult<Vec<LibraryView>> {
        let r = match self.session(server_id).await {
            Ok(s) => s.libraries().await,
            Err(e) => Err(e),
        };
        let mut libs = self.settle(server_id, r).await?;
        libs.sort_by_key(|l| l.sort_order);
        Ok(libs
            .into_iter()
            .map(|l| LibraryView {
                id: l.id,
                name: l.name,
                kind: l.kind,
            })
            .collect())
    }
}

fn emit_if_revoked<T>(app: &AppHandle, server_id: &str, r: &CmdResult<T>) {
    if let Err(e) = r
        && e.code == "signed_out"
        && let Err(err) = app.emit(
            EVENT_REVOKED,
            RevokedEvent {
                server_id: server_id.to_owned(),
            },
        )
    {
        tracing::warn!(error = %err, "could not emit auth://revoked");
    }
}

#[tauri::command]
pub async fn servers_list(state: State<'_, SiloState>) -> CmdResult<Vec<ServerView>> {
    Ok(state.servers().await)
}

#[tauri::command]
pub async fn servers_probe(state: State<'_, SiloState>, url: String) -> CmdResult<ProbeView> {
    state.probe(&url).await
}

#[tauri::command]
pub async fn servers_add(state: State<'_, SiloState>, url: String) -> CmdResult<ServerView> {
    state.add(&url).await
}

#[tauri::command]
pub async fn servers_remove(state: State<'_, SiloState>, server_id: String) -> CmdResult<()> {
    state.remove(&server_id).await
}

#[tauri::command]
pub async fn servers_activate(state: State<'_, SiloState>, server_id: String) -> CmdResult<()> {
    state.activate(&server_id).await
}

#[tauri::command]
pub async fn auth_login(
    state: State<'_, SiloState>,
    server_id: String,
    username: String,
    password: String,
) -> CmdResult<LoginView> {
    state.login(&server_id, &username, Secret::new(password)).await
}

#[tauri::command]
pub async fn auth_logout(state: State<'_, SiloState>, server_id: String) -> CmdResult<()> {
    state.logout(&server_id).await
}

#[tauri::command]
pub async fn profiles_list(
    app: AppHandle,
    state: State<'_, SiloState>,
    server_id: String,
) -> CmdResult<Vec<ProfileView>> {
    let r = state.profiles(&server_id).await;
    emit_if_revoked(&app, &server_id, &r);
    r
}

#[tauri::command]
pub async fn profiles_select(
    app: AppHandle,
    state: State<'_, SiloState>,
    server_id: String,
    profile_id: String,
    pin: Option<String>,
) -> CmdResult<()> {
    let r = state
        .select_profile(&server_id, &profile_id, pin.map(Secret::new))
        .await;
    emit_if_revoked(&app, &server_id, &r);
    r
}

#[tauri::command]
pub async fn libraries_list(
    app: AppHandle,
    state: State<'_, SiloState>,
    server_id: String,
) -> CmdResult<Vec<LibraryView>> {
    let r = state.libraries(&server_id).await;
    emit_if_revoked(&app, &server_id, &r);
    r
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use granary_silo_api::credentials::MemoryStore;
    use granary_silo_api::mock::MockSiloApi;

    fn temp_registry() -> PathBuf {
        std::env::temp_dir()
            .join(format!("granary-silo-{}", std::process::id()))
            .join(format!("{:?}", std::thread::current().id()).replace(['(', ')'], ""))
            .join("servers.json")
    }

    fn state() -> (Arc<MockSiloApi>, Arc<MemoryStore>, SiloState, PathBuf) {
        let api = Arc::new(MockSiloApi::demo());
        let store = Arc::new(MemoryStore::default());
        let path = temp_registry();
        let _ = std::fs::remove_file(&path);
        let s = SiloState::new(api.clone(), store.clone(), path.clone());
        (api, store, s, path)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn add_login_profile_libraries_logout() -> Result<(), CommandError> {
        let (api, store, s, path) = state();
        let probe = s.probe("http://mock.example:8096").await?;
        assert!(probe.insecure && !probe.known);
        let server = s.add("http://mock.example:8096/").await?;
        assert!(server.active && server.account.is_none());
        assert!(s.probe("http://mock.example:8096").await?.known);
        let id = server.server_id;

        let err = s.login(&id, "alice", Secret::new("wrong")).await.unwrap_err();
        assert_eq!(err.code, "invalid_credentials");
        let login = s.login(&id, " alice ", Secret::new("pw")).await?;
        assert_eq!(login.account.username, "alice");
        assert_eq!(store.len(), 1);

        let profiles = s.profiles(&id).await?;
        assert_eq!(profiles.len(), 2);
        let err = s.select_profile(&id, "2", Some(Secret::new("9999"))).await.unwrap_err();
        assert_eq!(err.code, "wrong_pin");
        s.select_profile(&id, "2", Some(Secret::new("1234"))).await?;
        let libs = s.libraries(&id).await?;
        assert_eq!(libs.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Shows"]);

        // Registry persisted: a fresh state (app restart) resumes from the credential store.
        let s2 = SiloState::new(api.clone(), store.clone(), path.clone());
        let servers = s2.servers().await;
        assert_eq!(servers[0].last_profile_id.as_deref(), Some("2"));
        assert!(s2.selected_profile(&id).await.is_none(), "PIN profile is not restored");
        s2.select_profile(&id, "1", None).await?;
        assert_eq!(s2.libraries(&id).await?.len(), 2);

        s2.logout(&id).await?;
        assert!(store.is_empty());
        assert!(s2.servers().await[0].account.is_none());
        assert_eq!(s2.libraries(&id).await.unwrap_err().code, "not_signed_in");
        s2.remove(&id).await?;
        assert!(s2.servers().await.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn new_address_signs_out_and_forgets_credential() -> Result<(), CommandError> {
        let (api, store, s, path) = state();
        let id = s.add("https://mock.example").await?.server_id;
        s.login(&id, "alice", Secret::new("pw")).await?;
        assert_eq!(store.len(), 1);
        // Same server id reported at another address (look-alike server or plain-http LAN address).
        let moved = s.add("http://192.0.2.10:8096").await?;
        assert_eq!(moved.server_id, id);
        assert!(moved.account.is_none(), "account must not follow the server id");
        assert!(store.is_empty(), "refresh token deleted");
        assert_eq!(api.live_sessions(), 0, "old session revoked");
        assert_eq!(s.libraries(&id).await.unwrap_err().code, "not_signed_in");
        // Re-adding at the same address keeps the account.
        s.login(&id, "alice", Secret::new("pw")).await?;
        assert!(s.add("http://192.0.2.10:8096/").await?.account.is_some());
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn signing_in_again_ends_previous_session() -> Result<(), CommandError> {
        let (api, store, s, path) = state();
        let id = s.add("https://mock.example").await?.server_id;
        s.login(&id, "alice", Secret::new("pw")).await?;
        s.login(&id, "alice", Secret::new("pw")).await?;
        assert_eq!(store.len(), 1);
        assert_eq!(api.live_sessions(), 1, "previous session revoked");
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn revoked_session_clears_account() -> Result<(), CommandError> {
        let (api, store, s, path) = state();
        let id = s.add("https://mock.example").await?.server_id;
        s.login(&id, "alice", Secret::new("pw")).await?;
        api.revoke_all_sessions();
        let err = s.libraries(&id).await.unwrap_err();
        assert_eq!(err.code, "signed_out");
        assert!(store.is_empty());
        assert!(s.servers().await[0].account.is_none());
        let _ = std::fs::remove_dir_all(path.parent().unwrap_or(&path));
        Ok(())
    }

    #[test]
    fn command_errors_serialise_camel_case() {
        let e = CommandError::from(ApiError::Http {
            status: 503,
            problem: None,
            retry_after: Some(7),
        });
        let v = serde_json::to_value(e).unwrap_or_default();
        assert_eq!(v["code"], "unavailable");
        assert_eq!(v["retryAfter"], 7);
        assert_eq!(v["status"], 503);
    }
}
