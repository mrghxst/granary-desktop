//! The reader (R10): the host fetches a book through the session (`readEbookFile`, docs/research/silo-api.md §4.1k)
//! and keeps it in memory; the WebView asks for one entry at a time (an EPUB document, stylesheet or image, a comic
//! page) or, for a PDF, the whole file. Nothing is written to disk and the WebView never sees credentials.
use crate::catalog::is_opaque_id;
use crate::silo::{CommandError, SiloState};
use granary_silo_api::catalog::{EbookProgress, EbookProgressSave};
use serde::Serialize;
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};
use tauri::State;
use tauri::ipc::Response;
use zip::ZipArchive;

type CmdResult<T> = Result<T, CommandError>;

/// Books kept open at once (the current one and the one being switched from).
const MAX_OPEN: usize = 2;
/// Largest single entry inflated from an archive (a defence against zip bombs; real pages are a few MiB).
const MAX_ENTRY_BYTES: u64 = 128 * 1024 * 1024;
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "avif", "bmp"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BookFormat {
    Epub,
    Pdf,
    /// A comic archive (CBZ): its images, in natural name order, are the pages.
    Comic,
}

enum Body {
    Pdf(Arc<Vec<u8>>),
    Zip(Mutex<ZipArchive<Cursor<Vec<u8>>>>),
}

struct OpenBook {
    handle: String,
    body: Body,
}

#[derive(Default)]
pub struct BookState {
    open: Mutex<Vec<Arc<OpenBook>>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookInfo {
    pub handle: String,
    pub format: BookFormat,
    /// Archive entry names (EPUB and comics); empty for a PDF.
    pub entries: Vec<String>,
    pub size: usize,
}

fn unsupported() -> CommandError {
    CommandError::plain(
        "unsupported_book",
        "this book format cannot be opened in Granary".into(),
    )
}

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !lower.starts_with("__macosx/")
        && lower
            .rsplit_once('.')
            .is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext))
}

/// What a downloaded book is, from its bytes (the served MIME type can be generic).
fn detect(bytes: Vec<u8>) -> CmdResult<(BookFormat, Body, Vec<String>)> {
    if bytes.starts_with(b"%PDF-") {
        return Ok((BookFormat::Pdf, Body::Pdf(Arc::new(bytes)), Vec::new()));
    }
    if !bytes.starts_with(b"PK") {
        // MOBI/AZW (when the server does not convert them), CBR, FB2 and others.
        return Err(unsupported());
    }
    let archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| unsupported())?;
    let entries: Vec<String> = archive
        .file_names()
        .filter_map(|n| n.ok().map(std::borrow::Cow::into_owned))
        .collect();
    let format = if entries.iter().any(|n| n == "META-INF/container.xml") {
        BookFormat::Epub
    } else if entries.iter().any(|n| is_image(n)) {
        BookFormat::Comic
    } else {
        return Err(unsupported());
    };
    Ok((format, Body::Zip(Mutex::new(archive)), entries))
}

fn lock_err() -> CommandError {
    CommandError::plain("internal", "reader state unavailable".into())
}

impl BookState {
    fn insert(&self, book: OpenBook) -> CmdResult<()> {
        let mut open = self.open.lock().map_err(|_| lock_err())?;
        open.push(Arc::new(book));
        let excess = open.len().saturating_sub(MAX_OPEN);
        open.drain(..excess);
        Ok(())
    }

    fn get(&self, handle: &str) -> CmdResult<Arc<OpenBook>> {
        let open = self.open.lock().map_err(|_| lock_err())?;
        open.iter()
            .find(|b| b.handle == handle)
            .cloned()
            .ok_or_else(|| CommandError::plain("book_closed", "the book is no longer open".into()))
    }
}

fn read_entry(book: &OpenBook, name: &str) -> CmdResult<Vec<u8>> {
    match &book.body {
        Body::Pdf(bytes) if name.is_empty() => Ok(bytes.as_ref().clone()),
        Body::Pdf(_) => Err(CommandError::invalid("book entry".into())),
        Body::Zip(archive) => {
            let mut archive = archive.lock().map_err(|_| lock_err())?;
            let mut file = archive
                .by_name(name)
                .map_err(|_| CommandError::plain("not_found", "no such entry in the book".into()))?;
            if file.size() > MAX_ENTRY_BYTES {
                return Err(CommandError::plain("book_too_large", "book entry too large".into()));
            }
            let mut out = Vec::with_capacity(usize::try_from(file.size()).unwrap_or(0));
            // `take` caps what a lying header can inflate to.
            (&mut file)
                .take(MAX_ENTRY_BYTES + 1)
                .read_to_end(&mut out)
                .map_err(|_| unsupported())?;
            if out.len() as u64 > MAX_ENTRY_BYTES {
                return Err(CommandError::plain("book_too_large", "book entry too large".into()));
            }
            Ok(out)
        }
    }
}

/// Fetch a book file and open it for reading.
#[tauri::command]
pub async fn book_open(
    silo: State<'_, SiloState>,
    books: State<'_, BookState>,
    server_id: String,
    content_id: String,
    file_id: String,
) -> CmdResult<BookInfo> {
    if !is_opaque_id(&content_id) || !is_opaque_id(&file_id) {
        return Err(CommandError::invalid("book".into()));
    }
    let file = silo
        .with_session(&server_id, |s| async move { s.ebook_file(&content_id, &file_id).await })
        .await?;
    let size = file.bytes.len();
    let (format, body, entries) = tauri::async_runtime::spawn_blocking(move || detect(file.bytes))
        .await
        .map_err(|_| lock_err())??;
    let handle = uuid::Uuid::new_v4().to_string();
    books.insert(OpenBook {
        handle: handle.clone(),
        body,
    })?;
    tracing::info!(?format, size, entries = entries.len(), "book opened");
    Ok(BookInfo {
        handle,
        format,
        entries,
        size,
    })
}

