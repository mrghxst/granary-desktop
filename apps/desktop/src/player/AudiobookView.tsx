import {
  ArrowLeft,
  Check,
  Gauge,
  ListOrdered,
  Maximize,
  Minimize,
  Moon,
  Pause,
  Play,
  RotateCcw,
  RotateCw,
  SkipBack,
  SkipForward,
  X,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  BOOK_SPEEDS,
  bookChapters,
  bookDuration,
  bookParts,
  chapterAt,
  partAt,
  type BookChapter,
} from "../lib/audiobook";
import { artwork, catalog, type CatalogItemDetail } from "../lib/catalog";
import { formatTime } from "../lib/play";
import { player } from "../lib/player";
import { SLEEP_MINUTES, SLEEP_OFF, sleepCountdown } from "../lib/sleepTimer";
import { usePlayer } from "./context";

/** Seconds the skip buttons and arrow keys move. */
const SKIP_SECONDS = 30;
/** How often the book position is saved while it plays (as the web player). */
const SAVE_MS = 10_000;
/** More chapters than this and the ticks on the bar would only be noise. */
const MAX_TICKS = 80;

type Menu = "speed" | "sleep" | null;

/**
 * The audiobook player (after `silo-server/web/src/pages/audiobooks/player@749db96a`): one timeline across the book's
 * parts, its chapters, speed, sleep timer and skips. mpv plays the current part; the next one starts when it ends.
 * The book position is saved with `syncProgress` every 10 s while playing, on pause and on leaving.
 *
 * Unlike the video player it never goes full screen by itself; the full-screen button (or F) does it on request.
 */
