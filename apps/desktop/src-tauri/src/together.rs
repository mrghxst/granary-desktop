//! Watch Together (silo-api.md §4.1i): the room operations and the room socket. The room proof stays here; the page
//! gets the snapshot and every socket frame as `together://event` and sends frames back through `together_send`.
//! The socket closes after at most 5 minutes and is reopened with a fresh ticket; a closed room, a refused ticket or
//! this profile joining on another device ends the link.
use crate::playback::events::backoff;
use crate::silo::{CmdResult, CommandError, SiloState};
use granary_silo_api::together::{RoomGrant, RoomSelection, TogetherCapabilities, check_client_frame, frame_type};
use granary_silo_api::{ApiError, Secret, Session};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, mpsc};

pub const EVENT_TOGETHER: &str = "together://event";

/// What the page hears about the room.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TogetherEvent {
    /// A frame from the room socket (`snapshot`, `transport_command`, `pong`, ...).
    Frame { frame: serde_json::Value },
    /// `connecting`, `connected` or `closed` (with why: `room_closed`, `connection_replaced`, `forbidden`, `left`).
    Status { state: String, reason: Option<String> },
}

struct Link {
    server_id: String,
    room_id: String,
    host: bool,
    out: mpsc::UnboundedSender<String>,
    task: tauri::async_runtime::JoinHandle<()>,
}

#[derive(Default)]
pub struct TogetherState {
    link: Mutex<Option<Link>>,
}

/// The room as the page sees it.
#[derive(Debug, Clone, Serialize)]
pub struct RoomView {
    pub room: serde_json::Value,
}

fn emit(app: &AppHandle, event: TogetherEvent) {
    let _ = app.emit(EVENT_TOGETHER, &event);
}

fn status(app: &AppHandle, state: &str, reason: Option<&str>) {
    emit(
        app,
        TogetherEvent::Status {
            state: state.into(),
            reason: reason.map(str::to_owned),
        },
    );
}

/// A ticket refusal that will not heal by retrying (the web client stops on these too).
fn terminal(e: &ApiError) -> Option<&'static str> {
    match e {
        ApiError::Http { status: 409, .. } => Some("room_closed"),
        ApiError::Http { status: 404, .. } => Some("not_found"),
        ApiError::Http {
            status: 401 | 403 | 422,
            ..
        } => Some("forbidden"),
        ApiError::SignedOut | ApiError::NotSignedIn | ApiError::ProfileRequired => Some("forbidden"),
        _ => None,
    }
}

async fn run(
    app: AppHandle,
    session: Arc<Session>,
    room_id: String,
    token: Secret,
    mut out: mpsc::UnboundedReceiver<String>,
) {
    let mut failures = 0u32;
    loop {
        status(&app, "connecting", None);
        match session.open_room(&room_id, &token).await {
            Ok(mut socket) => {
                tracing::info!("room socket connected");
                failures = 0;
                status(&app, "connected", None);
                loop {
                    tokio::select! {
                        frame = socket.next() => match frame {
                            Some(Ok(frame)) => {
                                let end = match frame_type(&frame) {
                                    Some("room_closed") => Some("room_closed"),
                                    Some("connection_replaced") => Some("connection_replaced"),
                                    _ => None,
                                };
                                emit(&app, TogetherEvent::Frame { frame });
                                if let Some(reason) = end {
                                    tracing::info!(reason, "room link ended");
                                    socket.close().await;
                                    status(&app, "closed", Some(reason));
                                    return;
                                }
                            }
                            Some(Err(e)) => {
                                tracing::debug!(code = e.code(), "room socket read failed");
                                break;
                            }
                            None => break,
                        },
                        text = out.recv() => match text {
                            Some(text) => {
                                if let Err(e) = socket.send(text).await {
                                    tracing::debug!(code = e.code(), "room socket write failed");
                                    break;
                                }
                            }
                            None => {
                                socket.close().await;
                                return;
                            }
                        },
                    }
                }
                tracing::debug!("room socket closed; reconnecting with a new ticket");
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(e) => {
                if let Some(reason) = terminal(&e) {
                    tracing::info!(code = e.code(), "room link refused");
                    status(&app, "closed", Some(reason));
                    return;
                }
                failures += 1;
                let wait = backoff(failures);
                tracing::warn!(code = e.code(), wait_s = wait.as_secs(), "room socket unavailable");
                tokio::time::sleep(wait).await;
            }
        }
    }
}

/// Replace the current link (if any) with one for `grant`.
async fn link(
    app: &AppHandle,
    state: &TogetherState,
    server_id: String,
    session: Arc<Session>,
    grant: RoomGrant,
) -> RoomView {
    let mut current = state.link.lock().await;
    if let Some(old) = current.take() {
        old.task.abort();
    }
    let (tx, rx) = mpsc::unbounded_channel();
    let room_id = grant.room_id().to_owned();
    let handle = app.app_handle().clone();
    let id = room_id.clone();
    let token = grant.room_access_token.clone();
    *current = Some(Link {
        server_id,
        room_id,
        host: grant.is_host(),
        out: tx,
        task: tauri::async_runtime::spawn(async move { run(handle, session, id, token, rx).await }),
    });
    RoomView { room: grant.room }
}

/// Whether Watch Together is offered on this server (`getWatchTogetherCapabilities`).
#[tauri::command]
pub async fn together_capabilities(silo: State<'_, SiloState>, server_id: String) -> CmdResult<TogetherCapabilities> {
    silo.with_session(&server_id, |s| async move { s.together_capabilities().await })
        .await
}

fn opaque_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && !id.chars().any(char::is_control)
}

