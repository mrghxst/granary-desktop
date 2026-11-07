//! Loopback media relay (ADR-0008). mpv never sees a credential: every server media URL in a playback plan is
//! registered here under a random 128-bit route token, mpv opens `http://127.0.0.1:<port>/r/<token>/<name>`, and the
//! relay fetches the real URL through the account's [`Session`] (bearer and profile headers, refresh on 401).
//!
//! - Bound to 127.0.0.1 on an ephemeral port; the `Host` header must name exactly that address (DNS rebinding).
//! - Only GET and HEAD; only the range/conditional headers in [`FORWARDED_REQUEST_HEADERS`] go upstream and only
//!   [`FORWARDED_RESPONSE_HEADERS`] come back. Bodies are streamed.
//! - A path below a route (`/r/<token>/<rest>`) resolves relative to the registered URL's directory, so HLS
//!   manifests with relative segment names work. `..`, backslashes, encoded slashes and empty segments are refused,
//!   and the resolved URL must stay inside that directory on the same origin.
//! - Unknown tokens answer 404. Routes are dropped when playback stops.
use bytes::Bytes;
use futures_util::TryStreamExt;
use granary_silo_api::media::{FORWARDED_REQUEST_HEADERS, MediaResponse};
use granary_silo_api::{ApiError, Session};
use http_body_util::{BodyExt, Empty, StreamBody, combinators::UnsyncBoxBody};
use hyper::body::{Frame, Incoming};
use hyper::{Method, Request, Response, StatusCode};
use std::collections::HashMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

pub type FetchFuture = Pin<Box<dyn Future<Output = Result<MediaResponse, ApiError>> + Send>>;

/// Where relayed bytes come from: the signed-in [`Session`] in the app, a fake in tests.
pub trait MediaSource: Send + Sync {
    fn fetch(&self, url: String, head: bool, headers: Vec<(String, String)>) -> FetchFuture;
}

impl MediaSource for Arc<Session> {
    fn fetch(&self, url: String, head: bool, headers: Vec<(String, String)>) -> FetchFuture {
        let s = Arc::clone(self);
        Box::pin(async move { s.fetch_media(&url, head, &headers).await })
    }
}

#[derive(Clone)]
struct Route {
    /// The registered absolute URL (served as-is for its own name).
    target: url::Url,
    /// The last path segment of `target` (what the local URL ends with).
    name: String,
    source: Arc<dyn MediaSource>,
}

type Routes = Arc<Mutex<HashMap<String, Route>>>;

pub struct Relay {
    addr: SocketAddr,
    routes: Routes,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn lock(routes: &Routes) -> std::sync::MutexGuard<'_, HashMap<String, Route>> {
    match routes.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Relay {
    /// Bind 127.0.0.1:0 and serve on the current Tokio runtime.
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let routes: Routes = Arc::default();
        let r = Arc::clone(&routes);
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let routes = Arc::clone(&r);
                tokio::spawn(async move {
                    let service = hyper::service::service_fn(move |req| handle(Arc::clone(&routes), addr, req));
                    let io = hyper_util::rt::TokioIo::new(stream);
                    // mpv closes connections mid-body on every seek; that is not worth a log line.
                    let _ = hyper::server::conn::http1::Builder::new()
                        .keep_alive(true)
                        .serve_connection(io, service)
                        .await;
                });
            }
        });
        tracing::info!(port = addr.port(), "media relay listening on loopback");
        Ok(Self { addr, routes, task })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Register an absolute server URL and return the loopback URL mpv should open.
    pub fn register(&self, source: Arc<dyn MediaSource>, target: &str) -> Result<String, ApiError> {
        let target = url::Url::parse(target).map_err(|_| ApiError::Internal("relay target is not a URL".into()))?;
        let name = target
            .path_segments()
            .and_then(|mut s| s.next_back())
            .filter(|n| !n.is_empty())
            .unwrap_or("media")
            .to_owned();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let local = format!("http://{}/r/{token}/{name}", self.addr);
        lock(&self.routes).insert(token, Route { target, name, source });
        Ok(local)
    }

    /// Forget every route (playback stopped): later requests for them answer 404.
    pub fn clear(&self) {
        lock(&self.routes).clear();
    }

    pub fn route_count(&self) -> usize {
        lock(&self.routes).len()
    }
}

