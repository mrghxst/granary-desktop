//! Byte-range sources for the Matroska preview index ([`crate::mkv`]): HTTP/1.1 range requests to the app's loopback
//! relay (which adds the server credentials, ADR-0008) and local files. Plain `std` networking: the relay is always
//! `http://127.0.0.1:<port>/...`, so neither TLS nor a client library is needed.
use crate::mkv::RangeSource;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// A local file.
pub struct FileRange(std::fs::File);

impl FileRange {
    pub fn open(path: &std::path::Path) -> io::Result<Self> {
        std::fs::File::open(path).map(Self)
    }
}

impl RangeSource for FileRange {
    fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
        self.0.seek(SeekFrom::Start(offset))?;
        let mut out = Vec::new();
        (&mut self.0).take(len).read_to_end(&mut out)?;
        Ok(out)
    }
}

/// Lets another thread give up a read in progress: the reader waits for the server in short turns and looks at
/// this between them, so it fails within moments instead of waiting out a slow server. (Shutting the socket down
/// from the other thread does not wake a blocked read on Windows.) Also tells whether the server has started to
/// answer.
#[derive(Default)]
pub struct Cancel {
    cancelled: AtomicBool,
    answered: AtomicBool,
    /// Milliseconds from the request to the status line (0 until then).
    answer_ms: std::sync::atomic::AtomicU64,
}

impl Cancel {
    /// Stop the read in progress (and any later one through the same handle).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// The server sent its status line: the bytes are on their way.
    pub fn answered(&self) -> bool {
        self.answered.load(Ordering::SeqCst)
    }

    /// How long the server took to start answering the last request, once it has.
    pub fn answer_time(&self) -> Option<Duration> {
        self.answered()
            .then(|| Duration::from_millis(self.answer_ms.load(Ordering::SeqCst)))
    }
}

/// How long a cancellable read waits for the server before it looks at its [`Cancel`] again.
const TURN: Duration = Duration::from_millis(100);

/// A connection whose reads can be given up: each waits in turns of [`TURN`] up to `timeout`.
struct Watched {
    stream: TcpStream,
    cancel: Option<Arc<Cancel>>,
    timeout: Duration,
}

impl Read for Watched {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(cancel) = &self.cancel else {
            return self.stream.read(buf);
        };
        let started = std::time::Instant::now();
        loop {
            if cancel.is_cancelled() {
                // Not `Interrupted`: the standard readers take that as "try again".
                return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "read cancelled"));
            }
            match self.stream.read(buf) {
                Err(e) if matches!(e.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                    if started.elapsed() >= self.timeout {
                        return Err(e);
                    }
                }
                done => return done,
            }
        }
    }
}

/// `http://host:port/path` on loopback, read with one `Range` request per call.
pub struct HttpRange {
    authority: String,
    path: String,
    timeout: Duration,
    cancel: Option<Arc<Cancel>>,
}

/// Says, from the bytes read so far, how many are enough (see [`RangeSource::read_at_until`]).
type Enough<'a> = dyn Fn(&[u8]) -> Option<u64> + 'a;

impl HttpRange {
    /// Only plain-HTTP loopback URLs are accepted (the relay); anything else is not ours to fetch here.
    pub fn new(url: &str, timeout: Duration) -> Option<Self> {
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = rest
            .split_once('/')
            .map_or((rest, "/".to_owned()), |(a, p)| (a, format!("/{p}")));
        let host = authority.rsplit_once(':').map_or(authority, |(h, _)| h);
        if host != "127.0.0.1" && host != "localhost" && host != "[::1]" {
            return None;
        }
        Some(Self {
            authority: authority.to_owned(),
            path,
            timeout,
            cancel: None,
        })
    }

    /// Reads through this source can be given up with `cancel`.
    #[must_use]
    pub fn with_cancel(mut self, cancel: Arc<Cancel>) -> Self {
        self.cancel = Some(cancel);
        self
    }
}

impl RangeSource for HttpRange {
    fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
        self.read(offset, len, None)
    }

    fn read_at_until(&mut self, offset: u64, len: u64, enough: &dyn Fn(&[u8]) -> Option<u64>) -> io::Result<Vec<u8>> {
        self.read(offset, len, Some(enough))
    }
}

