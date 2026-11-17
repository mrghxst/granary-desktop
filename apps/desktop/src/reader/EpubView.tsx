import { ChevronLeft, ChevronRight, ListTree } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  type BookInfo,
  books,
  containerRootfile,
  type EpubSpineItem,
  type EpubTocEntry,
  mimeFor,
  parseNav,
  parseNcx,
  parseOpf,
  sanitizeChapter,
  sanitizeCss,
} from "../lib/books";
import { errorMessage } from "../lib/errors";
import { type ReaderLook, THEME_COLORS } from "./look";

const XLINK = "http://www.w3.org/1999/xlink";
const XHTML = "http://www.w3.org/1999/xhtml";

async function text(handle: string, name: string): Promise<string> {
  return new TextDecoder().decode(await books.entry(handle, name));
}

function lookCss(look: ReaderLook): string {
  const c = THEME_COLORS[look.theme];
  return `html { background: ${c.bg} !important; color: ${c.fg} !important; }
body { box-sizing: border-box; max-width: 44em; margin: 0 auto !important; padding: 3rem 1.75rem 2rem !important;
  font-size: ${String(look.fontSize)}% !important; line-height: 1.65; overflow: hidden !important;
  font-family: ui-serif, Georgia, Cambria, "Times New Roman", serif; background: transparent !important; }
body * { color: inherit !important; background-color: transparent !important; border-color: currentColor; }
img, svg, video { max-width: 100% !important; height: auto !important; }
[data-granary-href] { text-decoration: underline; cursor: pointer; }
::selection { background: rgba(120, 160, 255, 0.35); }`;
}

/** A chapter document made safe and self-contained: images as blob URLs, stylesheets inlined, links inert. */
async function renderChapter(
  handle: string,
  path: string,
  look: ReaderLook,
): Promise<{ html: string; urls: string[] }> {
  const { doc, res } = sanitizeChapter(await text(handle, path), path);
  const urls: string[] = [];
  await Promise.all(
    res.images.map(async (img) => {
      try {
        const bytes = await books.entry(handle, img.path);
        const url = URL.createObjectURL(new Blob([bytes], { type: mimeFor(img.path) }));
        urls.push(url);
        if (img.attr === "xlink:href") img.el.setAttributeNS(XLINK, "xlink:href", url);
        else img.el.setAttribute(img.attr, url);
      } catch {
        img.el.removeAttribute(img.attr);
      }
    }),
  );
  await Promise.all(
    res.styles.map(async (s) => {
      try {
        const style = doc.createElementNS(XHTML, "style");
        style.textContent = sanitizeCss(await text(handle, s.path));
        s.el.replaceWith(style);
      } catch {
        s.el.remove();
      }
    }),
  );
  for (const l of res.links) {
    const href = l.el.getAttribute("href") ?? "";
    l.el.removeAttribute("href");
    if (l.path)
      l.el.setAttribute("data-granary-href", l.path + (href.includes("#") ? `#${href.split("#")[1] ?? ""}` : ""));
    else if (href.startsWith("#")) l.el.setAttribute("data-granary-href", `#${href.slice(1)}`);
  }
  // Serialise with HTML rules (the frame parses `srcdoc` as HTML, where `<div/>` would not close).
  const out = document.implementation.createHTMLDocument("");
  out.documentElement.replaceWith(out.importNode(doc.documentElement, true));
  let head = out.querySelector("head");
  if (!head) {
    head = out.createElement("head");
    out.documentElement.prepend(head);
  }
  const look_ = out.createElement("style");
  look_.textContent = lookCss(look);
  head.append(look_);
  return { html: `<!doctype html>${out.documentElement.outerHTML}`, urls };
}

/**
 * An EPUB, one spine document at a time in a frame without scripts (`sandbox` without `allow-scripts`), scrolled by
 * the reader. The book position is the chapter index plus the scrolled share of it, over the spine.
 */
