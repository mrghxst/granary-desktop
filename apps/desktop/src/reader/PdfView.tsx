import { ChevronLeft, ChevronRight } from "lucide-react";
import { GlobalWorkerOptions, getDocument, type PDFDocumentProxy, type RenderTask } from "pdfjs-dist";
import workerUrl from "pdfjs-dist/build/pdf.worker.min.mjs?url";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { type BookInfo, books } from "../lib/books";
import { errorMessage } from "../lib/errors";
import { pageFor } from "./look";

// The worker is bundled with the app (CSP `script-src 'self'`); nothing is loaded from the network (ADR-0013).
GlobalWorkerOptions.workerSrc = workerUrl;

/** A PDF, one page at a time, fitted to the window (pdf.js). */
export function PdfView({
  book,
  start,
  onPosition,
}: {
  book: BookInfo;
  start: number;
  onPosition: (fraction: number) => void;
}) {
  const { t } = useTranslation();
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [page, setPage] = useState(0);
  const [size, setSize] = useState({ w: 0, h: 0 });
  const frame = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const report = useRef(onPosition);
  useEffect(() => {
    report.current = onPosition;
  });

  useEffect(() => {
    const live = { alive: true };
    const isAlive = () => live.alive;
    const task = { current: null as ReturnType<typeof getDocument> | null };
    void (async () => {
      try {
        const data = new Uint8Array(await books.entry(book.handle, ""));
        if (!isAlive()) return;
        task.current = getDocument({ data, enableXfa: false });
        const pdf = await task.current.promise;
        if (!isAlive()) return;
        setPage(pageFor(start, pdf.numPages));
        setDoc(pdf);
      } catch (e) {
        if (isAlive()) setError(e);
      }
    })();
    return () => {
      live.alive = false;
      void task.current?.destroy();
    };
    // `start` only applies when the book opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [book.handle]);

  useEffect(() => {
    const el = frame.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      setSize({ w: el.clientWidth, h: el.clientHeight });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
    };
  }, [doc]);

  useEffect(() => {
    if (!doc || !canvas.current || size.w === 0) return;
    let task: RenderTask | null = null;
    let alive = true;
    void doc.getPage(page + 1).then((p) => {
      const c = canvas.current;
      if (!alive || !c) return;
      const base = p.getViewport({ scale: 1 });
      const fit = Math.min((size.w - 32) / base.width, (size.h - 32) / base.height);
      const dpr = window.devicePixelRatio || 1;
      const viewport = p.getViewport({ scale: Math.max(0.1, fit) * dpr });
      c.width = Math.floor(viewport.width);
      c.height = Math.floor(viewport.height);
      c.style.width = `${String(Math.floor(viewport.width / dpr))}px`;
      c.style.height = `${String(Math.floor(viewport.height / dpr))}px`;
      task = p.render({ canvas: c, viewport });
      task.promise.catch(() => undefined);
    });
    return () => {
      alive = false;
      task?.cancel();
    };
  }, [doc, page, size]);

  const count = doc?.numPages ?? 0;
  useEffect(() => {
    if (count > 0) report.current((page + 1) / count);
  }, [page, count]);

  const go = (delta: number) => {
    setPage((p) => Math.min(Math.max(0, count - 1), Math.max(0, p + delta)));
  };
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowLeft" || e.key === "PageUp") go(-1);
      else if (e.key === "ArrowRight" || e.key === "PageDown" || e.key === " ") go(1);
      else if (e.key === "Home") setPage(0);
      else if (e.key === "End") setPage(Math.max(0, count - 1));
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  });

  if (error) return <p className="p-10 text-center text-destructive">{errorMessage(t, error)}</p>;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div ref={frame} className="flex min-h-0 flex-1 items-center justify-center bg-neutral-800">
        {doc ? (
          <canvas ref={canvas} className="bg-white shadow-2xl" />
        ) : (
          <div className="size-10 animate-spin rounded-full border-2 border-white/20 border-t-white/80" />
        )}
      </div>
      <div className="flex items-center justify-center gap-4 border-t border-white/10 bg-black/80 px-4 py-2 text-sm text-white/80">
        <button
          type="button"
          onClick={() => {
            go(-1);
          }}
          aria-label={t("reader.previousPage")}
          className="rounded-full p-1.5 hover:bg-white/10"
        >
          <ChevronLeft className="size-5" />
        </button>
        {count > 0 ? (
          <input
            type="range"
            min={0}
            max={count - 1}
            value={page}
            onChange={(e) => {
              setPage(Number(e.target.value));
            }}
            aria-label={t("reader.position")}
            className="w-64 accent-primary"
          />
        ) : null}
        <span className="w-28 text-center tabular-nums">
          {count > 0 ? t("reader.page", { n: page + 1, count }) : ""}
        </span>
        <button
          type="button"
          onClick={() => {
            go(1);
          }}
          aria-label={t("reader.nextPage")}
          className="rounded-full p-1.5 hover:bg-white/10"
        >
          <ChevronRight className="size-5" />
        </button>
      </div>
    </div>
  );
}