impl HttpRange {
    /// One range request. With `enough`, the body is read piece by piece and the connection is dropped once the
    /// bytes it asks for are there: what the server would still send is not transferred.
    fn read(&mut self, offset: u64, len: u64, enough: Option<&Enough<'_>>) -> io::Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        if self.cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(io::Error::new(io::ErrorKind::ConnectionAborted, "read cancelled"));
        }
        let asked = std::time::Instant::now();
        let stream = TcpStream::connect(&self.authority)?;
        stream.set_read_timeout(Some(if self.cancel.is_some() {
            TURN.min(self.timeout)
        } else {
            self.timeout
        }))?;
        stream.set_write_timeout(Some(self.timeout))?;
        stream.set_nodelay(true)?;
        let last = offset.saturating_add(len - 1);
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nRange: bytes={offset}-{last}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            self.path, self.authority
        );
        (&stream).write_all(request.as_bytes())?;
        let mut r = BufReader::with_capacity(
            256 * 1024,
            Watched {
                stream,
                cancel: self.cancel.clone(),
                timeout: self.timeout,
            },
        );
        let mut line = String::new();
        r.read_line(&mut line)?;
        if let Some(c) = &self.cancel {
            c.answer_ms.store(
                u64::try_from(asked.elapsed().as_millis()).unwrap_or(u64::MAX),
                Ordering::SeqCst,
            );
            c.answered.store(true, Ordering::SeqCst);
        }
        let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let mut length: Option<u64> = None;
        let mut chunked = false;
        loop {
            line.clear();
            if r.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                let v = v.trim();
                if k.eq_ignore_ascii_case("content-length") {
                    length = v.parse().ok();
                } else if k.eq_ignore_ascii_case("transfer-encoding") && v.eq_ignore_ascii_case("chunked") {
                    chunked = true;
                }
            }
        }
        match status {
            206 => {}
            // A server that ignores Range answers 200 with the whole file: only the start is usable.
            200 if offset == 0 => {}
            416 => return Ok(Vec::new()),
            s => return Err(io::Error::other(format!("range request answered {s}"))),
        }
        let mut out = Vec::with_capacity(usize::try_from(len.min(64 * 1024 * 1024)).unwrap_or(0));
        if chunked {
            loop {
                line.clear();
                r.read_line(&mut line)?;
                let size = u64::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
                if size == 0 || out.len() as u64 >= len {
                    break;
                }
                (&mut r).take(size).read_to_end(&mut out)?;
                line.clear();
                r.read_line(&mut line)?;
            }
        } else if let Some(enough) = enough {
            let mut limit = length.unwrap_or(len).min(len);
            let mut told = false;
            let mut piece = vec![0u8; 64 * 1024];
            while (out.len() as u64) < limit {
                let room = usize::try_from(limit - out.len() as u64)
                    .unwrap_or(usize::MAX)
                    .min(piece.len());
                let n = r.read(&mut piece[..room])?;
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&piece[..n]);
                if !told && let Some(needed) = enough(&out) {
                    told = true;
                    limit = limit.min(needed);
                }
            }
        } else {
            (&mut r).take(length.unwrap_or(len).min(len)).read_to_end(&mut out)?;
        }
        out.truncate(usize::try_from(len).unwrap_or(usize::MAX));
        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn only_loopback_http() {
        assert!(HttpRange::new("http://127.0.0.1:9/r/x/a.mkv", Duration::from_secs(1)).is_some());
        assert!(HttpRange::new("https://127.0.0.1:9/a", Duration::from_secs(1)).is_none());
        assert!(HttpRange::new("http://example.com/a", Duration::from_secs(1)).is_none());
    }

    #[test]
    fn reads_a_range_from_a_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(&s);
            let mut req = String::new();
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l == "\r\n" {
                    break;
                }
                req.push_str(&l);
            }
            (&s).write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\n\r\nabcd")
                .unwrap();
            req
        });
        let mut h = HttpRange::new(&format!("http://127.0.0.1:{port}/r/t/a.mkv"), Duration::from_secs(5)).unwrap();
        assert_eq!(h.read_at(10, 4).unwrap(), b"abcd");
        let req = server.join().unwrap();
        assert!(req.starts_with("GET /r/t/a.mkv HTTP/1.1"));
        assert!(req.contains(&format!("Host: 127.0.0.1:{port}")));
        assert!(req.contains("Range: bytes=10-13"));
    }

    #[test]
    fn a_read_stops_where_it_is_told_to() {
        // The server offers a megabyte; the reader needs the first 100 000 bytes, which the first bytes tell it.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(&s);
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l == "\r\n" {
                    break;
                }
            }
            (&s).write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 1048576\r\n\r\n")
                .unwrap();
            // The reader hangs up early; how much was written before that does not matter.
            let _ = (&s).write_all(&vec![7u8; 1_048_576]);
        });
        let mut h = HttpRange::new(&format!("http://127.0.0.1:{port}/a"), Duration::from_secs(5)).unwrap();
        let got = h
            .read_at_until(0, 1_048_576, &|b: &[u8]| (b.len() >= 4).then_some(100_000))
            .unwrap();
        assert_eq!(got.len(), 100_000);
        server.join().unwrap();
        // Without an answer from `enough` the whole range is read.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (s, _) = listener.accept().unwrap();
            let mut r = BufReader::new(&s);
            loop {
                let mut l = String::new();
                r.read_line(&mut l).unwrap();
                if l == "\r\n" {
                    break;
                }
            }
            (&s).write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 6\r\n\r\nabcdef")
                .unwrap();
        });
        let mut h = HttpRange::new(&format!("http://127.0.0.1:{port}/a"), Duration::from_secs(5)).unwrap();
        assert_eq!(h.read_at_until(0, 6, &|_: &[u8]| None).unwrap(), b"abcdef");
        server.join().unwrap();
    }

    #[test]
    fn a_cancelled_read_fails_at_once() {
        // A server that accepts and never answers.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let cancel = Arc::new(Cancel::default());
        let mut h = HttpRange::new(&format!("http://127.0.0.1:{port}/a"), Duration::from_secs(30))
            .unwrap()
            .with_cancel(Arc::clone(&cancel));
        let reader = std::thread::spawn(move || h.read_at(0, 4));
        let (_held, _) = listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(!cancel.answered());
        let started = std::time::Instant::now();
        cancel.cancel();
        assert!(reader.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        // Later reads through the same handle do not start.
        let mut again = HttpRange::new(&format!("http://127.0.0.1:{port}/a"), Duration::from_secs(30))
            .unwrap()
            .with_cancel(cancel);
        assert!(again.read_at(0, 4).is_err());
    }
}
