//! Byte-range sources for the Matroska preview index ([`crate::mkv`]): HTTP/1.1 range requests to the app's loopback
//! relay (which adds the server credentials, ADR-0008) and local files. Plain `std` networking: the relay is always
//! `http://127.0.0.1:<port>/...`, so neither TLS nor a client library is needed.
use crate::mkv::RangeSource;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
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

/// `http://host:port/path` on loopback, read with one `Range` request per call.
pub struct HttpRange {
    authority: String,
    path: String,
    timeout: Duration,
}

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
        })
    }
}

impl RangeSource for HttpRange {
    fn read_at(&mut self, offset: u64, len: u64) -> io::Result<Vec<u8>> {
        if len == 0 {
            return Ok(Vec::new());
        }
        let stream = TcpStream::connect(&self.authority)?;
        stream.set_read_timeout(Some(self.timeout))?;
        stream.set_write_timeout(Some(self.timeout))?;
        stream.set_nodelay(true)?;
        let last = offset.saturating_add(len - 1);
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nRange: bytes={offset}-{last}\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n",
            self.path, self.authority
        );
        (&stream).write_all(request.as_bytes())?;
        let mut r = BufReader::with_capacity(256 * 1024, stream);
        let mut line = String::new();
        r.read_line(&mut line)?;
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
}
