//! Events socket (silo-api.md §9, `realtime-api.md@6791aac`): a single-use ticket from `POST /api/v2/events/ws-ticket`,
//! then `GET /api/v2/events/ws` offering the subprotocols `silo.events.v2` and `silo.ticket.<ticket>` (the ticket never
//! goes into the URL). Connections last at most 5 minutes; the caller reconnects with a new ticket.
use crate::{ApiError, Result, Secret};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{self, Message};
use tokio_tungstenite::{Connector, MaybeTlsStream, WebSocketStream};

pub const PROTOCOL: &str = "silo.events.v2";
/// Viewer channels Granary uses (`types.go` `ClientChannels`): watch-state changes and catalog metadata updates.
pub const CHANNELS: &[&str] = &["user_state", "catalog"];
/// Close code after `access_changed` (`realtime-api.md` L257-305).
pub const CLOSE_ACCESS_CHANGED: u16 = 4001;

/// An `event` envelope: `{type:"event", channel, event, event_id, timestamp, data}` (`internal/events/types.go`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerEvent {
    pub channel: String,
    pub event: String,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Hello {
        available_channels: Vec<String>,
    },
    Subscribed {
        channels: Vec<String>,
        rejected: Vec<String>,
    },
    Event(ServerEvent),
    /// Access changed: refetch everything and reconnect with a new ticket.
    AccessChanged,
    Error(String),
    /// A frame type Granary does not use (`snapshot`, future additions).
    Other(String),
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    available_channels: Vec<String>,
    #[serde(default)]
    channels: Vec<String>,
    #[serde(default)]
    rejected: Vec<Rejected>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

#[derive(Deserialize)]
struct Rejected {
    channel: String,
}

/// Parse one text frame; `None` for something that is not a JSON frame.
pub fn parse_frame(text: &str) -> Option<Frame> {
    let env: Envelope = serde_json::from_str(text).ok()?;
    Some(match env.kind.as_str() {
        "hello" => Frame::Hello {
            available_channels: env.available_channels,
        },
        "subscribed" => Frame::Subscribed {
            channels: env.channels,
            rejected: env.rejected.into_iter().map(|r| r.channel).collect(),
        },
        "event" => Frame::Event(serde_json::from_str(text).ok()?),
        "access_changed" => Frame::AccessChanged,
        "error" => Frame::Error(env.code.or(env.message).unwrap_or_default()),
        other => Frame::Other(other.to_owned()),
    })
}

/// `wss://host/api/v2/events/ws` for an `https` origin (`ws://` for `http`).
pub fn socket_url(origin: &str) -> Result<String> {
    let url = url::Url::parse(origin).map_err(|e| ApiError::InvalidUrl(e.to_string()))?;
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" => "ws",
        s => return Err(ApiError::InvalidUrl(format!("scheme {s}"))),
    };
    let host = url.host_str().ok_or_else(|| ApiError::InvalidUrl("no host".into()))?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    Ok(format!("{scheme}://{host}{port}/api/v2/events/ws"))
}

pub fn subscribe_frame(request_id: &str) -> String {
    serde_json::json!({"type": "subscribe", "request_id": request_id, "channels": CHANNELS}).to_string()
}

