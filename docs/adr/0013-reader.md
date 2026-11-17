# ADR-0013: Ebook and manga reader

- Status: Accepted
- Date: 2026-11-17
- Inputs:
  - Roadmap item R10 (owner, 2026-11-16): port the Silo web app's ebook and manga reading.
  - `docs/research/silo-api.md` §4.1k: the ebook bytes, progress and manga chapter contracts @749db96a.
  - The web reader (`web/src/reader/FoliateBookReader.tsx@749db96a`), read as data. It uses a vendored foliate-js
    for EPUB, MOBI, FB2 and comics, and pdfjs-dist for PDF.

## Context

- Books are files the server serves whole (`readEbookFile`). The WebView must not receive credentials (ADR-0004),
  and its CSP forbids remote content.
- EPUB documents are XHTML from untrusted files. They can contain scripts, frames, forms and links to the web.
- A manga chapter is a CBZ: a zip of page images.
- Vendoring foliate-js would mean carrying a reader engine that has no npm release we can pin and review. The web
  app carries a fork of it.

## Decision

- **The host holds the book.** `book_open` fetches the file through the session, up to 512 MiB, and decides the
  format from its bytes:
  - `%PDF-` is a PDF;
  - a zip with `META-INF/container.xml` is an EPUB;
  - a zip with images is a comic;
  - anything else (MOBI the server did not convert, CBR, FB2) is reported as `unsupported_book`.
- **The UI reads one piece at a time.** `book_entry` returns one archive entry as raw bytes (`tauri::ipc::Response`),
  or the whole file for a PDF.
  - Entries are capped at 128 MiB after inflating, so a zip bomb fails rather than filling memory.
  - At most two books stay open. Nothing is written to disk.
  - The archive is read with the `zip` crate, inflate only (`deflate-flate2-zlib-rs`; no writing, encryption or
    other codecs).
- **EPUB rendering is Granary's own and small.** `src/lib/books.ts` reads the container, the OPF spine and the
  nav/NCX contents with `DOMParser`. Each spine document is then made safe and self-contained:
  - scripts, frames, objects, forms, `base` and `meta` are removed, and so are event-handler attributes and
    `javascript:` URLs;
  - images become blob URLs, and stylesheets are inlined with their `url()` and `@import` removed;
  - links are made inert and handled by the reader.
  - The document is shown in an `iframe` with `srcdoc` and `sandbox="allow-same-origin"` (no `allow-scripts`). That
    is a second fence after the sanitiser, and the app CSP still applies inside.
  - The reader scrolls each chapter; text size and page colour (dark, sepia, light) are kept on this computer.
- **PDF uses pdfjs-dist 6.5.438** (Apache-2.0, the library the web app uses). Its worker is bundled by Vite and loaded
  from the app origin, so the CSP stays `script-src 'self'`. XFA is off. Pages render one at a time to a canvas.
- **Comics show one page at a time**, in natural name order, fitted to the window, with optional right-to-left
  reading. Neighbouring pages are decoded ahead and released behind.
- **Progress is the server's**, saved a second after the reader settles and when it closes.
  - The location is `fraction:<n>`, the form the web reader restores without a CFI. A CFI saved by the web reader
    reopens from its `progress` fraction.
  - The event time is kept with each save, as the contract asks.

## Consequences

- EPUB, PDF and CBZ open in the app; the server's Kindle conversion makes most MOBI/AZW books EPUBs.
- Not supported: CBR (RAR), FB2, MOBI without conversion, fixed-layout EPUB spreads, paginated EPUB columns, EPUB
  fonts from CSS, annotations, reader configuration sync, and CFI-exact positions. A CFI from the web reader is
  approximated by its fraction, and Granary's fraction locations reopen in the web reader at the same share of the
  book.
- No real-server check was possible: the owner's server has no ebook or manga library. The reader was tested against
  the mock server with a generated EPUB, PDF and two CBZ chapters (`scripts/gen-mock-books.mjs`).