export function AudiobookView() {
  const { t } = useTranslation();
  const { state, ctx, start, toast, sleep, setSleep, window: win, setWindow } = usePlayer();
  const [detail, setDetail] = useState<CatalogItemDetail | null>(null);
  const [showChapters, setShowChapters] = useState(false);
  const [menu, setMenu] = useState<Menu>(null);

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
  const nearestSpeed = BOOK_SPEEDS.reduce((a, b) => (Math.abs(b - speed) < Math.abs(a - speed) ? b : a));

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
  const toggleFullscreen = (on?: boolean) => {
    void player
      .fullscreen(on)
      .then(setWindow)
      .catch(() => undefined);
  };

  // Space plays and pauses, the arrows skip, F toggles full screen. Escape closes the open menu or panel first, then
  // leaves full screen, then leaves the book.
  const keyRef = useRef<(e: KeyboardEvent) => void>(() => undefined);
  useEffect(() => {
    keyRef.current = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLSelectElement) return;
      if (e.ctrlKey || e.altKey || e.metaKey) return;
      if (e.key === " ") {
        e.preventDefault();
        void player.pause().catch(() => undefined);
      } else if (e.key === "ArrowLeft") {
        seekBook(where.current.at - SKIP_SECONDS);
      } else if (e.key === "ArrowRight") {
        seekBook(where.current.at + SKIP_SECONDS);
      } else if (e.key === "f" || e.key === "F") {
        toggleFullscreen();
      } else if (e.key === "Escape") {
        if (menu) setMenu(null);
        else if (showChapters) setShowChapters(false);
        else if (win.fullscreen) toggleFullscreen(false);
        else void player.stop().catch(() => undefined);
      }
    };
  });
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      keyRef.current(e);
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
  const chapterEnd = chapters[current + 1]?.start ?? total;
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
  const perSpeed = (s: number) => Math.max(0, s) / Math.max(0.1, speed);

  return (
    <div
      className="fixed inset-0 z-40 flex flex-col overflow-hidden bg-[#08080a] text-white select-none"
      data-testid="audiobook-view"
    >
      {/* Ambient backdrop: the cover, blurred and darkened, so every book gets its own colour. */}
      {poster ? (
        <img
          src={poster}
          alt=""
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 size-full scale-125 object-cover opacity-60 blur-[90px] saturate-150"
        />
      ) : null}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute inset-0 bg-[radial-gradient(ellipse_at_top,rgba(0,0,0,0.15),rgba(0,0,0,0.75)_70%)]"
      />

      <header className="relative flex items-center justify-between gap-3 px-5 pt-5">
        <GlassButton
          label={t("player.back")}
          onClick={() => {
            void player.stop().catch(() => undefined);
          }}
        >
          <ArrowLeft className="size-5" />
        </GlassButton>
        <span className="text-xs font-semibold tracking-[0.18em] text-white/55 uppercase">
          {t("book.nowListening")}
        </span>
        <div className="flex items-center gap-2">
          <GlassButton
            label={t("book.chaptersHeading")}
            active={showChapters}
            onClick={() => {
              setShowChapters((s) => !s);
            }}
          >
            <ListOrdered className="size-5" />
          </GlassButton>
          <GlassButton
            label={win.fullscreen ? t("player.exitFullscreen") : t("player.fullscreen")}
            onClick={() => {
              toggleFullscreen();
            }}
          >
            {win.fullscreen ? <Minimize className="size-5" /> : <Maximize className="size-5" />}
          </GlassButton>
        </div>
      </header>

      <main className="relative flex min-h-0 flex-1 items-center justify-center gap-12 px-6 pb-8">
        <section className="flex w-full max-w-xl flex-col items-center">
          <div className="aspect-square w-[min(20rem,38vh)] overflow-hidden rounded-2xl bg-white/5 shadow-[0_40px_100px_-20px_rgba(0,0,0,0.9)] ring-1 ring-white/10">
            {poster ? <img src={poster} alt="" className="size-full object-cover" /> : null}
          </div>

          <div className="mt-7 w-full text-center">
            <h1 className="line-clamp-2 text-2xl font-bold tracking-tight text-balance sm:text-3xl">
              {ctx?.watch.title}
            </h1>
            {authors ? <p className="mt-1.5 text-[15px] font-medium text-white/75">{authors}</p> : null}
            {narrators ? (
              <p className="mt-0.5 text-sm text-white/50">{t("book.narratedBy", { names: narrators })}</p>
            ) : null}
          </div>

          {chapter ? (
            <button
              type="button"
              onClick={() => {
                setShowChapters(true);
              }}
              className="mt-5 flex max-w-full items-center gap-2 rounded-full bg-white/[0.08] py-1.5 pr-4 pl-3 text-sm ring-1 ring-white/10 transition hover:bg-white/[0.14] focus-visible:outline-2 focus-visible:outline-white/70"
            >
              <ListOrdered className="size-4 shrink-0 text-white/60" aria-hidden="true" />
              <span className="truncate font-medium">{chapter.title}</span>
              <span className="shrink-0 text-white/50 tabular-nums">
                · {t("book.leftInChapter", { time: formatTime(perSpeed(chapterEnd - at)) })}
              </span>
            </button>
          ) : null}

          <div className="mt-6 w-full">
            <BookSeekBar
              position={at}
              duration={total}
              chapters={chapters}
              label={t("book.position")}
              onSeek={seekBook}
            />
            <div className="mt-1 flex items-center justify-between text-xs text-white/55 tabular-nums">
              <span>{formatTime(at)}</span>
              {parts.length > 1 ? (
                <span className="text-white/40">{t("book.part", { n: index + 1, count: parts.length })}</span>
              ) : null}
              <span>-{formatTime(perSpeed(total - at))}</span>
            </div>
          </div>

          <div className="mt-5 flex items-center gap-3 sm:gap-5">
            <TransportButton label={t("book.previousChapter")} onClick={prevChapter}>
              <SkipBack className="size-6" fill="currentColor" />
            </TransportButton>
            <TransportButton
              label={t("book.back30")}
              onClick={() => {
                seekBook(at - SKIP_SECONDS);
              }}
            >
              <SkipIcon>
                <RotateCcw className="size-9" strokeWidth={1.6} />
              </SkipIcon>
            </TransportButton>
            <button
              type="button"
              aria-label={state.paused ? t("player.play") : t("player.pause")}
              onClick={() => {
                void player.pause().catch(() => undefined);
              }}
              className="grid size-[4.5rem] place-items-center rounded-full bg-white text-black shadow-[0_12px_40px_-8px_rgba(0,0,0,0.7)] transition hover:scale-105 active:scale-95 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white/80"
            >
              {state.paused ? (
                <Play className="ml-1 size-8" fill="currentColor" />
              ) : (
                <Pause className="size-8" fill="currentColor" />
              )}
            </button>
            <TransportButton
              label={t("book.forward30")}
              onClick={() => {
                seekBook(at + SKIP_SECONDS);
              }}
            >
              <SkipIcon>
                <RotateCw className="size-9" strokeWidth={1.6} />
              </SkipIcon>
            </TransportButton>
            <TransportButton
              label={t("book.nextChapter")}
              onClick={nextChapter}
              disabled={current + 1 >= chapters.length}
            >
              <SkipForward className="size-6" fill="currentColor" />
            </TransportButton>
          </div>

          <div className="mt-7 flex flex-wrap items-center justify-center gap-2">
            <PillMenu
              open={menu === "speed"}
              onOpenChange={(o) => {
                setMenu(o ? "speed" : null);
              }}
              label={t("book.speed")}
              icon={<Gauge className="size-4" />}
              value={nearestSpeed === 1 ? t("player.speedNormal") : `${String(nearestSpeed)}×`}
              active={nearestSpeed !== 1}
            >
              {BOOK_SPEEDS.map((s) => (
                <MenuChoice
                  key={s}
                  selected={s === nearestSpeed}
                  onSelect={() => {
                    void player.set("speed", s).catch(() => undefined);
                    setMenu(null);
                  }}
                >
                  {s === 1 ? `1× · ${t("player.speedNormal")}` : `${String(s)}×`}
                </MenuChoice>
              ))}
            </PillMenu>
            <PillMenu
              open={menu === "sleep"}
              onOpenChange={(o) => {
                setMenu(o ? "sleep" : null);
              }}
              label={t("player.sleep")}
              icon={<Moon className="size-4" />}
              value={sleep.kind === "time" ? sleepCountdown(sleep.remainingMs) : t("player.sleepOff")}
              active={sleep.kind === "time"}
            >
              <MenuChoice
                selected={sleep.kind !== "time"}
                onSelect={() => {
                  setSleep(SLEEP_OFF);
                  setMenu(null);
                }}
              >
                {t("player.sleepOff")}
              </MenuChoice>
              {SLEEP_MINUTES.map((m) => (
                <MenuChoice
                  key={m}
                  selected={false}
                  onSelect={() => {
                    setSleep({ kind: "time", remainingMs: m * 60_000 });
                    setMenu(null);
                  }}
                >
                  {t("player.sleepMinutes", { count: m })}
                </MenuChoice>
              ))}
            </PillMenu>
          </div>
        </section>

        {showChapters ? (
          <ChapterPanel
            chapters={chapters}
            current={current}
            onPick={seekBook}
            onClose={() => {
              setShowChapters(false);
            }}
          />
        ) : null}
      </main>

      {toast ? (
        <div
          role="status"
          className="absolute inset-x-0 bottom-6 mx-auto w-fit rounded-full bg-black/70 px-4 py-2 text-sm shadow-lg ring-1 ring-white/10 backdrop-blur-xl"
        >
          {toast}
        </div>
      ) : null}
    </div>
  );
}