export function EpubView({
  book,
  start,
  look,
  onPosition,
}: {
  book: BookInfo;
  start: number;
  look: ReaderLook;
  onPosition: (fraction: number) => void;
}) {
  const { t } = useTranslation();
  const [spine, setSpine] = useState<EpubSpineItem[] | null>(null);
  const [toc, setToc] = useState<EpubTocEntry[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [index, setIndex] = useState(0);
  const [chapter, setChapter] = useState<{ html: string; index: number } | null>(null);
  const [height, setHeight] = useState(0);
  const [tocOpen, setTocOpen] = useState(false);
  const scroller = useRef<HTMLDivElement>(null);
  const frame = useRef<HTMLIFrameElement>(null);
  /** Share of the chapter to scroll to once it has laid out (restore, or "from the end" when going back). */
  const pendingScroll = useRef<number | null>(null);
  const anchor = useRef<string | null>(null);
  const observer = useRef<ResizeObserver | null>(null);
  useEffect(
    () => () => {
      observer.current?.disconnect();
    },
    [],
  );
  const report = useRef(onPosition);
  useEffect(() => {
    report.current = onPosition;
  });

  useEffect(() => {
    const live = { alive: true };
    void (async () => {
      try {
        const opfPath = containerRootfile(await text(book.handle, "META-INF/container.xml"));
        if (!opfPath) throw new Error("no package document");
        const pkg = parseOpf(await text(book.handle, opfPath), opfPath);
        let contents: EpubTocEntry[] = [];
        try {
          if (pkg.navPath) contents = parseNav(await text(book.handle, pkg.navPath), pkg.navPath);
          if (contents.length === 0 && pkg.ncxPath)
            contents = parseNcx(await text(book.handle, pkg.ncxPath), pkg.ncxPath);
        } catch {
          // A broken table of contents leaves the book readable.
        }
        if (!live.alive) return;
        if (pkg.spine.length === 0) throw new Error("empty spine");
        const at = Math.min(pkg.spine.length - 1, Math.floor(start * pkg.spine.length));
        pendingScroll.current = start * pkg.spine.length - at;
        setSpine(pkg.spine);
        setToc(contents);
        setIndex(at);
      } catch (e) {
        if (live.alive) setError(e);
      }
    })();
    return () => {
      live.alive = false;
    };
    // `start` only applies when the book opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [book.handle]);

  useEffect(() => {
    const item = spine?.[index];
    if (!item) return;
    let alive = true;
    let urls: string[] = [];
    void renderChapter(book.handle, item.path, look).then(
      (r) => {
        urls = r.urls;
        if (!alive) {
          for (const u of urls) URL.revokeObjectURL(u);
          return;
        }
        setChapter({ html: r.html, index });
      },
      (e: unknown) => {
        if (alive) setError(e);
      },
    );
    return () => {
      alive = false;
      // Revoke after the next chapter replaced the frame's document.
      window.setTimeout(() => {
        for (const u of urls) URL.revokeObjectURL(u);
      }, 2000);
    };
  }, [book.handle, spine, index, look]);

  const count = spine?.length ?? 0;
  const position = useCallback(() => {
    const el = scroller.current;
    if (!el || count === 0 || !chapter) return;
    const room = el.scrollHeight - el.clientHeight;
    const within = room > 4 ? el.scrollTop / room : chapter.index === count - 1 ? 1 : 0;
    report.current(Math.min(1, (chapter.index + within) / count));
  }, [chapter, count]);

  const go = useCallback(
    (to: number, from: "start" | "end" = "start") => {
      if (to < 0 || to >= count) return;
      pendingScroll.current = from === "end" ? 1 : 0;
      setIndex(to);
    },
    [count],
  );

  const follow = useCallback(
    (target: string) => {
      const [path, hash] = target.split("#");
      if (!path) {
        if (hash) frame.current?.contentDocument?.getElementById(hash)?.scrollIntoView();
        return;
      }
      const to = spine?.findIndex((s) => s.path === path) ?? -1;
      if (to < 0) return;
      anchor.current = hash ?? null;
      if (to === index) {
        if (hash) frame.current?.contentDocument?.getElementById(hash)?.scrollIntoView();
        else scroller.current?.scrollTo({ top: 0 });
      } else go(to);
    },
    [spine, index, go],
  );

  const onLoad = () => {
    const doc = frame.current?.contentDocument;
    if (!doc) return;
    const fit = () => {
      setHeight(doc.documentElement.scrollHeight);
    };
    fit();
    // Measured on the next frame: resizing the frame inside the observer callback would loop.
    const ro = new ResizeObserver(() => requestAnimationFrame(fit));
    // One observer at a time: the previous chapter's document is gone with its frame.
    observer.current?.disconnect();
    observer.current = ro;
    ro.observe(doc.body);
    doc.addEventListener("click", (e) => {
      const a = (e.target as Element | null)?.closest("[data-granary-href]");
      const target = a?.getAttribute("data-granary-href");
      if (target) {
        e.preventDefault();
        follow(target);
      }
    });
    doc.addEventListener("keydown", (e) => {
      onKey(e);
    });
    doc.addEventListener("wheel", userScrolls, { passive: true });
    doc.addEventListener("pointerdown", userScrolls);
    requestAnimationFrame(fit);
  };

  /** The reader takes over the scroll position: restores and "from the end" stop re-applying. */
  const userScrolls = () => {
    pendingScroll.current = null;
  };

  // Once the frame has its height (and again when late images change it), put the scroll where it should be;
  // until the reader scrolls, the restored share wins over layout changes.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (!el || height === 0) return;
    if (anchor.current) {
      const target = frame.current?.contentDocument?.getElementById(anchor.current);
      if (target) {
        el.scrollTop = target.getBoundingClientRect().top;
        anchor.current = null;
      }
    } else if (pendingScroll.current !== null) {
      el.scrollTop = pendingScroll.current * Math.max(0, el.scrollHeight - el.clientHeight);
    }
    position();
  }, [height, position]);

  const onKey = (e: KeyboardEvent) => {
    const el = scroller.current;
    if (!el) return;
    userScrolls();
    const atEnd = el.scrollTop >= el.scrollHeight - el.clientHeight - 4;
    const atTop = el.scrollTop <= 4;
    if (e.key === "ArrowRight" || ((e.key === "PageDown" || e.key === " ") && atEnd)) go(index + 1);
    else if (e.key === "ArrowLeft" || (e.key === "PageUp" && atTop)) go(index - 1, "end");
    else if (e.key === "PageDown" || e.key === " ") el.scrollBy({ top: el.clientHeight * 0.9 });
    else if (e.key === "PageUp") el.scrollBy({ top: -el.clientHeight * 0.9 });
    else if (e.key === "ArrowDown") el.scrollBy({ top: 60 });
    else if (e.key === "ArrowUp") el.scrollBy({ top: -60 });
    else return;
    e.preventDefault();
  };

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      onKey(e);
    };
    window.addEventListener("keydown", handler);
    return () => {
      window.removeEventListener("keydown", handler);
    };
  });

  if (error) return <p className="p-10 text-center text-destructive">{errorMessage(t, error)}</p>;
  const colors = THEME_COLORS[look.theme];
  const current = spine?.[index]?.path;
  return (
    <div className="relative flex min-h-0 flex-1" style={{ background: colors.bg, color: colors.fg }}>
      <div
        ref={scroller}
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={position}
        onWheel={userScrolls}
        onPointerDown={userScrolls}
      >
        {chapter ? (
          <iframe
            ref={frame}
            key={`${String(chapter.index)}-${String(look.fontSize)}-${look.theme}`}
            title={t("reader.chapterFrame")}
            sandbox="allow-same-origin"
            srcDoc={chapter.html}
            onLoad={onLoad}
            className="block w-full border-0"
            style={{ height: Math.max(height, 200) }}
          />
        ) : (
          <div className="mx-auto mt-24 size-10 animate-spin rounded-full border-2 border-current/20 border-t-current/80" />
        )}
        <div className="mx-auto flex max-w-[44em] items-center justify-between px-7 pt-2 pb-12 text-sm opacity-80">
          <button
            type="button"
            disabled={index <= 0}
            onClick={() => {
              go(index - 1, "end");
            }}
            className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 hover:bg-current/10 disabled:opacity-30"
          >
            <ChevronLeft className="size-4" />
            {t("reader.previousChapter")}
          </button>
          <span className="tabular-nums">{t("reader.section", { n: index + 1, count })}</span>
          <button
            type="button"
            disabled={index >= count - 1}
            onClick={() => {
              go(index + 1);
            }}
            className="inline-flex items-center gap-1 rounded-full px-3 py-1.5 hover:bg-current/10 disabled:opacity-30"
          >
            {t("reader.nextChapter")}
            <ChevronRight className="size-4" />
          </button>
        </div>
      </div>
      {toc.length > 0 ? (
        <button
          type="button"
          onClick={() => {
            setTocOpen((v) => !v);
          }}
          aria-expanded={tocOpen}
          aria-label={t("reader.contents")}
          className="absolute top-3 left-3 rounded-full bg-black/30 p-2 text-white hover:bg-black/50"
        >
          <ListTree className="size-5" />
        </button>
      ) : null}
      {tocOpen ? (
        <nav
          aria-label={t("reader.contents")}
          className="absolute top-14 bottom-3 left-3 w-80 overflow-y-auto rounded-xl border border-white/10 bg-popover/95 p-2 text-popover-foreground shadow-2xl backdrop-blur"
        >
          <ul>
            {toc.map((e, i) => (
              <li key={`${e.path}-${String(i)}`}>
                <button
                  type="button"
                  onClick={() => {
                    setTocOpen(false);
                    follow(e.path);
                  }}
                  className={`w-full rounded-lg px-3 py-1.5 text-left text-sm hover:bg-white/10 ${e.path === current ? "font-semibold text-primary" : ""}`}
                  style={{ paddingLeft: `${String(0.75 + e.depth)}rem` }}
                >
                  {e.label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
      ) : null}
    </div>
  );
}