type Body = UnsyncBoxBody<Bytes, std::io::Error>;

fn empty(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Empty::<Bytes>::new().map_err(|never| match never {}).boxed_unsync());
    *r.status_mut() = status;
    r
}

/// Resolve `/r/<token>/<rest>?<query>` against the route. `None` → 404 (unknown token or a path that leaves the
/// route's directory).
fn resolve(route: &Route, rest: &str, query: Option<&str>) -> Option<url::Url> {
    if rest == route.name && query.is_none() {
        return Some(route.target.clone());
    }
    let safe = !rest.is_empty()
        && rest.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
        && !rest.contains('\\')
        && !rest.to_ascii_lowercase().contains("%2f")
        && !rest.to_ascii_lowercase().contains("%5c")
        && !rest.contains("%2e%2e")
        && !rest.contains(':');
    if !safe {
        return None;
    }
    let mut base = route.target.clone();
    base.set_query(None);
    let mut url = base.join(rest).ok()?;
    url.set_query(query);
    let dir = &base.path()[..=base.path().rfind('/')?];
    (url.origin() == base.origin() && url.path().starts_with(dir)).then_some(url)
}

async fn handle(
    routes: Routes,
    addr: SocketAddr,
    req: Request<Incoming>,
) -> Result<Response<Body>, std::convert::Infallible> {
    let host_ok = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| h == addr.to_string());
    if !host_ok {
        return Ok(empty(StatusCode::FORBIDDEN));
    }
    let head = match *req.method() {
        Method::GET => false,
        Method::HEAD => true,
        _ => return Ok(empty(StatusCode::METHOD_NOT_ALLOWED)),
    };
    let path = req.uri().path();
    let Some((token, rest)) = path.strip_prefix("/r/").and_then(|p| p.split_once('/')) else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let Some(route) = lock(&routes).get(token).cloned() else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let Some(target) = resolve(&route, rest, req.uri().query()) else {
        return Ok(empty(StatusCode::NOT_FOUND));
    };
    let headers: Vec<(String, String)> = req
        .headers()
        .iter()
        .filter(|(k, _)| FORWARDED_REQUEST_HEADERS.contains(&k.as_str()))
        .filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned())))
        .collect();
    match route.source.fetch(target.to_string(), head, headers).await {
        Ok(media) => Ok(respond(media)),
        Err(e) => {
            let status = match &e {
                ApiError::Http { status, .. } => StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY),
                _ => StatusCode::BAD_GATEWAY,
            };
            tracing::warn!(
                code = e.code(),
                status = status.as_u16(),
                "relayed media request failed"
            );
            Ok(empty(status))
        }
    }
}

