//! Watch Together (silo-api.md §4.1i, `silo-server/docs/realtime-api.md@749db96a`): rooms over HTTP, then the room
//! socket. A room proof (`room_access_token`) from create, join or select goes in `X-Room-Token` to mint a single-use
//! socket ticket; the socket offers `silo.room.v2` and `silo.ticket.<ticket>` and lasts at most 5 minutes.
//! Frames are JSON objects with a `type`; Granary relays them to the page, which runs the sync.
use crate::{ApiError, Result, Secret};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub const PROTOCOL: &str = "silo.room.v2";
/// Frames the page may send (`internal/api/handlers/watch_together.go@749db96a` `handleRoomClientMessage`).
pub const CLIENT_FRAMES: &[&str] = &[
    "attach_session",
    "transport_request",
    "state_report",
    "ready",
    "buffering",
    "lobby_ready",
    "ping",
];
/// A larger client frame closes the socket with 1009.
pub const MAX_CLIENT_FRAME: usize = 64 * 1024;

/// `WatchTogetherCapabilities` (`getWatchTogetherCapabilities`); members Granary does not use are dropped.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TogetherCapabilities {
    pub revision: String,
    /// `available`, `not_configured`, ...
    pub state: String,
    pub allowed: bool,
    pub connection_replaced: bool,
    pub socket_protocol: String,
}

/// `WatchTogetherRoomReadOutputBody`: the room snapshot (kept as JSON for the page) and the room proof.
#[derive(Debug, Clone, Deserialize)]
pub struct RoomGrant {
    pub room: serde_json::Value,
    /// Bound to the room, account and profile; a credential (never logged or handed to the page).
    pub room_access_token: Secret,
}

impl RoomGrant {
    pub fn room_id(&self) -> &str {
        self.room["room_id"].as_str().unwrap_or_default()
    }

    pub fn is_host(&self) -> bool {
        self.room["self_role"] == "host"
    }
}

/// `WatchTogetherSelectionInputBody`: ids are positive integers sent as strings.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RoomSelection {
    pub content_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

/// `wss://host/api/v2/watch-together/rooms/{room_id}/ws` for an `https` origin (`ws://` for `http`).
pub fn socket_url(origin: &str, room_id: &str) -> Result<String> {
    let base = crate::events::socket_url(origin)?;
    let root = base.trim_end_matches("/api/v2/events/ws");
    Ok(format!(
        "{root}/api/v2/watch-together/rooms/{}/ws",
        crate::api::url_segment(room_id)
    ))
}

/// The `type` of a JSON frame; `None` when it is not a JSON object with a string `type`.
pub fn frame_type(frame: &serde_json::Value) -> Option<&str> {
    frame.get("type")?.as_str()
}

/// A frame the page wants sent: an object of a known client type, within the size limit.
pub fn check_client_frame(frame: &serde_json::Value) -> std::result::Result<String, String> {
    let kind = frame_type(frame).ok_or("frame without a type")?;
    if !CLIENT_FRAMES.contains(&kind) {
        return Err(format!("frame type {kind} is not sent"));
    }
    let text = frame.to_string();
    if text.len() > MAX_CLIENT_FRAME {
        return Err("frame too large".into());
    }
    Ok(text)
}

fn tls_connector() -> Result<Connector> {
    use rustls_platform_verifier::ConfigVerifierExt;
    let config = rustls::ClientConfig::with_platform_verifier().map_err(|e| ApiError::Tls {
        kind: crate::TlsProblem::Other,
        message: e.to_string(),
    })?;
    Ok(Connector::Rustls(Arc::new(config)))
}

fn ws_error(e: tungstenite::Error) -> ApiError {
    match e {
        tungstenite::Error::Http(r) => ApiError::Http {
            status: r.status().as_u16(),
            problem: None,
            retry_after: None,
        },
        tungstenite::Error::Io(e) => ApiError::Unreachable(e.to_string()),
        other => ApiError::Unreachable(other.to_string()),
    }
}