function GlassButton({
  label,
  onClick,
  active = false,
  children,
}: {
  label: string;
  onClick: () => void;
  active?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active || undefined}
      onClick={onClick}
      className={`grid size-10 place-items-center rounded-full ring-1 ring-white/10 backdrop-blur-xl transition focus-visible:outline-2 focus-visible:outline-white/70 ${
        active ? "bg-white text-black" : "bg-white/[0.08] text-white hover:bg-white/[0.16]"
      }`}
    >
      {children}
    </button>
  );
}

function TransportButton({
  label,
  onClick,
  disabled = false,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="grid size-14 place-items-center rounded-full text-white/90 transition hover:bg-white/10 hover:text-white active:scale-95 focus-visible:outline-2 focus-visible:outline-white/70 disabled:pointer-events-none disabled:opacity-30"
    >
      {children}
    </button>
  );
}

/** A circular-arrow skip icon with the seconds written inside it. */
function SkipIcon({ children }: { children: ReactNode }) {
  return (
    <span className="relative grid place-items-center">
      {children}
      <span className="absolute pt-px text-[10px] font-bold tabular-nums" aria-hidden="true">
        {SKIP_SECONDS}
      </span>
    </span>
  );
}

/** A pill showing a setting's current value; it opens a list of choices above it. */
function PillMenu({
  open,
  onOpenChange,
  label,
  icon,
  value,
  active,
  children,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  label: string;
  icon: ReactNode;
  value: string;
  active: boolean;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onOpenChange(false);
    };
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("mousedown", onDown);
    };
  }, [open, onOpenChange]);
  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={`${label}: ${value}`}
        onClick={() => {
          onOpenChange(!open);
        }}
        className={`flex items-center gap-2 rounded-full py-2 pr-4 pl-3.5 text-sm font-medium ring-1 backdrop-blur-xl transition focus-visible:outline-2 focus-visible:outline-white/70 ${
          open || active
            ? "bg-white/[0.16] text-white ring-white/25"
            : "bg-white/[0.07] text-white/80 ring-white/10 hover:bg-white/[0.13] hover:text-white"
        }`}
      >
        <span className={active ? "text-white" : "text-white/60"}>{icon}</span>
        <span className="text-white/55">{label}</span>
        <span className="tabular-nums">{value}</span>
      </button>
      {open ? (
        <div
          role="menu"
          aria-label={label}
          className="absolute bottom-full left-1/2 z-20 mb-2 max-h-[50vh] w-56 -translate-x-1/2 origin-bottom animate-[menu-in_160ms_cubic-bezier(0.2,0.8,0.2,1)] overflow-y-auto overscroll-contain rounded-2xl bg-[#0d0d10]/90 p-1.5 text-sm shadow-[0_30px_80px_-24px_rgba(0,0,0,0.95)] ring-1 ring-white/10 backdrop-blur-2xl"
        >
          {children}
        </div>
      ) : null}
    </div>
  );
}

