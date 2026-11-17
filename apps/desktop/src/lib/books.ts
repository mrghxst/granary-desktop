import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CatalogItemDetail, FileVersion } from "./catalog";

/** Book formats the reader opens (R10); the host decides from the file's bytes. */
export type BookFormat = "epub" | "pdf" | "comic";

export interface BookInfo {
  handle: string;
  format: BookFormat;
  /** Archive entry names (EPUB, comics). */
  entries: string[];
  size: number;
}

/** `EbookProgress`: `location` is a CFI (from the web reader) or a `fraction:<n>` value. */
export interface EbookProgress {
  content_id: string;
  file_id: string;
  location: string;
  progress: number;
  updated_at: string;
}

function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(new Error("no host"));
  return invoke<T>(cmd, args);
}

export const books = {
  open: (serverId: string, contentId: string, fileId: string) =>
    call<BookInfo>("book_open", { serverId, contentId, fileId }),
  /** One archive entry, or (`name` empty) a whole PDF, as bytes. */
  entry: (handle: string, name: string) => call<ArrayBuffer>("book_entry", { handle, name }),
  close: (handle: string) => call<null>("book_close", { handle }),
  progress: (serverId: string, contentId: string) =>
    call<EbookProgress | null>("ebook_progress", { serverId, contentId }),
  saveProgress: (
    serverId: string,
    contentId: string,
    fileId: string,
    location: string,
    progress: number,
    updatedAt: string,
  ) =>
    call<EbookProgress | null>("ebook_progress_save", { serverId, contentId, fileId, location, progress, updatedAt }),
};

const READABLE = ["epub", "pdf", "cbz"];

/** The format a file looks like from its container (`readerFileFormat` in the web reader, `@749db96a`). */
export function fileFormat(v: Pick<FileVersion, "container">): string {
  return v.container.trim().toLowerCase().replace(/^\./, "");
}

/** The file to read: EPUB first, then PDF, then a comic archive, as the web reader prefers EPUB. */
export function readerFile(versions: readonly FileVersion[], requested?: string | null): FileVersion | undefined {
  const asked = requested ? versions.find((v) => v.file_id === requested) : undefined;
  if (asked) return asked;
  for (const f of READABLE) {
    const v = versions.find((x) => fileFormat(x) === f);
    if (v) return v;
  }
  return versions[0];
}

export function isBook(item: Pick<CatalogItemDetail, "type">): boolean {
  return item.type === "ebook";
}

/** `fraction:<n>`, the location form the web reader restores for formats without CFIs. */
export function fractionLocation(fraction: number): string {
  return `fraction:${clamp(fraction).toFixed(6)}`;
}

/** Where to reopen a book, as a fraction of it: a `fraction:` location, else the stored progress. */
export function restoreFraction(p: Pick<EbookProgress, "location" | "progress"> | null | undefined): number {
  if (!p) return 0;
  const loc = p.location.trim();
  if (loc.startsWith("fraction:")) {
    const v = Number(loc.slice("fraction:".length));
    if (Number.isFinite(v)) return clamp(v);
  }
  return Number.isFinite(p.progress) ? clamp(p.progress) : 0;
}

/**
 * A client event time for a progress save. The server refuses future times and keeps only strictly newer events, so
 * it is taken a few seconds behind the clock (a computer slightly ahead of the server still saves).
 */
export function eventTime(now = Date.now()): string {
  return new Date(now - 3000).toISOString();
}

function clamp(v: number): number {
  return Math.min(1, Math.max(0, v));
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
const IMAGE = /\.(jpe?g|png|webp|gif|avif|bmp)$/i;

/** A comic's pages: its images in natural order (`page2` before `page10`), skipping macOS metadata. */
export function comicPages(entries: readonly string[]): string[] {
  return entries
    .filter((e) => IMAGE.test(e) && !e.toLowerCase().startsWith("__macosx/") && !e.split("/").pop()?.startsWith("."))
    .sort((a, b) => collator.compare(a, b));
}

export function mimeFor(name: string): string {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    png: "image/png",
    webp: "image/webp",
    gif: "image/gif",
    avif: "image/avif",
    bmp: "image/bmp",
    svg: "image/svg+xml",
    css: "text/css",
  };
  return map[ext] ?? "application/octet-stream";
}