pub struct RoomSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl RoomSocket {
    pub async fn connect(origin: &str, room_id: &str, ticket: &Secret) -> Result<Self> {
        let url = socket_url(origin, room_id)?;
        let mut request = url.into_client_request().map_err(ws_error)?;
        let protocols = HeaderValue::from_str(&format!("{PROTOCOL}, silo.ticket.{}", ticket.expose()))
            .map_err(|_| ApiError::Internal("ticket is not a valid header value".into()))?;
        request.headers_mut().insert("sec-websocket-protocol", protocols);
        let connector = if origin.starts_with("https:") {
            Some(tls_connector()?)
        } else {
            None
        };
        let (ws, response) = tokio_tungstenite::connect_async_tls_with_config(request, None, true, connector)
            .await
            .map_err(ws_error)?;
        let chosen = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok());
        if chosen != Some(PROTOCOL) {
            return Err(ApiError::Internal(format!("room socket chose protocol {chosen:?}")));
        }
        Ok(Self { ws })
    }

    pub async fn send(&mut self, text: String) -> Result<()> {
        self.ws.send(Message::text(text)).await.map_err(ws_error)
    }

    /// The next JSON frame; `None` once the socket closed. Protocol pings are answered by the library while reading.
    pub async fn next(&mut self) -> Option<Result<serde_json::Value>> {
        loop {
            match self.ws.next().await? {
                Ok(Message::Text(t)) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str())
                        && frame_type(&v).is_some()
                    {
                        return Some(Ok(v));
                    }
                }
                Ok(Message::Close(_)) => return None,
                Ok(_) => {}
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => return None,
                Err(e) => return Some(Err(ws_error(e))),
            }
        }
    }

    pub async fn close(mut self) {
        let _ = self.ws.close(None).await;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::result_large_err)] // test code (the handshake callback signature is fixed by tungstenite)
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    #[test]
    fn urls_and_frames() {
        assert_eq!(
            socket_url("https://media.example.test", "4f1c2d3e-0000-4000-8000-000000000001").unwrap(),
            "wss://media.example.test/api/v2/watch-together/rooms/4f1c2d3e-0000-4000-8000-000000000001/ws"
        );
        assert_eq!(
            socket_url("http://10.0.0.2:8080", "r1").unwrap(),
            "ws://10.0.0.2:8080/api/v2/watch-together/rooms/r1/ws"
        );
        let ok = serde_json::json!({"type": "state_report", "session_id": "ps-1", "position_seconds": 3.5, "is_paused": false});
        assert!(check_client_frame(&ok).is_ok());
        assert!(check_client_frame(&serde_json::json!({"type": "snapshot"})).is_err());
        assert!(check_client_frame(&serde_json::json!({"session_id": "x"})).is_err());
        let big = serde_json::json!({"type": "ping", "pad": "x".repeat(MAX_CLIENT_FRAME)});
        assert!(check_client_frame(&big).is_err());
    }

    #[tokio::test]
    async fn connects_with_the_ticket_subprotocol_and_relays_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut offered = String::new();
            let mut ws = tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, mut resp: Response| {
                assert_eq!(req.uri().path(), "/api/v2/watch-together/rooms/r1/ws");
                assert!(req.uri().query().is_none(), "no credential in the URL");
                offered = req.headers()["sec-websocket-protocol"].to_str().unwrap().to_owned();
                resp.headers_mut()
                    .insert("sec-websocket-protocol", HeaderValue::from_static(PROTOCOL));
                Ok(resp)
            })
            .await
            .unwrap();
            ws.send(Message::text(
                r#"{"type":"snapshot","room":{"room_id":"r1","phase":"playing"}}"#,
            ))
            .await
            .unwrap();
            let got = ws.next().await.unwrap().unwrap();
            let got: serde_json::Value = serde_json::from_str(got.to_text().unwrap()).unwrap();
            assert_eq!(got["type"], "attach_session");
            ws.send(Message::text("not json")).await.unwrap();
            ws.send(Message::text(
                r#"{"type":"transport_command","command":{"command_id":"c1","action":"play","position_seconds":12.5}}"#,
            ))
            .await
            .unwrap();
            ws.close(None).await.unwrap();
            offered
        });
        let mut s = RoomSocket::connect(&format!("http://127.0.0.1:{port}"), "r1", &Secret::new("tk-9"))
            .await
            .unwrap();
        let snap = s.next().await.unwrap().unwrap();
        assert_eq!(snap["room"]["phase"], "playing");
        s.send(r#"{"type":"attach_session","session_id":"ps-1"}"#.into())
            .await
            .unwrap();
        let cmd = s.next().await.unwrap().unwrap();
        assert_eq!(cmd["command"]["command_id"], "c1");
        assert!(s.next().await.is_none());
        assert_eq!(server.await.unwrap(), "silo.room.v2, silo.ticket.tk-9");
    }
}
