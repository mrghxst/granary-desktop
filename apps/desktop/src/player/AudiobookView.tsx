import { ArrowLeft, ListOrdered, Moon, Pause, Play, RotateCcw, RotateCw, SkipBack, SkipForward } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { BOOK_SPEEDS, bookChapters, bookDuration, bookParts, chapterAt, partAt } from "../lib/audiobook";
import { artwork, catalog, type CatalogItemDetail } from "../lib/catalog";
import { formatTime } from "../lib/play";
import { player } from "../lib/player";
import { SLEEP_MINUTES, SLEEP_OFF, sleepCountdown } from "../lib/sleepTimer";
import { usePlayer } from "./context";

/** Seconds the skip buttons and arrow keys move. */
const SKIP_SECONDS = 30;
/** How often the book position is saved while it plays (as the web player). */
const SAVE_MS = 10_000;

/**
 * The audiobook player (after `silo-server/web/src/pages/audiobooks/player@749db96a`): one timeline across the book's
 * parts, its chapters, speed, sleep timer and skips. mpv plays the current part; the next one starts when it ends.
 * The book position is saved with `syncProgress` every 10 s while playing, on pause and on leaving.
 */
export function AudiobookView() {
  const { t } = useTranslation();
  const { state, ctx, start, toast, sleep, setSleep } = usePlayer();
  const [detail, setDetail] = useState<CatalogItemDetail | null>(null);
  const [showChapters, setShowChapters] = useState(false);

  const serverId = ctx?.serverId ?? "";
  const contentId = ctx?.watch.content_id ?? "";
  const versions = useMemo(() => ctx?.watch.versions ?? [], [ctx]);
  const parts = useMemo(() => bookParts(versions), [versions]);
  const chapters = useMemo(() => bookChapters(versions, parts, (n) => t("book.chapterN", { n })), [versions, parts, t]);
  const index = parts.findIndex((p) => p.fileId === ctx?.version.file_id);
  const total = bookDuration(parts);
  const at = Math.min(total, (parts[index]?.start ?? 0) + Math.max(0, state.position));
  const current = chapterAt(chapters, at);
  const speed = typeof state.props.speed === "number" ? state.props.speed : 1;

  useEffect(() => {
    void player.overlay(true).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!serverId || !contentId) return;
    let alive = true;
    catalog
      .item(serverId, contentId)
      .then((d) => {
        if (alive) setDetail(d);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [serverId, contentId]);

  // The book position, saved while playing, on pause and when the view goes (stop, end of the book).
  const where = useRef({ at, total, serverId, contentId });
  useEffect(() => {
    where.current = { at, total, serverId, contentId };
  });
  const save = () => {
    const w = where.current;
    if (!w.serverId || !w.contentId || w.total <= 0) return;
    void catalog.titleProgress(w.serverId, w.contentId, w.at, w.total).catch(() => undefined);
  };
  const saveRef = useRef(save);
  useEffect(() => {
    saveRef.current = save;
  });
  useEffect(() => {
    if (state.paused) {
      saveRef.current();
      return;
    }
    const id = setInterval(() => {
      saveRef.current();
    }, SAVE_MS);
    return () => {
      clearInterval(id);
    };
  }, [state.paused]);
  useEffect(
    () => () => {
      saveRef.current();
    },
    [],
  );

  const seekBook = (to: number) => {
    if (!ctx || parts.length === 0) return;
    const target = Math.max(0, Math.min(to, Math.max(0, total - 1)));
    const { index: i, local } = partAt(parts, target);
    const part = parts[i];
    if (!part) return;
    if (i === index) {
      void player.seek(local).catch(() => undefined);
      return;
    }
    void start(serverId, contentId, { watch: ctx.watch, fileId: part.fileId, startAt: local, continuing: true }).catch(
      () => undefined,
    );
  };
  const seekRef = useRef(seekBook);
  useEffect(() => {
    seekRef.current = seekBook;
  });

  // Space plays and pauses, the arrows skip, Escape leaves.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
      if (e.key === " ") {
        e.preventDefault();
        void player.pause().catch(() => undefined);
      } else if (e.key === "ArrowLeft") {
        seekRef.current(where.current.at - SKIP_SECONDS);
      } else if (e.key === "ArrowRight") {
        seekRef.current(where.current.at + SKIP_SECONDS);
      } else if (e.key === "Escape") {
        void player.stop().catch(() => undefined);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const poster = artwork(serverId, detail?.poster_url);
  const people = (list: { name: string }[] | undefined) => (list ?? []).map((p) => p.name).join(", ");
  const authors = people(detail?.audiobook?.authors);
  const narrators = people(detail?.audiobook?.narrators);
  const chapter = chapters[current];
  const prevChapter = () => {
    const c = chapters[current];
    // Within the first seconds of a chapter, back goes to the previous one; later, to this one's start.
    const back = c && at - c.start > 3 ? c : chapters[Math.max(0, current - 1)];
    if (back) seekBook(back.start);
  };
  const nextChapter = () => {
    const n = chapters[current + 1];
    if (n) seekBook(n.start);
  };
  const button =
    "grid place-items-center rounded-full p-3 text-foreground transition hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-40";

  return (
    <div
      className="fixed inset-0 z-40 flex flex-col overflow-hidden bg-background text-foreground"
      data-testid="audiobook-view"
    >
      {poster ? (
        <img
          src={poster}
          alt=""
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 size-full scale-110 object-cover opacity-25 blur-3xl"
        />
      ) : null}
      <div className="relative flex items-center gap-3 p-4">
        <button
          type="button"
          aria-label={t("player.back")}
          onClick={() => {
            void player.stop().catch(() => undefined);
          }}
          className={button}
        >
          <ArrowLeft className="size-5" />
        </button>
        <span className="text-sm text-muted-foreground">{t("book.nowListening")}</span>
      </div>

      <div className="relative flex min-h-0 flex-1 flex-col items-center justify-center gap-6 px-6 pb-8 lg:flex-row lg:gap-12">
        <div className="flex w-full max-w-md flex-col items-center gap-5">
          <div className="aspect-square w-full max-w-[18rem] overflow-hidden rounded-xl bg-surface shadow-2xl">
            {poster ? <img src={poster} alt="" className="size-full object-cover" /> : null}
          </div>
          <div className="w-full text-center">
            <h1 className="text-xl font-semibold">{ctx?.watch.title}</h1>
            {authors ? <p className="text-sm text-muted-foreground">{authors}</p> : null}
            {narrators ? (
              <p className="text-xs text-muted-foreground">{t("book.narratedBy", { names: narrators })}</p>
            ) : null}
            {chapter ? <p className="mt-2 text-sm font-medium">{chapter.title}</p> : null}
            {parts.length > 1 ? (
              <p className="text-xs text-muted-foreground">{t("book.part", { n: index + 1, count: parts.length })}</p>
            ) : null}
          </div>

          <div className="flex w-full flex-col gap-1">
            <input
              type="range"
              aria-label={t("book.position")}
              min={0}
              max={Math.max(1, total)}
              step={1}
              value={Math.floor(at)}
              onChange={(e) => {
                seekBook(Number(e.target.value));
              }}
              className="w-full accent-primary"
            />
            <div className="flex justify-between text-xs text-muted-foreground tabular-nums">
              <span>{formatTime(at)}</span>
              <span>-{formatTime(Math.max(0, total - at) / Math.max(0.1, speed))}</span>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button type="button" aria-label={t("book.previousChapter")} onClick={prevChapter} className={button}>
              <SkipBack className="size-5" />
            </button>
            <button
              type="button"
              aria-label={t("book.back30")}
              onClick={() => {
                seekBook(at - SKIP_SECONDS);
              }}
              className={button}
            >
              <RotateCcw className="size-6" />
            </button>
            <button
              type="button"
              aria-label={state.paused ? t("player.play") : t("player.pause")}
              onClick={() => {
                void player.pause().catch(() => undefined);
              }}
              className="grid size-16 place-items-center rounded-full bg-primary text-primary-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              {state.paused ? <Play className="size-7" /> : <Pause className="size-7" />}
            </button>
            <button
              type="button"
              aria-label={t("book.forward30")}
              onClick={() => {
                seekBook(at + SKIP_SECONDS);
              }}
              className={button}
            >
              <RotateCw className="size-6" />
            </button>
            <button
              type="button"
              aria-label={t("book.nextChapter")}
              onClick={nextChapter}
              disabled={current + 1 >= chapters.length}
              className={button}
            >
              <SkipForward className="size-5" />
            </button>
          </div>

          <div className="flex flex-wrap items-center justify-center gap-3 text-sm">
            <label className="flex items-center gap-2">
              <span className="text-muted-foreground">{t("book.speed")}</span>
              <select
                value={String(BOOK_SPEEDS.reduce((a, b) => (Math.abs(b - speed) < Math.abs(a - speed) ? b : a)))}
                onChange={(e) => {
                  void player.set("speed", Number(e.target.value)).catch(() => undefined);
                }}
                className="rounded-md border border-border bg-background px-2 py-1"
              >
                {BOOK_SPEEDS.map((s) => (
                  <option key={s} value={String(s)}>
                    {`${String(s)}×`}
                  </option>
                ))}
              </select>
            </label>
            <label className="flex items-center gap-2">
              <Moon className="size-4 text-muted-foreground" aria-hidden="true" />
              <span className="sr-only">{t("player.sleep")}</span>
              <select
                aria-label={t("player.sleep")}
                value={sleep.kind === "time" ? "time" : "off"}
                onChange={(e) => {
                  const v = e.target.value;
                  setSleep(v === "off" || v === "time" ? SLEEP_OFF : { kind: "time", remainingMs: Number(v) * 60_000 });
                }}
                className="rounded-md border border-border bg-background px-2 py-1"
              >
                <option value="off">{t("player.sleepOff")}</option>
                {sleep.kind === "time" ? <option value="time">{sleepCountdown(sleep.remainingMs)}</option> : null}
                {SLEEP_MINUTES.map((m) => (
                  <option key={m} value={String(m)}>
                    {t("player.sleepMinutes", { count: m })}
                  </option>
                ))}
              </select>
            </label>
            <button
              type="button"
              aria-pressed={showChapters}
              onClick={() => {
                setShowChapters((s) => !s);
              }}
              className="flex items-center gap-2 rounded-md border border-border px-3 py-1 hover:bg-surface-hover"
            >
              <ListOrdered className="size-4" aria-hidden="true" />
              {t("book.chapters", { count: chapters.length })}
            </button>
          </div>
        </div>

        {showChapters ? (
          <ol
            aria-label={t("book.chapters", { count: chapters.length })}
            className="max-h-[60vh] w-full max-w-md overflow-y-auto rounded-xl border border-border bg-card/80 p-2"
          >
            {chapters.map((c, i) => (
              <li key={`${String(c.part)}-${String(c.start)}`}>
                <button
                  type="button"
                  aria-current={i === current ? "true" : undefined}
                  onClick={() => {
                    seekBook(c.start);
                  }}
                  className={`flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm hover:bg-surface-hover ${
                    i === current ? "bg-surface-hover font-semibold" : ""
                  }`}
                >
                  <span className="truncate">{c.title}</span>
                  <span className="shrink-0 text-xs text-muted-foreground tabular-nums">{formatTime(c.start)}</span>
                </button>
              </li>
            ))}
          </ol>
        ) : null}
      </div>
      {toast ? (
        <div
          role="status"
          className="absolute inset-x-0 bottom-6 mx-auto w-fit rounded-full bg-card px-4 py-2 text-sm shadow-lg"
        >
          {toast}
        </div>
      ) : null}
    </div>
  );
}
