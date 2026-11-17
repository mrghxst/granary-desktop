import { ArrowLeft, ChevronRight, Minus, Plus, Repeat2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { Alert } from "../components/ui";
import { type BookInfo, books, eventTime, fractionLocation, readerFile, restoreFraction } from "../lib/books";
import { catalog } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { useLoad } from "../lib/useLoad";
import { ComicView } from "./ComicView";
import { EpubView } from "./EpubView";
import { FONT_SIZES, loadLook, type ReaderLook, type ReaderTheme, saveLook } from "./look";
import { PdfView } from "./PdfView";

const THEMES: ReaderTheme[] = ["dark", "sepia", "light"];

/** The reader route (`/servers/:serverId/read/:contentId`), remounted per book. */
export function Reader() {
  const { serverId = "", contentId = "" } = useParams();
  return <ReaderPage key={`${serverId}/${contentId}`} serverId={serverId} contentId={contentId} />;
}

function ReaderPage({ serverId, contentId }: { serverId: string; contentId: string }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const { data: item, error } = useLoad(`${serverId}/${contentId}/reader`, () => catalog.item(serverId, contentId));
  const { data: series } = useLoad(item?.series_id ? `${serverId}/${item.series_id}/reader-series` : null, () =>
    catalog.item(serverId, item?.series_id ?? ""),
  );
  const file = item ? readerFile(item.versions, params.get("file")) : undefined;
  const [opened, setOpened] = useState<{ book: BookInfo; start: number } | null>(null);
  const [openError, setOpenError] = useState<unknown>(null);
  const [fraction, setFraction] = useState(0);
  const [look, setLook] = useState<ReaderLook>(loadLook);
  const fileId = file?.file_id;
  // Progress: saved a second after the reader settles, and when it closes. The event time is taken when the
  // position changes and kept for that save (the server orders saves by it).
  const pending = useRef<{ fraction: number; at: string } | null>(null);
  const last = useRef(0);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!fileId) return;
    let alive = true;
    let handle: string | null = null;
    void Promise.all([
      books.open(serverId, contentId, fileId),
      books.progress(serverId, contentId).catch(() => null),
    ]).then(
      ([book, saved]) => {
        handle = book.handle;
        if (!alive) {
          void books.close(book.handle).catch(() => undefined);
          return;
        }
        const start = restoreFraction(saved);
        last.current = start;
        setFraction(start);
        setOpened({ book, start });
      },
      (e: unknown) => {
        if (alive) setOpenError(e);
      },
    );
    return () => {
      alive = false;
      if (handle) void books.close(handle).catch(() => undefined);
    };
  }, [serverId, contentId, fileId]);

  const flush = useCallback(() => {
    const p = pending.current;
    if (!p || !fileId) return;
    pending.current = null;
    last.current = p.fraction;
    void books
      .saveProgress(serverId, contentId, fileId, fractionLocation(p.fraction), p.fraction, p.at)
      .catch(() => undefined);
  }, [serverId, contentId, fileId]);
  const onPosition = useCallback(
    (f: number) => {
      setFraction(f);
      if (Math.abs(f - last.current) < 0.0005 && pending.current === null) return;
      pending.current = { fraction: f, at: eventTime() };
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(flush, 1000);
    },
    [flush],
  );
  useEffect(
    () => () => {
      window.clearTimeout(timer.current);
      flush();
    },
    [flush],
  );

  const updateLook = (next: Partial<ReaderLook>) => {
    setLook((l) => {
      const v = { ...l, ...next };
      saveLook(v);
      return v;
    });
  };

  // The next chapter of a manga series, when this book is one.
  const next = useMemo(() => {
    const chapters = series?.manga?.chapters ?? [];
    const i = chapters.findIndex((c) => c.content_id === contentId);
    return i >= 0 ? chapters[i + 1] : undefined;
  }, [series, contentId]);

  const close = () => {
    if (window.history.length > 1) void navigate(-1);
    else void navigate(`/servers/${serverId}/item/${encodeURIComponent(contentId)}`);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  });

  const title = series && item ? `${series.title} · ${item.title}` : (item?.title ?? "");
  const failure = error ?? openError;
  const format = opened?.book.format;
  return (
    <div className="fixed inset-0 z-40 flex flex-col bg-background text-foreground">
      <header className="flex h-12 shrink-0 items-center gap-3 border-b border-border/60 bg-background/95 px-3">
        <button
          type="button"
          onClick={close}
          aria-label={t("reader.close")}
          className="rounded-full p-2 hover:bg-white/10"
        >
          <ArrowLeft className="size-5" />
        </button>
        <h1 className="min-w-0 flex-1 truncate text-sm font-medium">{title}</h1>
        {format === "epub" ? (
          <div className="flex items-center gap-1">
            <button
              type="button"
              aria-label={t("reader.smaller")}
              disabled={look.fontSize <= (FONT_SIZES[0] ?? 80)}
              onClick={() => {
                updateLook({
                  fontSize: FONT_SIZES[Math.max(0, FONT_SIZES.indexOf(look.fontSize) - 1)] ?? look.fontSize,
                });
              }}
              className="rounded-full p-2 hover:bg-white/10 disabled:opacity-30"
            >
              <Minus className="size-4" />
            </button>
            <button
              type="button"
              aria-label={t("reader.larger")}
              disabled={look.fontSize >= (FONT_SIZES[FONT_SIZES.length - 1] ?? 170)}
              onClick={() => {
                updateLook({
                  fontSize:
                    FONT_SIZES[Math.min(FONT_SIZES.length - 1, FONT_SIZES.indexOf(look.fontSize) + 1)] ?? look.fontSize,
                });
              }}
              className="rounded-full p-2 hover:bg-white/10 disabled:opacity-30"
            >
              <Plus className="size-4" />
            </button>
            <select
              value={look.theme}
              onChange={(e) => {
                updateLook({ theme: e.target.value as ReaderTheme });
              }}
              aria-label={t("reader.theme")}
              className="rounded-lg border border-border bg-background px-2 py-1 text-sm"
            >
              {THEMES.map((th) => (
                <option key={th} value={th}>
                  {t(`reader.themes.${th}`)}
                </option>
              ))}
            </select>
          </div>
        ) : null}
        {format === "comic" ? (
          <button
            type="button"
            aria-pressed={look.rtl}
            onClick={() => {
              updateLook({ rtl: !look.rtl });
            }}
            className={`inline-flex items-center gap-1.5 rounded-full px-3 py-1.5 text-sm hover:bg-white/10 ${look.rtl ? "bg-white/10 text-primary" : ""}`}
          >
            <Repeat2 className="size-4" />
            {t("reader.rtl")}
          </button>
        ) : null}
        {opened ? (
          <span className="w-12 text-right text-sm text-muted-foreground tabular-nums">
            {Math.round(fraction * 100)}%
          </span>
        ) : null}
        {next ? (
          <button
            type="button"
            onClick={() => {
              flush();
              void navigate(`/servers/${serverId}/read/${encodeURIComponent(next.content_id)}`, { replace: true });
            }}
            className="inline-flex items-center gap-1 rounded-full bg-white/10 px-3 py-1.5 text-sm hover:bg-white/15"
          >
            {t("reader.nextChapter")}
            <ChevronRight className="size-4" />
          </button>
        ) : null}
      </header>
      {failure ? (
        <div className="mx-auto mt-16 w-full max-w-lg">
          <Alert>{errorMessage(t, failure)}</Alert>
        </div>
      ) : item && !file ? (
        <div className="mx-auto mt-16 w-full max-w-lg">
          <Alert tone="warning">{t("reader.noFile")}</Alert>
        </div>
      ) : !opened ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
          <div className="size-10 animate-spin rounded-full border-2 border-white/20 border-t-white/80" />
          {t("reader.opening")}
        </div>
      ) : opened.book.format === "epub" ? (
        <EpubView book={opened.book} start={opened.start} look={look} onPosition={onPosition} />
      ) : opened.book.format === "pdf" ? (
        <PdfView book={opened.book} start={opened.start} onPosition={onPosition} />
      ) : (
        <ComicView book={opened.book} start={opened.start} rtl={look.rtl} onPosition={onPosition} />
      )}
    </div>
  );
}