fn tls_connector() -> Result<Connector> {
    use rustls_platform_verifier::ConfigVerifierExt;
    // Same trust as the HTTP client: the Windows certificate store via the platform verifier (ADR-0004 L3).
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

pub struct EventsSocket {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl EventsSocket {
    /// Connect and subscribe to [`CHANNELS`].
    pub async fn connect(origin: &str, ticket: &Secret) -> Result<Self> {
        let url = socket_url(origin)?;
        let mut request = url.into_client_request().map_err(ws_error)?;
        let protocols = HeaderValue::from_str(&format!("{PROTOCOL}, silo.ticket.{}", ticket.expose()))
            .map_err(|_| ApiError::Internal("ticket is not a valid header value".into()))?;
        request.headers_mut().insert("sec-websocket-protocol", protocols);
        let connector = if origin.starts_with("https:") {
            Some(tls_connector()?)
        } else {
            None
        };
        let (mut ws, response) = tokio_tungstenite::connect_async_tls_with_config(request, None, true, connector)
            .await
            .map_err(ws_error)?;
        let chosen = response
            .headers()
            .get("sec-websocket-protocol")
            .and_then(|v| v.to_str().ok());
        if chosen != Some(PROTOCOL) {
            return Err(ApiError::Internal(format!("events socket chose protocol {chosen:?}")));
        }
        ws.send(Message::text(subscribe_frame(&crate::playback::mint_id())))
            .await
            .map_err(ws_error)?;
        Ok(Self { ws })
    }

    /// The next frame; `None` once the socket closed (normally after at most 5 minutes). Pings are answered by the
    /// library while reading.
    pub async fn next(&mut self) -> Option<Result<Frame>> {
        loop {
            match self.ws.next().await? {
                Ok(Message::Text(t)) => {
                    if let Some(f) = parse_frame(t.as_str()) {
                        return Some(Ok(f));
                    }
                }
                Ok(Message::Close(Some(CloseFrame { code, .. }))) if u16::from(code) == CLOSE_ACCESS_CHANGED => {
                    return Some(Ok(Frame::AccessChanged));
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
    fn tls_connector_builds() {
        assert!(tls_connector().is_ok());
    }

    #[test]
    fn frames_and_urls() {
        assert_eq!(
            socket_url("https://media.example.test").unwrap(),
            "wss://media.example.test/api/v2/events/ws"
        );
        assert_eq!(
            socket_url("http://10.0.0.2:8080").unwrap(),
            "ws://10.0.0.2:8080/api/v2/events/ws"
        );
        assert!(socket_url("ftp://x").is_err());
        let ev = parse_frame(
            r#"{"type":"event","channel":"user_state","event":"user_state.changed","event_id":"e1","timestamp":"t",
                "data":{"profile_id":"1","content_id":"m1","change":"progress"}}"#,
        )
        .unwrap();
        let Frame::Event(e) = ev else { panic!("{ev:?}") };
        assert_eq!(
            (e.channel.as_str(), e.event.as_str()),
            ("user_state", "user_state.changed")
        );
        assert_eq!(e.data["change"], "progress");
        assert_eq!(parse_frame(r#"{"type":"access_changed"}"#), Some(Frame::AccessChanged));
        assert_eq!(
            parse_frame(
                r#"{"type":"subscribed","channels":["catalog"],"rejected":[{"channel":"user_state","code":"x"}]}"#
            ),
            Some(Frame::Subscribed {
                channels: vec!["catalog".into()],
                rejected: vec!["user_state".into()]
            })
        );
        assert_eq!(parse_frame("not json"), None);
        let sub: serde_json::Value = serde_json::from_str(&subscribe_frame("r1")).unwrap();
        assert_eq!(sub["channels"], serde_json::json!(["user_state", "catalog"]));
    }

    #[tokio::test]
    async fn connects_with_the_ticket_subprotocol_and_reads_events() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut offered = String::new();
            let mut ws = tokio_tungstenite::accept_hdr_async(tcp, |req: &Request, mut resp: Response| {
                assert_eq!(req.uri().path(), "/api/v2/events/ws");
                assert!(req.uri().query().is_none(), "no token in the URL");
                offered = req.headers()["sec-websocket-protocol"].to_str().unwrap().to_owned();
                resp.headers_mut()
                    .insert("sec-websocket-protocol", HeaderValue::from_static(PROTOCOL));
                Ok(resp)
            })
            .await
            .unwrap();
            let sub = ws.next().await.unwrap().unwrap();
            let sub: serde_json::Value = serde_json::from_str(sub.to_text().unwrap()).unwrap();
            assert_eq!(sub["type"], "subscribe");
            ws.send(Message::text(
                r#"{"type":"subscribed","channels":["user_state","catalog"]}"#,
            ))
            .await
            .unwrap();
            ws.send(Message::text(
                r#"{"type":"event","channel":"catalog","event":"catalog.item.changed","data":{"change":"metadata_updated"}}"#,
            ))
            .await
            .unwrap();
            ws.send(Message::Close(Some(CloseFrame {
                code: CLOSE_ACCESS_CHANGED.into(),
                reason: "access_changed".into(),
            })))
            .await
            .unwrap();
            offered
        });
        let mut s = EventsSocket::connect(&format!("http://127.0.0.1:{port}"), &Secret::new("tk-1"))
            .await
            .unwrap();
        assert!(matches!(s.next().await, Some(Ok(Frame::Subscribed { .. }))));
        let Some(Ok(Frame::Event(e))) = s.next().await else {
            panic!("no event")
        };
        assert_eq!(e.event, "catalog.item.changed");
        assert_eq!(s.next().await.unwrap().unwrap(), Frame::AccessChanged);
        assert_eq!(server.await.unwrap(), "silo.events.v2, silo.ticket.tk-1");
    }
}
