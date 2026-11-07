//! The events socket for the active server (silo-api.md §9): `user_state.changed` (watch progress, watched flags)
//! and `catalog.item.changed` are forwarded to the UI as `server://event`, which refetches what it shows. The
//! socket closes after at most 5 minutes; the loop reconnects with a new ticket, backing off on failures.
use crate::silo::{CmdResult, SiloState};
use granary_silo_api::events::Frame;
use granary_silo_api::{ApiError, Session};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

pub const EVENT_SERVER: &str = "server://event";

#[derive(Default)]
pub struct EventsState {
    task: Mutex<Option<(String, tauri::async_runtime::JoinHandle<()>)>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerEventView {
    pub server_id: String,
    /// `user_state.changed`, `catalog.item.changed`, ... or `access_changed`.
    pub event: String,
    pub channel: String,
    pub data: serde_json::Value,
}

/// Backoff after a failed connection: 2, 4, 8 ... 60 s.
pub fn backoff(failures: u32) -> Duration {
    Duration::from_secs((2u64 << failures.min(5)).min(60))
}

async fn run(app: AppHandle, server_id: String, session: Arc<Session>) {
    let mut failures = 0u32;
    loop {
        match session.open_events().await {
            Ok(mut socket) => {
                tracing::info!("events socket connected");
                failures = 0;
                while let Some(frame) = socket.next().await {
                    let view = match frame {
                        Ok(Frame::Event(e)) => ServerEventView {
                            server_id: server_id.clone(),
                            event: e.event,
                            channel: e.channel,
                            data: e.data,
                        },
                        Ok(Frame::AccessChanged) => ServerEventView {
                            server_id: server_id.clone(),
                            event: "access_changed".into(),
                            channel: String::new(),
                            data: serde_json::Value::Null,
                        },
                        Ok(Frame::Subscribed { rejected, .. }) if !rejected.is_empty() => {
                            tracing::warn!(?rejected, "events channels rejected");
                            continue;
                        }
                        Ok(_) => continue,
                        Err(e) => {
                            tracing::debug!(code = e.code(), "events socket read failed");
                            break;
                        }
                    };
                    let access_changed = view.event == "access_changed";
                    let _ = app.emit(EVENT_SERVER, &view);
                    if access_changed {
                        break;
                    }
                }
                tracing::debug!("events socket closed; reconnecting with a new ticket");
            }
            Err(e @ (ApiError::SignedOut | ApiError::NotSignedIn | ApiError::ProfileRequired)) => {
                tracing::info!(code = e.code(), "events socket stopped");
                return;
            }
            Err(e) => {
                failures += 1;
                let wait = backoff(failures);
                tracing::warn!(code = e.code(), wait_s = wait.as_secs(), "events socket unavailable");
                tokio::time::sleep(wait).await;
                continue;
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// (Re)connect the events socket for a server; a previous connection is closed.
#[tauri::command]
pub async fn events_connect(
    app: AppHandle,
    state: State<'_, EventsState>,
    silo: State<'_, SiloState>,
    server_id: String,
) -> CmdResult<()> {
    let session = silo.with_session(&server_id, |s| async move { Ok(s) }).await?;
    let mut task = state.task.lock().await;
    if let Some((_, t)) = task.take() {
        t.abort();
    }
    let handle = app.app_handle().clone();
    let id = server_id.clone();
    *task = Some((
        server_id,
        tauri::async_runtime::spawn(async move { run(handle, id, session).await }),
    ));
    Ok(())
}

#[tauri::command]
pub async fn events_disconnect(state: State<'_, EventsState>) -> CmdResult<()> {
    if let Some((_, t)) = state.task.lock().await.take() {
        t.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::backoff;
    use std::time::Duration;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff(1), Duration::from_secs(4));
        assert_eq!(backoff(3), Duration::from_secs(16));
        assert_eq!(backoff(9), Duration::from_secs(60));
    }
}