/** Resolve `href` against the archive path `base` (a file), dropping any fragment; null when it leaves the book. */
export function resolveHref(base: string, href: string): string | null {
  const clean = href.split("#")[0]?.split("?")[0] ?? "";
  if (!clean || /^[a-z][a-z0-9+.-]*:/i.test(clean)) return null;
  let decoded: string;
  try {
    decoded = decodeURIComponent(clean);
  } catch {
    decoded = clean;
  }
  const parts = decoded.startsWith("/") ? [] : base.split("/").slice(0, -1);
  for (const seg of decoded.split("/")) {
    if (seg === "" || seg === ".") continue;
    if (seg === "..") {
      if (parts.length === 0) return null;
      parts.pop();
    } else parts.push(seg);
  }
  return parts.join("/");
}

function parseXml(text: string, type: DOMParserSupportedType = "application/xml"): Document {
  const doc = new DOMParser().parseFromString(text, type);
  if (type !== "text/html" && doc.getElementsByTagName("parsererror").length > 0) {
    return new DOMParser().parseFromString(text, "text/html");
  }
  return doc;
}

const byLocal = (root: Document | Element, name: string) =>
  Array.from(root.getElementsByTagName("*")).filter((e) => e.localName === name);

export interface EpubSpineItem {
  path: string;
  /** Rough length, for spreading the book fraction over chapters. */
  weight: number;
}

export interface EpubTocEntry {
  label: string;
  path: string;
  depth: number;
}

export interface EpubPackage {
  title: string;
  spine: EpubSpineItem[];
  navPath: string | null;
  ncxPath: string | null;
}

/** The package document's path from `META-INF/container.xml`. */
export function containerRootfile(xml: string): string | null {
  const root = byLocal(parseXml(xml), "rootfile")[0];
  return root?.getAttribute("full-path") ?? null;
}

/** Title, reading order and contents documents of an OPF package at `opfPath`. */
export function parseOpf(xml: string, opfPath: string): EpubPackage {
  const doc = parseXml(xml);
  const manifest = new Map<string, { path: string; type: string; props: string }>();
  for (const item of byLocal(doc, "item")) {
    const id = item.getAttribute("id");
    const href = item.getAttribute("href");
    const path = href ? resolveHref(opfPath, href) : null;
    if (id && path) {
      manifest.set(id, {
        path,
        type: item.getAttribute("media-type") ?? "",
        props: item.getAttribute("properties") ?? "",
      });
    }
  }
  const spine = byLocal(doc, "itemref")
    .filter((r) => r.getAttribute("linear") !== "no")
    .map((r) => manifest.get(r.getAttribute("idref") ?? ""))
    .filter((m): m is { path: string; type: string; props: string } => Boolean(m))
    .map((m) => ({ path: m.path, weight: 1 }));
  const nav = [...manifest.values()].find((m) => m.props.split(/\s+/).includes("nav"));
  const tocId = byLocal(doc, "spine")[0]?.getAttribute("toc");
  const ncx =
    (tocId ? manifest.get(tocId) : undefined) ??
    [...manifest.values()].find((m) => m.type === "application/x-dtbncx+xml");
  const title = byLocal(doc, "title")[0]?.textContent.trim() ?? "";
  return { title, spine, navPath: nav?.path ?? null, ncxPath: ncx?.path ?? null };
}