/// One entry of an open book as raw bytes (`name` empty: the whole PDF).
#[tauri::command]
pub async fn book_entry(books: State<'_, BookState>, handle: String, name: String) -> CmdResult<Response> {
    if name.len() > 1024 || name.chars().any(char::is_control) {
        return Err(CommandError::invalid("book entry".into()));
    }
    let book = books.get(&handle)?;
    let bytes = tauri::async_runtime::spawn_blocking(move || read_entry(&book, &name))
        .await
        .map_err(|_| lock_err())??;
    Ok(Response::new(bytes))
}

#[tauri::command]
pub fn book_close(books: State<'_, BookState>, handle: String) -> CmdResult<()> {
    let mut open = books.open.lock().map_err(|_| lock_err())?;
    open.retain(|b| b.handle != handle);
    Ok(())
}

/// The profile's place in a book (`getEbookProgress`).
#[tauri::command]
pub async fn ebook_progress(
    silo: State<'_, SiloState>,
    server_id: String,
    content_id: String,
) -> CmdResult<Option<EbookProgress>> {
    if !is_opaque_id(&content_id) {
        return Err(CommandError::invalid("book".into()));
    }
    silo.with_session(&server_id, |s| async move { s.ebook_progress(&content_id).await })
        .await
}

/// Whether `t` looks like the RFC 3339 UTC millisecond instant the server expects (`2026-01-02T03:04:05.000Z`).
fn is_event_time(t: &str) -> bool {
    let b = t.as_bytes();
    b.len() == 24
        && b[4] == b'-'
        && b[10] == b'T'
        && b[19] == b'.'
        && b[23] == b'Z'
        && t.chars().all(|c| c.is_ascii_digit() || "-T:.Z".contains(c))
}

/// Save the profile's place in a book (`saveEbookProgress`); returns what the server keeps.
#[tauri::command]
pub async fn ebook_progress_save(
    silo: State<'_, SiloState>,
    server_id: String,
    content_id: String,
    file_id: String,
    location: String,
    progress: f64,
    updated_at: String,
) -> CmdResult<Option<EbookProgress>> {
    let ok = is_opaque_id(&content_id)
        && is_opaque_id(&file_id)
        && !location.is_empty()
        && location.len() <= 8192
        && progress.is_finite()
        && (0.0..=1.0).contains(&progress)
        && is_event_time(&updated_at);
    if !ok {
        return Err(CommandError::invalid("reading progress".into()));
    }
    let save = EbookProgressSave {
        file_id,
        location,
        progress,
        updated_at,
    };
    silo.with_session(&server_id, |s| async move {
        let save = save;
        s.ebook_progress_save(&content_id, &save).await
    })
    .await
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // test code
mod tests {
    use super::*;

    fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            std::io::Write::write_all(&mut w, data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn detects_books_by_their_bytes() {
        let (f, _, e) = detect(b"%PDF-1.4\n".to_vec()).unwrap();
        assert_eq!((f, e.len()), (BookFormat::Pdf, 0));
        let epub = stored_zip(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", b"<c/>"),
        ]);
        assert_eq!(detect(epub).unwrap().0, BookFormat::Epub);
        let cbz = stored_zip(&[("ComicInfo.xml", b"<c/>"), ("01.JPG", b"x")]);
        let (f, _, e) = detect(cbz).unwrap();
        assert_eq!((f, e.len()), (BookFormat::Comic, 2));
        assert_eq!(
            detect(b"BOOKMOBI".to_vec()).err().map(|e| e.code).as_deref(),
            Some("unsupported_book")
        );
        let notes = stored_zip(&[("readme.txt", b"x")]);
        assert_eq!(detect(notes).err().map(|e| e.code).as_deref(), Some("unsupported_book"));
    }

    #[test]
    fn reads_entries_and_whole_pdfs() {
        let (_, body, _) = detect(stored_zip(&[("p/1.png", b"one")])).unwrap();
        let book = OpenBook {
            handle: "h".into(),
            body,
        };
        assert_eq!(read_entry(&book, "p/1.png").unwrap(), b"one");
        assert_eq!(read_entry(&book, "p/2.png").unwrap_err().code, "not_found");
        let (_, body, _) = detect(b"%PDF-1.7 x".to_vec()).unwrap();
        let pdf = OpenBook {
            handle: "p".into(),
            body,
        };
        assert_eq!(read_entry(&pdf, "").unwrap(), b"%PDF-1.7 x");
        assert!(read_entry(&pdf, "x").is_err());
    }

    #[test]
    fn keeps_only_the_latest_books_open() {
        let state = BookState::default();
        for h in ["a", "b", "c"] {
            let (_, body, _) = detect(b"%PDF-".to_vec()).unwrap();
            state.insert(OpenBook { handle: h.into(), body }).unwrap();
        }
        assert!(state.get("a").is_err());
        assert!(state.get("b").is_ok() && state.get("c").is_ok());
    }

    #[test]
    fn event_times_are_utc_milliseconds() {
        assert!(is_event_time("2026-01-02T03:04:05.000Z"));
        assert!(!is_event_time("2026-01-02T03:04:05Z"));
        assert!(!is_event_time("2026-01-02T03:04:05.000+01:00"));
        assert!(!is_event_time("2026-01-02 03:04:05.000Z"));
    }
}