/// Start a Watch Party for a title: a new host-pick room with it selected, then the room socket.
#[tauri::command]
pub async fn together_start(
    app: AppHandle,
    state: State<'_, TogetherState>,
    silo: State<'_, SiloState>,
    server_id: String,
    content_id: String,
    file_id: Option<String>,
) -> CmdResult<RoomView> {
    // The server checks the ids (positive integers as strings, `WatchTogetherSelectionInputBody`).
    if !opaque_id(&content_id) || file_id.as_deref().is_some_and(|f| !opaque_id(f)) {
        return Err(CommandError::invalid("watch together: ids".into()));
    }
    let session = silo.with_session(&server_id, |s| async move { Ok(s) }).await?;
    let selection = RoomSelection { content_id, file_id };
    let grant = silo
        .with_session(&server_id, |s| async move { s.together_start(&selection).await })
        .await?;
    Ok(link(&app, &state, server_id, session, grant).await)
}

/// Join a Watch Party by its code.
#[tauri::command]
pub async fn together_join(
    app: AppHandle,
    state: State<'_, TogetherState>,
    silo: State<'_, SiloState>,
    server_id: String,
    code: String,
) -> CmdResult<RoomView> {
    let code = code.trim().to_owned();
    if code.is_empty() || code.len() > 64 || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(CommandError::invalid("watch together: room code".into()));
    }
    let session = silo.with_session(&server_id, |s| async move { Ok(s) }).await?;
    let grant = silo
        .with_session(&server_id, |s| async move { s.together_join(&code).await })
        .await?;
    Ok(link(&app, &state, server_id, session, grant).await)
}

/// Send a frame on the room socket (dropped while it reconnects, as the web client does).
#[tauri::command]
pub async fn together_send(state: State<'_, TogetherState>, frame: serde_json::Value) -> CmdResult<()> {
    let text = check_client_frame(&frame).map_err(CommandError::invalid)?;
    let link = state.link.lock().await;
    let Some(link) = link.as_ref() else {
        return Err(CommandError::invalid("watch together: not in a room".into()));
    };
    let _ = link.out.send(text);
    Ok(())
}

/// Leave the room; the host closes it for everyone (`closeWatchTogetherRoom`). There is no leave frame: closing the
/// socket is leaving.
#[tauri::command]
pub async fn together_leave(
    app: AppHandle,
    state: State<'_, TogetherState>,
    silo: State<'_, SiloState>,
) -> CmdResult<()> {
    let Some(link) = state.link.lock().await.take() else {
        return Ok(());
    };
    link.task.abort();
    status(&app, "closed", Some("left"));
    if link.host {
        let room_id = link.room_id.clone();
        if let Err(e) = silo
            .with_session(&link.server_id, |s| async move { s.together_close(&room_id).await })
            .await
        {
            tracing::warn!(code = %e.code, "closing the room failed");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_terminal_refusals() {
        assert!(opaque_id("42") && opaque_id("9007199254740993"));
        assert!(!opaque_id("") && !opaque_id("a\nb"));
        let http = |status| ApiError::Http {
            status,
            problem: None,
            retry_after: None,
        };
        assert_eq!(terminal(&http(409)), Some("room_closed"));
        assert_eq!(terminal(&http(403)), Some("forbidden"));
        assert_eq!(terminal(&http(503)), None);
        assert_eq!(terminal(&ApiError::Unreachable("x".into())), None);
    }
}
