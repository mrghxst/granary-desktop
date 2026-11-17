import { ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type BookInfo, books, comicPages, mimeFor } from "../lib/books";
import { pageFor } from "./look";

/** A comic or manga chapter (CBZ): one page at a time, fitted to the window, optionally right to left. */
export function ComicView({
  book,
  start,
  rtl,
  onPosition,
}: {
  book: BookInfo;
  start: number;
  rtl: boolean;
  onPosition: (fraction: number) => void;
}) {
  const { t } = useTranslation();
  const pages = useMemo(() => comicPages(book.entries), [book.entries]);
  const [page, setPage] = useState(() => pageFor(start, pages.length));
  const [urls, setUrls] = useState<Record<number, string>>({});
  const cache = useRef(new Map<number, string>());
  const report = useRef(onPosition);
  useEffect(() => {
    report.current = onPosition;
  });

  // Keep the current page and its neighbours decoded; release the rest.
  useEffect(() => {
    let alive = true;
    const want = [page, page + 1, page + 2, page - 1].filter((i) => i >= 0 && i < pages.length);
    for (const [i, url] of cache.current) {
      if (i < page - 2 || i > page + 3) {
        URL.revokeObjectURL(url);
        cache.current.delete(i);
      }
    }
    void Promise.all(
      want.map(async (i) => {
        const name = pages[i];
        if (cache.current.has(i) || !name) return;
        const bytes = await books.entry(book.handle, name);
        if (!alive) return;
        cache.current.set(i, URL.createObjectURL(new Blob([bytes], { type: mimeFor(name) })));
      }),
    )
      .catch(() => undefined)
      .then(() => {
        if (alive) setUrls(Object.fromEntries(cache.current));
      });
    return () => {
      alive = false;
    };
  }, [book.handle, page, pages]);

  useEffect(() => {
    const map = cache.current;
    return () => {
      for (const url of map.values()) URL.revokeObjectURL(url);
      map.clear();
    };
  }, []);

  useEffect(() => {
    if (pages.length > 0) report.current((page + 1) / pages.length);
  }, [page, pages.length]);

  const go = (delta: number) => {
    setPage((p) => Math.min(pages.length - 1, Math.max(0, p + delta)));
  };
  // In right-to-left reading the left side moves forward.
  const left = () => {
    go(rtl ? 1 : -1);
  };
  const right = () => {
    go(rtl ? -1 : 1);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft") left();
      else if (e.key === "ArrowRight") right();
      else if (e.key === "PageDown" || e.key === " ") go(1);
      else if (e.key === "PageUp") go(-1);
      else if (e.key === "Home") setPage(0);
      else if (e.key === "End") setPage(pages.length - 1);
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  });

  if (pages.length === 0) return <p className="p-10 text-center text-muted-foreground">{t("reader.empty")}</p>;
  const url = urls[page];
  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div className="relative flex min-h-0 flex-1 items-center justify-center bg-black">
        {url ? (
          <img
            src={url}
            alt={t("reader.page", { n: page + 1, count: pages.length })}
            className="max-h-full max-w-full object-contain select-none"
            draggable={false}
          />
        ) : (
          <div className="size-10 animate-spin rounded-full border-2 border-white/20 border-t-white/80" />
        )}
        <button
          type="button"
          aria-label={t(rtl ? "reader.nextPage" : "reader.previousPage")}
          onClick={left}
          className="absolute inset-y-0 left-0 w-1/3 cursor-w-resize focus:outline-none"
        />
        <button
          type="button"
          aria-label={t(rtl ? "reader.previousPage" : "reader.nextPage")}
          onClick={right}
          className="absolute inset-y-0 right-0 w-1/3 cursor-e-resize focus:outline-none"
        />
      </div>
      <div className="flex items-center justify-center gap-4 border-t border-white/10 bg-black/80 px-4 py-2 text-sm text-white/80">
        <button
          type="button"
          onClick={left}
          aria-label={t(rtl ? "reader.nextPage" : "reader.previousPage")}
          className="rounded-full p-1.5 hover:bg-white/10"
        >
          <ChevronLeft className="size-5" />
        </button>
        <input
          type="range"
          min={0}
          max={pages.length - 1}
          value={rtl ? pages.length - 1 - page : page}
          onChange={(e) => {
            const v = Number(e.target.value);
            setPage(rtl ? pages.length - 1 - v : v);
          }}
          aria-label={t("reader.position")}
          className="w-64 accent-primary"
          dir="ltr"
        />
        <span className="w-28 text-center tabular-nums">{t("reader.page", { n: page + 1, count: pages.length })}</span>
        <button
          type="button"
          onClick={right}
          aria-label={t(rtl ? "reader.previousPage" : "reader.nextPage")}
          className="rounded-full p-1.5 hover:bg-white/10"
        >
          <ChevronRight className="size-5" />
        </button>
      </div>
    </div>
  );
}