function MenuChoice({
  selected,
  onSelect,
  children,
}: {
  selected: boolean;
  onSelect: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left font-medium transition-colors focus-visible:outline-2 focus-visible:outline-white/70 ${
        selected ? "bg-white/[0.12] text-white" : "text-white/80 hover:bg-white/[0.07] hover:text-white"
      }`}
    >
      <Check className={`size-4 shrink-0 ${selected ? "opacity-100" : "opacity-0"}`} />
      {children}
    </button>
  );
}

/** The book's timeline: one bar across all parts, chapter ticks, a hover label with chapter and time, drag to seek. */
function BookSeekBar({
  position,
  duration,
  chapters,
  label,
  onSeek,
}: {
  position: number;
  duration: number;
  chapters: readonly BookChapter[];
  label: string;
  onSeek: (seconds: number) => void;
}) {
  const bar = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [drag, setDrag] = useState<number | null>(null);
  const at = (clientX: number) => {
    const r = bar.current?.getBoundingClientRect();
    if (!r || r.width <= 0) return 0;
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  };
  const frac = (s: number) => (duration > 0 ? Math.min(1, Math.max(0, s / duration)) : 0);
  const pct = (s: number) => `${String(frac(s) * 100)}%`;
  const shown = drag ?? position;
  const preview = drag ?? hover;
  const previewChapter = preview !== null ? chapters[chapterAt(chapters, preview)] : undefined;
  const ticks = chapters.length <= MAX_TICKS ? chapters.filter((c) => c.start > 0) : [];

  return (
    <div className="group relative py-2.5">
      {preview !== null ? (
        <div
          className="pointer-events-none absolute bottom-7 max-w-64 -translate-x-1/2 truncate rounded-lg bg-black/85 px-2.5 py-1 text-xs whitespace-nowrap shadow-lg ring-1 ring-white/10 tabular-nums"
          style={{ left: `clamp(4rem, ${pct(preview)}, calc(100% - 4rem))` }}
        >
          {previewChapter ? `${previewChapter.title} · ` : ""}
          {formatTime(preview)}
        </div>
      ) : null}
      <div
        ref={bar}
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(shown)}
        aria-valuetext={`${formatTime(shown)} / ${formatTime(duration)}`}
        className="relative h-1.5 cursor-pointer rounded-full bg-white/20 transition-[height] group-hover:h-2 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-white/70"
        onPointerMove={(e) => {
          const s = at(e.clientX);
          setHover(s);
          if (drag !== null) setDrag(s);
        }}
        onPointerLeave={() => {
          setHover(null);
        }}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDrag(at(e.clientX));
        }}
        onPointerUp={(e) => {
          const s = at(e.clientX);
          setDrag(null);
          onSeek(s);
        }}
        onKeyDown={(e) => {
          // The arrows are handled by the view (30 s skips); Home and End jump to the ends.
          if (e.key === "Home") onSeek(0);
          else if (e.key === "End") onSeek(duration);
        }}
      >
        <div className="absolute inset-0 overflow-hidden rounded-full">
          <div className="absolute inset-y-0 left-0 rounded-full bg-white" style={{ width: pct(shown) }} />
          {ticks.map((c) => (
            <div
              key={`${String(c.part)}-${String(c.start)}`}
              className="absolute inset-y-0 w-0.5 bg-black/55"
              style={{ left: pct(c.start) }}
            />
          ))}
        </div>
        <div
          className={`absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white shadow-[0_0_0_4px_rgba(255,255,255,0.15)] transition-opacity ${
            drag !== null ? "opacity-100" : "opacity-0 group-hover:opacity-100"
          }`}
          style={{ left: pct(shown) }}
        />
      </div>
    </div>
  );
}

/** The chapter list beside the book; the current chapter is highlighted and scrolled into view. */
function ChapterPanel({
  chapters,
  current,
  onPick,
  onClose,
}: {
  chapters: readonly BookChapter[];
  current: number;
  onPick: (seconds: number) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const currentRef = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    currentRef.current?.scrollIntoView({ block: "center" });
  }, []);
  return (
    <aside
      aria-label={t("book.chaptersHeading")}
      className="absolute inset-x-4 top-2 bottom-4 z-10 flex animate-[menu-in_160ms_cubic-bezier(0.2,0.8,0.2,1)] flex-col overflow-hidden rounded-2xl bg-[#0d0d10]/85 shadow-[0_30px_80px_-24px_rgba(0,0,0,0.95)] ring-1 ring-white/10 backdrop-blur-2xl lg:static lg:h-[min(70vh,40rem)] lg:w-[24rem] lg:shrink-0"
    >
      <div className="flex shrink-0 items-center justify-between py-3 pr-3 pl-5">
        <span className="text-[15px] font-semibold tracking-tight">
          {t("book.chaptersHeading")}
          <span className="ml-2 text-sm font-normal text-white/45">{chapters.length}</span>
        </span>
        <button
          type="button"
          aria-label={t("player.close")}
          onClick={onClose}
          className="grid size-8 place-items-center rounded-full text-white/60 transition hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-white/70"
        >
          <X className="size-4" />
        </button>
      </div>
      <ol className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-2 pb-2">
        {chapters.map((c, i) => {
          const isCurrent = i === current;
          return (
            <li key={`${String(c.part)}-${String(c.start)}`}>
              <button
                ref={isCurrent ? currentRef : undefined}
                type="button"
                aria-current={isCurrent ? "true" : undefined}
                onClick={() => {
                  onPick(c.start);
                }}
                className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left text-sm transition-colors focus-visible:outline-2 focus-visible:outline-white/70 ${
                  isCurrent ? "bg-white/[0.12] text-white" : "text-white/75 hover:bg-white/[0.07] hover:text-white"
                }`}
              >
                <span className="w-6 shrink-0 text-right text-xs text-white/40 tabular-nums">{i + 1}</span>
                <span className={`min-w-0 flex-1 truncate ${isCurrent ? "font-semibold" : ""}`}>{c.title}</span>
                <span className="shrink-0 text-xs text-white/45 tabular-nums">{formatTime(c.start)}</span>
              </button>
            </li>
          );
        })}
      </ol>
    </aside>
  );
}