fn respond(media: MediaResponse) -> Response<Body> {
    let mut builder = Response::builder().status(media.status);
    for (k, v) in &media.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let body = StreamBody::new(media.body.map_ok(Frame::data)).boxed_unsync();
    builder.body(body).unwrap_or_else(|_| empty(StatusCode::BAD_GATEWAY))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    type Seen = (String, bool, Vec<(String, String)>);

    /// Serves `bytes` for every URL and records what was asked.
    #[derive(Default)]
    struct Fake {
        seen: Mutex<Vec<Seen>>,
    }

    impl MediaSource for Arc<Fake> {
        fn fetch(&self, url: String, head: bool, headers: Vec<(String, String)>) -> FetchFuture {
            self.seen.lock().unwrap().push((url.clone(), head, headers));
            Box::pin(async move {
                if url.contains("gone") {
                    return Err(ApiError::Http {
                        status: 410,
                        problem: None,
                        retry_after: None,
                    });
                }
                let chunks: Vec<std::io::Result<Bytes>> =
                    vec![Ok(Bytes::from_static(b"hello ")), Ok(Bytes::from_static(b"relay"))];
                Ok(MediaResponse {
                    status: 206,
                    headers: vec![
                        ("content-type".into(), "video/x-matroska".into()),
                        ("content-range".into(), "bytes 0-10/100".into()),
                        ("content-length".into(), "11".into()),
                    ],
                    body: Box::pin(futures_util::stream::iter(chunks)),
                })
            })
        }
    }

    async fn raw(port: u16, request: String) -> String {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        s.write_all(request.as_bytes()).await.unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    fn get(port: u16, path: &str, extra: &str) -> String {
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n{extra}\r\n")
    }

    #[tokio::test]
    async fn relays_with_route_tokens_and_only_safe_headers() {
        let relay = Relay::start().await.unwrap();
        let fake = Arc::new(Fake::default());
        let local = relay
            .register(
                Arc::new(fake.clone()),
                "https://media.example.test/api/v2/stream/ps-1?st=secret",
            )
            .unwrap();
        assert!(local.starts_with(&format!("http://127.0.0.1:{}/r/", relay.port())));
        assert!(local.ends_with("/ps-1") && !local.contains("secret"));
        let path = local.split_once(&relay.port().to_string()).unwrap().1.to_owned();
        let port = relay.port();

        let out = raw(
            port,
            get(
                port,
                &path,
                "Range: bytes=0-10\r\nCookie: a=b\r\nAuthorization: Bearer x\r\n",
            ),
        )
        .await;
        assert!(out.starts_with("HTTP/1.1 206"), "{out}");
        assert!(
            out.contains("content-range: bytes 0-10/100") && out.ends_with("hello relay"),
            "{out}"
        );
        let seen = fake.seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "https://media.example.test/api/v2/stream/ps-1?st=secret");
        assert_eq!(seen[0].2, vec![("range".to_owned(), "bytes=0-10".to_owned())]);

        // HEAD, a sibling path (HLS segment) with its own query, and the escapes that must not work.
        let head = format!("HEAD {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
        assert!(raw(port, head).await.starts_with("HTTP/1.1 206"));
        assert!(fake.seen.lock().unwrap()[1].1);
        let token_dir = path.rsplit_once('/').unwrap().0;
        let seg = raw(port, get(port, &format!("{token_dir}/seg/1.ts?x=1"), "")).await;
        assert!(seg.starts_with("HTTP/1.1 206"), "{seg}");
        assert_eq!(
            fake.seen.lock().unwrap()[2].0,
            "https://media.example.test/api/v2/stream/seg/1.ts?x=1"
        );
        for bad in ["../../v2/users", "..%2fx", "a//b", "a\\b", "%2e%2e/x", "http:x"] {
            let out = raw(port, get(port, &format!("{token_dir}/{bad}"), "")).await;
            assert!(out.starts_with("HTTP/1.1 404"), "{bad}: {out}");
        }
        assert_eq!(fake.seen.lock().unwrap().len(), 3);

        // Wrong Host (DNS rebinding), unknown token, other methods, upstream errors.
        let rebinding = format!("GET {path} HTTP/1.1\r\nHost: evil.test\r\nConnection: close\r\n\r\n");
        assert!(raw(port, rebinding).await.starts_with("HTTP/1.1 403"));
        assert!(
            raw(port, get(port, "/r/0000/ps-1", ""))
                .await
                .starts_with("HTTP/1.1 404")
        );
        let post =
            format!("POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
        assert!(raw(port, post).await.starts_with("HTTP/1.1 405"));
        let gone = relay
            .register(Arc::new(fake.clone()), "https://media.example.test/gone/x")
            .unwrap();
        let gone_path = gone.split_once(&port.to_string()).unwrap().1.to_owned();
        assert!(raw(port, get(port, &gone_path, "")).await.starts_with("HTTP/1.1 410"));

        relay.clear();
        assert_eq!(relay.route_count(), 0);
        assert!(raw(port, get(port, &path, "")).await.starts_with("HTTP/1.1 404"));
    }
}