/** The table of contents of an EPUB 3 nav document. */
export function parseNav(xhtml: string, navPath: string): EpubTocEntry[] {
  const doc = parseXml(xhtml);
  const navs = byLocal(doc, "nav");
  const toc =
    navs.find(
      (n) => (n.getAttribute("epub:type") ?? n.getAttributeNS("http://www.idpf.org/2007/ops", "type")) === "toc",
    ) ?? navs[0];
  if (!toc) return [];
  const out: EpubTocEntry[] = [];
  for (const a of byLocal(toc, "a")) {
    const href = a.getAttribute("href");
    const path = href ? resolveHref(navPath, href) : null;
    let depth = 0;
    for (let p = a.parentElement; p && p !== toc; p = p.parentElement) if (p.localName === "ol") depth++;
    const label = a.textContent.replace(/\s+/g, " ").trim();
    if (path && label) out.push({ label, path, depth: Math.max(0, depth - 1) });
  }
  return out;
}

/** The table of contents of an EPUB 2 NCX document. */
export function parseNcx(xml: string, ncxPath: string): EpubTocEntry[] {
  const doc = parseXml(xml);
  const out: EpubTocEntry[] = [];
  for (const point of byLocal(doc, "navPoint")) {
    const label = byLocal(point, "text")[0]?.textContent.trim() ?? "";
    const src = byLocal(point, "content")[0]?.getAttribute("src");
    const path = src ? resolveHref(ncxPath, src) : null;
    let depth = 0;
    for (let p = point.parentElement; p; p = p.parentElement) if (p.localName === "navPoint") depth++;
    if (label && path) out.push({ label, path, depth });
  }
  return out;
}

const DROP = ["script", "iframe", "frame", "object", "embed", "form", "base", "meta", "noscript", "audio", "video"];

/** Resources an EPUB document refers to, by attribute. */
export interface ChapterResources {
  images: { el: Element; attr: string; path: string }[];
  styles: { el: Element; path: string }[];
  links: { el: Element; path: string | null }[];
}

/**
 * Parse an EPUB content document and strip everything active (scripts, frames, forms, event handlers, `javascript:`
 * links). The reader renders it in a frame without scripts as well; this is the first of two fences.
 */
export function sanitizeChapter(xhtml: string, path: string): { doc: Document; res: ChapterResources } {
  const doc = parseXml(xhtml, "application/xhtml+xml");
  for (const tag of DROP) for (const el of byLocal(doc, tag)) el.remove();
  const res: ChapterResources = { images: [], styles: [], links: [] };
  for (const el of Array.from(doc.getElementsByTagName("*"))) {
    for (const attr of Array.from(el.attributes)) {
      const name = attr.name.toLowerCase();
      if (name.startsWith("on") || name === "srcset") el.removeAttribute(attr.name);
      else if (/^\s*(javascript|vbscript|data:text\/html)/i.test(attr.value)) el.removeAttribute(attr.name);
    }
    const local = el.localName;
    if (local === "img" || local === "image") {
      const attr = el.hasAttribute("src") ? "src" : el.hasAttribute("href") ? "href" : "xlink:href";
      const ref = el.getAttribute(attr) ?? el.getAttributeNS("http://www.w3.org/1999/xlink", "href");
      const target = ref ? resolveHref(path, ref) : null;
      if (target) res.images.push({ el, attr, path: target });
    } else if (local === "link") {
      const rel = (el.getAttribute("rel") ?? "").toLowerCase();
      const href = el.getAttribute("href");
      const target = href ? resolveHref(path, href) : null;
      if (rel.includes("stylesheet") && target) res.styles.push({ el, path: target });
      else el.remove();
    } else if (local === "a") {
      const href = el.getAttribute("href");
      res.links.push({ el, path: href ? resolveHref(path, href) : null });
    }
  }
  return { doc, res };
}

/** Stylesheet text with `url()` references and imports removed (fonts and images from CSS are not loaded). */
export function sanitizeCss(css: string): string {
  return css.replace(/@import[^;]*;/gi, "").replace(/url\([^)]*\)/gi, "none");
}
