import {
  ArrowLeft,
  AudioLines,
  Check,
  Gauge,
  Info,
  Keyboard,
  Loader2,
  Maximize,
  Minimize,
  Pause,
  PictureInPicture2,
  Play,
  RectangleHorizontal,
  SkipBack,
  SkipForward,
  Subtitles,
  Volume2,
  VolumeX,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ContextMenu, type MenuAction } from "../components/ContextMenu";
import { artwork } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { formatTime } from "../lib/play";
import { rememberTracks } from "../lib/trackMemory";
import { audioInfo, subtitleInfo, trackSummary, type TrackInfo } from "../lib/tracks";
import {
  ASPECTS,
  KEYMAP,
  SPEEDS,
  actionFor,
  activeMarker,
  chapterAt,
  chapterTarget,
  clampSpeed,
  clampVolume,
  keyFromActivity,
  keyName,
  mediaLines,
  neighbours,
  player,
  upNextAnchorFor,
  upNextRemaining,
  upNextStart,
  isFinalCredits,
  UP_NEXT_SECONDS,
  type PlayerAction,
  type SubtitleChoice,
} from "../lib/player";
import { enabledSources, providerTile, resolvePreviewProvider, seekr, type PreviewProvider } from "../lib/previews";
import { usePlayer } from "./context";
import { LaunchScreen } from "./LaunchScreen";
import { useLocalPreview } from "./useLocalPreview";

const HIDE_PLAYING_MS = 2500;
const SPINNER_DELAY_MS = 350;
/** How long a new skip or next-episode prompt shows by itself while the controls are hidden. */
const PROMPT_POP_MS = 8000;
/** OLED: static controls do not stay on a paused picture for long either. */
const HIDE_PAUSED_MS = 30000;
/** A paused picture dims the controls first. */
const DIM_PAUSED_MS = 8000;
type MenuName = "audio" | "subtitles" | "speed" | "picture";

interface SubtitleOption {
  key: string;
  info: TrackInfo;
  choice: SubtitleChoice;
}

/** A track row: language first (bold), the track's own title below, badges right. */
function TrackRow({ info }: { info: TrackInfo }) {
  return (
    <>
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[14px]">{info.headline}</span>
        {info.detail ? (
          <span className="mt-0.5 block truncate text-xs font-normal text-white/50">{info.detail}</span>
        ) : null}
      </span>
      {info.badges.length > 0 ? (
        <span className="ml-2 flex shrink-0 flex-wrap justify-end gap-1">
          {info.badges.map((b) => (
            <span
              key={b}
              className="rounded-[5px] bg-white/[0.08] px-1.5 py-0.5 text-[10px] leading-none font-semibold tracking-wide text-white/75 uppercase ring-1 ring-white/10 ring-inset"
            >
              {b}
            </span>
          ))}
        </span>
      ) : null}
    </>
  );
}

function num(v: unknown, fallback: number): number {
  const n = typeof v === "number" ? v : typeof v === "string" ? Number(v) : NaN;
  return Number.isFinite(n) ? n : fallback;
}

function IconButton({
  label,
  onClick,
  children,
  active = false,
  disabled = false,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
  active?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      aria-pressed={active || undefined}
      onClick={onClick}
      className={`grid size-10 place-items-center rounded-full transition hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-40 ${
        active ? "text-primary" : "text-white"
      }`}
    >
      {children}
    </button>
  );
}

function Popover({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const { t } = useTranslation();
  return (
    <div
      role="dialog"
      aria-label={title}
      className="absolute right-4 bottom-24 z-10 flex max-h-[min(70vh,38rem)] w-[26rem] max-w-[calc(100vw-2rem)] origin-bottom-right animate-[menu-in_160ms_cubic-bezier(0.2,0.8,0.2,1)] flex-col overflow-hidden rounded-2xl border border-white/10 bg-[#0d0d10]/85 text-sm text-white shadow-[0_30px_80px_-24px_rgba(0,0,0,0.95)] backdrop-blur-2xl"
    >
      <div className="flex shrink-0 items-center justify-between py-3 pr-3 pl-5">
        <span className="text-[15px] font-semibold tracking-tight">{title}</span>
        <button
          type="button"
          aria-label={t("player.close")}
          className="grid size-8 place-items-center rounded-full text-white/60 transition hover:bg-white/10 hover:text-white focus-visible:outline-2 focus-visible:outline-white/70"
          onClick={onClose}
        >
          <X className="size-4" />
        </button>
      </div>
      <div className="min-h-0 overflow-y-auto overscroll-contain px-2 pb-2">{children}</div>
    </div>
  );
}

function Choice({ selected, onSelect, children }: { selected: boolean; onSelect: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2.5 text-left font-medium transition-colors focus-visible:outline-2 focus-visible:outline-white/70 ${
        selected ? "bg-white/[0.12] text-white" : "text-white/85 hover:bg-white/[0.07] hover:text-white"
      }`}
    >
      <Check className={`size-4 shrink-0 transition-opacity ${selected ? "opacity-100" : "opacity-0"}`} />
      {children}
    </button>
  );
}

function Stepper({
  label,
  value,
  onDown,
  onUp,
  onReset,
}: {
  label: string;
  value: string;
  onDown: () => void;
  onUp: () => void;
  onReset: () => void;
}) {
  return (
    <div className="flex items-center justify-between px-3 py-1.5">
      <span>{label}</span>
      <span className="flex items-center gap-1">
        <button type="button" className="rounded px-2 hover:bg-white/15" aria-label={`${label} −`} onClick={onDown}>
          −
        </button>
        <button type="button" className="min-w-16 rounded px-1 tabular-nums hover:bg-white/15" onClick={onReset}>
          {value}
        </button>
        <button type="button" className="rounded px-2 hover:bg-white/15" aria-label={`${label} +`} onClick={onUp}>
          +
        </button>
      </span>
    </div>
  );
}

/** Width of the seek preview tile in CSS pixels, the same for trickplay sheets and local frames. */
const PREVIEW_WIDTH = 240;

/** Seek bar with chapter ticks, marker ranges and a hover preview (sprite tile or local frame, time, chapter). */
function SeekBar({
  position,
  duration,
  onSeek,
  serverId,
  provider,
  onTileError,
}: {
  position: number;
  duration: number;
  onSeek: (seconds: number) => void;
  serverId: string;
  /** Where previews come from for this file (server trickplay, Seekr, local decoding or none). */
  provider: PreviewProvider;
  /** A sprite sheet failed to load (an expired signed URL): look the previews up again. */
  onTileError: () => void;
}) {
  const { t } = useTranslation();
  const { ctx } = usePlayer();
  const bar = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [barWidth, setBarWidth] = useState(0);
  const [drag, setDrag] = useState<number | null>(null);
  const chapters = ctx?.version.chapters ?? [];
  const markers = ctx?.version.marker_segments ?? [];

  const at = (clientX: number) => {
    const r = bar.current?.getBoundingClientRect();
    if (!r || r.width <= 0) return 0;
    if (r.width !== barWidth) setBarWidth(r.width);
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  };
  const pct = (s: number) => `${String(duration > 0 ? Math.min(100, (s / duration) * 100) : 0)}%`;
  const shown = drag ?? position;
  const preview = drag ?? hover;
  const tile = preview !== null ? providerTile(provider, preview) : undefined;
  // `&p=1`: server sheets stay in the host's memory, never in the artwork cache on disk.
  const tileSrc = tile
    ? tile.server === "file"
      ? `${artwork(serverId, tile.url) ?? ""}&p=1`
      : (artwork(tile.server, tile.url) ?? "")
    : "";
  // Background images report no errors: probe the sheet once so an expired one triggers a fresh lookup.
  useEffect(() => {
    if (!tileSrc) return;
    const img = new Image();
    img.onerror = onTileError;
    img.src = tileSrc;
    return () => {
      img.onerror = null;
    };
  }, [tileSrc, onTileError]);
  // Every preview has the same width (sheets of any tile size are scaled to it) and the box stays inside the bar.
  const scale = tile ? PREVIEW_WIDTH / tile.width : 1;
  const local = useLocalPreview(provider.kind === "local", preview, duration);
  const previewLeft =
    preview !== null && duration > 0 && barWidth > 0
      ? Math.min(barWidth - PREVIEW_WIDTH / 2, Math.max(PREVIEW_WIDTH / 2, (preview / duration) * barWidth)) -
        PREVIEW_WIDTH / 2
      : null;
  const chapter = preview !== null ? chapterAt(chapters, preview) : undefined;

  return (
    <div className="group relative py-2">
      {preview !== null ? (
        <div
          className={`pointer-events-none absolute bottom-6 flex flex-col items-center gap-1 ${
            previewLeft === null ? "-translate-x-1/2" : ""
          }`}
          style={{
            width: PREVIEW_WIDTH,
            left: previewLeft === null ? pct(preview) : `${String(previewLeft)}px`,
          }}
        >
          {tile ? (
            <div
              className="shrink-0 overflow-hidden rounded-md border border-white/20 shadow-lg"
              style={{ width: PREVIEW_WIDTH, height: tile.height * scale }}
            >
              <div
                style={{
                  width: tile.width,
                  height: tile.height,
                  transform: `scale(${String(scale)})`,
                  transformOrigin: "top left",
                  backgroundImage: `url("${tileSrc}")`,
                  backgroundPosition: `-${String(tile.x)}px -${String(tile.y)}px`,
                }}
              />
            </div>
          ) : local ? (
            <img
              src={local.url}
              alt=""
              data-testid="seek-preview"
              data-approx={local.approx ? "true" : undefined}
              className="aspect-video shrink-0 rounded-md border border-white/20 bg-black object-cover shadow-lg"
              decoding="sync"
              style={{ width: PREVIEW_WIDTH }}
            />
          ) : provider.kind === "local" ? (
            // Same box while the first frame loads: the time label does not jump when the frame arrives.
            <div
              className="aspect-video shrink-0 rounded-md border border-white/20 bg-black/80 shadow-lg"
              style={{ width: PREVIEW_WIDTH }}
            />
          ) : null}
          <span className="rounded bg-black/80 px-2 py-0.5 text-xs text-white tabular-nums">
            {chapter ? `${chapter.title} · ` : ""}
            {formatTime(preview)}
          </span>
        </div>
      ) : null}
      <div
        ref={bar}
        role="slider"
        tabIndex={0}
        aria-label={t("player.seek")}
        aria-valuemin={0}
        aria-valuemax={Math.round(duration)}
        aria-valuenow={Math.round(shown)}
        aria-valuetext={`${formatTime(shown)} / ${formatTime(duration)}`}
        className="relative h-1.5 cursor-pointer rounded-full bg-white/25 transition-[height] group-hover:h-2.5"
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
      >
        {markers.map((m) => (
          <div
            key={`${m.kind}-${String(m.start_seconds)}`}
            className="absolute inset-y-0 bg-primary/45"
            style={{ left: pct(m.start_seconds), width: pct(m.end_seconds - m.start_seconds) }}
          />
        ))}
        <div className="absolute inset-y-0 left-0 rounded-full bg-primary" style={{ width: pct(shown) }} />
        {chapters
          .filter((c) => c.start_seconds > 0)
          .map((c) => (
            <div
              key={c.index}
              className="absolute inset-y-0 w-0.5 bg-black/70"
              style={{ left: pct(c.start_seconds) }}
            />
          ))}
        <div
          className="absolute top-1/2 size-3.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-white opacity-0 shadow group-hover:opacity-100"
          style={{ left: pct(shown) }}
        />
      </div>
    </div>
  );
}

/**
 * The player overlay over the native video window: transport, seek bar, track menus, picture settings, chapters,
 * skip intro/credits, previous/next episode with autoplay, stats and help. Idle → the host hides the whole WebView
 * (no static element stays on an OLED panel); input on the video window brings it back.
 */
function PlayerControls() {
  const { t, i18n } = useTranslation();
  const p = usePlayer();
  const { state, ctx, plan, settings, window: win, setWindow, showToast } = p;
  const [visible, setVisible] = useState(true);
  const [dimmed, setDimmed] = useState(false);
  const dimTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [menu, setMenu] = useState<MenuName | null>(null);
  const [help, setHelp] = useState(false);
  const [stats, setStats] = useState<Record<string, string> | null>(null);
  const [statsOpen, setStatsOpen] = useState(false);
  const [subVisible, setSubVisible] = useState(true);
  const [subKey, setSubKey] = useState<string | null>(null);
  const [previewProvider, setPreviewProvider] = useState<PreviewProvider>({ kind: "none" });
  const [previewEpoch, setPreviewEpoch] = useState(0);
  const lastTileRetry = useRef(0);
  const onTileError = useCallback(() => {
    // At most once a minute: a sheet that keeps failing must not turn into a lookup loop.
    const now = Date.now();
    if (now - lastTileRetry.current < 60_000) return;
    lastTileRetry.current = now;
    setPreviewEpoch((e) => e + 1);
  }, []);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [upNextCancelled, setUpNextCancelled] = useState(false);
  const upNextFired = useRef(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const visibleRef = useRef(visible);

  const active = state.active;
  const duration = state.duration ?? ctx?.version.duration_seconds ?? 0;
  const position = state.position;
  const paused = state.paused;
  const props = state.props;
  const speed = num(props.speed, 1);
  const subDelay = num(props["sub-delay"], 0);
  const subScale = num(props["sub-scale"], settings.subtitles.scale);
  const aspect = typeof props["video-aspect-override"] === "string" ? props["video-aspect-override"] : "no";
  const zoom = num(props["video-zoom"], 0);
  const near = neighbours(ctx);
  const chapters = useMemo(() => ctx?.version.chapters ?? [], [ctx]);
  const markers = useMemo(() => ctx?.version.marker_segments ?? [], [ctx]);
  const marker = activeMarker(markers, position);
  const lines = ctx ? mediaLines(ctx.watch) : null;
  const loadingNow =
    state.buffering || state.phase === "loading" || state.phase === "seeking" || state.phase === "buffering";
  // Shown after a short delay so quick seeks do not flash a spinner (or the overlay) over the picture.
  const [delayed, setDelayed] = useState(false);
  useEffect(() => {
    if (!loadingNow) return;
    const id = setTimeout(() => {
      setDelayed(true);
    }, SPINNER_DELAY_MS);
    return () => {
      clearTimeout(id);
      setDelayed(false);
    };
  }, [loadingNow]);
  const loading = loadingNow && delayed;
  const audioIndex = props.aid !== undefined && props.aid !== null ? num(props.aid, 0) - 1 : (ctx?.audioIndex ?? -1);

  const subtitleOptions = useMemo<SubtitleOption[]>(() => {
    const off: SubtitleOption = {
      key: "off",
      info: { headline: t("item.subtitlesOff"), detail: null, badges: [] },
      choice: { kind: "off" },
    };
    const labels = {
      fallback: t("item.unknownTrack"),
      forced: t("item.forced"),
      sdh: "SDH",
      default: t("item.default"),
      external: t("item.external"),
    };
    const info = (
      l: {
        label?: string | null;
        title?: string | null;
        embedded_title?: string | null;
        language?: string | null;
        codec?: string | null;
        hearing_impaired?: boolean;
        default?: boolean;
      },
      forced: boolean,
      external: boolean,
    ) =>
      subtitleInfo(
        {
          language: l.language ?? null,
          title: l.label ?? l.title ?? l.embedded_title ?? null,
          codec: l.codec ?? null,
          forced,
          hearing_impaired: l.hearing_impaired === true,
          default: l.default === true,
          external,
        },
        i18n.language,
        labels,
      );
    if (plan && plan.subtitles.length > 0) {
      return [
        off,
        ...plan.subtitles.map<SubtitleOption>((s) =>
          s.source === "embedded" && s.stream_index != null
            ? {
                key: `e${String(s.stream_index)}`,
                info: info(s, s.forced, false),
                choice: { kind: "embedded", stream_index: s.stream_index },
              }
            : {
                key: `x${s.track_id}`,
                info: info(s, s.forced, true),
                choice: { kind: "external", track_id: s.track_id },
              },
        ),
      ];
    }
    const tracks = ctx?.version.subtitle_tracks ?? [];
    return [
      off,
      ...tracks
        .filter((s) => !s.external && typeof s.index === "number")
        .map<SubtitleOption>((s) => ({
          key: `e${String(s.index)}`,
          info: info(s, s.forced, false),
          choice: { kind: "embedded", stream_index: s.index ?? 0 },
        })),
    ];
  }, [plan, ctx, t, i18n.language]);
  const currentSub = subKey ?? (ctx?.subtitleIndex != null ? `e${String(ctx.subtitleIndex)}` : "off");

  useEffect(() => {
    visibleRef.current = visible;
  });

  // Seek previews for the file (ADR-0011): the first enabled source that has them, in the user's order —
  // server trickplay, then Seekr (TMDB/IMDb id + duration), then local decoding.
  const fileKey = ctx ? `${ctx.serverId}|${ctx.watch.content_id}|${ctx.version.file_id}` : null;
  const order = enabledSources(settings.player).join(",");
  // Whole seconds: the lookup must not repeat while mpv refines the duration.
  const durationS = Math.round(duration);
  useEffect(() => {
    if (!ctx || !fileKey) return;
    let alive = true;
    const { serverId, watch, version } = ctx;
    void resolvePreviewProvider(
      order ? (order.split(",") as ReturnType<typeof enabledSources>) : [],
      {
        server: () =>
          version.trickplay_available
            ? player.trickplay(serverId, watch.content_id, version.file_id)
            : Promise.resolve(null),
        seekr: () =>
          durationS > 0 ? seekr.track(serverId, watch.content_id, durationS * 1000) : Promise.resolve(null),
      },
      () => alive,
    ).then((p) => {
      if (alive) setPreviewProvider(p);
    });
    return () => {
      alive = false;
      setPreviewProvider({ kind: "none" });
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- re-resolve per file, settings change or expired sheet
  }, [fileKey, order, durationS > 0, previewEpoch]);

  const keepVisible = menu !== null || help || contextMenu !== null || p.launch !== null;

  const schedule = useCallback(() => {
    if (hideTimer.current !== undefined) clearTimeout(hideTimer.current);
    hideTimer.current = undefined;
    if (dimTimer.current !== undefined) clearTimeout(dimTimer.current);
    dimTimer.current = undefined;
    if (keepVisible) return;
    if (paused) {
      dimTimer.current = setTimeout(() => {
        setDimmed(true);
      }, DIM_PAUSED_MS);
    }
    hideTimer.current = setTimeout(
      () => {
        setVisible(false);
      },
      paused ? HIDE_PAUSED_MS : HIDE_PLAYING_MS,
    );
  }, [keepVisible, paused]);

  const poke = useCallback(() => {
    if (!visibleRef.current) {
      setVisible(true);
      void player.overlay(true).catch(() => undefined);
    }
    setDimmed(false);
    schedule();
  }, [schedule]);

  // The WebView overlay is hidden with the controls (independent flip for the video, ADR-0003), but stays up while
  // the player is loading or buffering so the spinner says why the picture stopped, as in other players.
  // It also comes back with every new file (this component remounts per file while the previous one may have hidden
  // the overlay), so the launch screen covers the transparent window instead of showing the desktop through it.
  // A skip or next-episode prompt that just became available pops up on its own for a few seconds (the up-next
  // countdown for as long as it runs), even with the controls hidden, so the viewer sees that skipping is possible.
  const launching = p.launch !== null;
  const [expired, setExpired] = useState<string | null>(null);

  useEffect(() => {
    if (!active) return;
    schedule();
    return () => {
      if (hideTimer.current !== undefined) clearTimeout(hideTimer.current);
      if (dimTimer.current !== undefined) clearTimeout(dimTimer.current);
    };
  }, [active, schedule]);

  // Stats overlay polls the host while open.
  useEffect(() => {
    if (!statsOpen || !active) return;
    let alive = true;
    const load = () => {
      player
        .stats()
        .then((s) => {
          if (alive) setStats(s);
        })
        .catch(() => undefined);
    };
    load();
    const id = setInterval(load, 1000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [statsOpen, active]);

  // Not direct play: say why, once per plan.
  useEffect(() => {
    if (!plan) return;
    if (!plan.direct_play) showToast(t("player.notDirect", { reason: plan.decision_reason }));
    else if (plan.degradation_warnings.length > 0) showToast(plan.degradation_warnings.join(" · "));
  }, [plan, showToast, t]);

  useEffect(() => {
    if (!p.toast) return;
    const id = setTimeout(() => {
      showToast(null);
    }, 5000);
    return () => {
      clearTimeout(id);
    };
  }, [p.toast, showToast]);

  const fail = useCallback(
    (e: unknown) => {
      showToast(errorMessage(t, e));
    },
    [showToast, t],
  );

  const playEpisode = useCallback(
    (contentId: string) => {
      if (!ctx) return;
      void p.start(ctx.serverId, contentId).catch(fail);
    },
    [ctx, p, fail],
  );

  // Up next: from the credits marker (or 20 s before the end) a countdown in media time starts the next episode, so
  // it pauses with the video. It always ends at least 2 s before the file does.
  // The countdown is anchored where it was first seen while playing, so a file that starts inside the window (a
  // resume point in the credits, a stale position right after a file switch) still gets the full countdown.
  const upNextFrom = upNextStart(markers, duration);
  const inWindow = position >= upNextFrom && position < duration;
  const eligible =
    near.next !== undefined &&
    settings.player.autoplayNext &&
    !upNextCancelled &&
    duration > 60 &&
    state.phase === "playing" &&
    inWindow;
  const [upNextAnchor, setUpNextAnchor] = useState<number | null>(null);
  const anchorNow = upNextAnchorFor(upNextAnchor, eligible, position, upNextFrom);
  if (anchorNow !== upNextAnchor) setUpNextAnchor(anchorNow);
  const counting =
    anchorNow !== null && settings.player.autoplayNext && !upNextCancelled && near.next !== undefined && inWindow;
  const remaining = counting ? upNextRemaining(anchorNow, position) : UP_NEXT_SECONDS;
  useEffect(() => {
    if (!counting || remaining > 0 || !near.next || upNextFired.current) return;
    upNextFired.current = true;
    playEpisode(near.next.content_id);
  }, [counting, remaining, near.next, playEpisode]);

  const set = useCallback(
    (name: string, value: string | number | boolean) => {
      void player.set(name, value).catch(fail);
    },
    [fail],
  );

  const selectSubtitle = useCallback(
    (o: SubtitleOption) => {
      setSubKey(o.key);
      if (o.choice.kind !== "off") {
        setSubVisible(true);
      }
      const subtitleIndex = o.choice.kind === "embedded" ? o.choice.stream_index : null;
      p.updateCtx({ subtitleIndex });
      // An external file has no stable description to match on the next episode: only embedded and off carry over.
      if (ctx && o.choice.kind !== "external") rememberTracks(ctx.serverId, ctx.watch, ctx.version, { subtitleIndex });
      void player.subtitle(o.choice).catch(fail);
    },
    [p, ctx, fail],
  );

  const selectAudio = useCallback(
    (i: number) => {
      p.updateCtx({ audioIndex: i });
      if (ctx) rememberTracks(ctx.serverId, ctx.watch, ctx.version, { audioIndex: i });
      void player.audio(i).catch(fail);
    },
    [p, ctx, fail],
  );

  const toggleFullscreen = useCallback(
    (on?: boolean) => {
      void player.fullscreen(on).then(setWindow).catch(fail);
    },
    [setWindow, fail],
  );

  const toggleMini = useCallback(
    (on: boolean) => {
      void player.mini(on).then(setWindow).catch(fail);
    },
    [setWindow, fail],
  );

  const run = useCallback(
    (a: PlayerAction) => {
      const step = settings.player.seekStep;
      const seekBy = (s: number) => void player.seek(s, true).catch(fail);
      const tracks = ctx?.version.audio_tracks ?? [];
      switch (a) {
        case "toggle-pause":
          void player.pause().catch(fail);
          break;
        case "seek-back":
          seekBy(-step);
          break;
        case "seek-forward":
          seekBy(step);
          break;
        case "seek-back-small":
          seekBy(-1);
          break;
        case "seek-forward-small":
          seekBy(1);
          break;
        case "seek-back-large":
          seekBy(-60);
          break;
        case "seek-forward-large":
          seekBy(60);
          break;
        case "chapter-previous":
        case "chapter-next": {
          const target = chapterTarget(chapters, position, a === "chapter-next" ? 1 : -1);
          if (target !== undefined) void player.seek(target).catch(fail);
          else if (a === "chapter-previous") void player.seek(0).catch(fail);
          break;
        }
        case "volume-down":
          set("volume", clampVolume(state.volume - 5));
          break;
        case "volume-up":
          set("volume", clampVolume(state.volume + 5));
          break;
        case "mute":
          set("mute", !state.muted);
          break;
        case "fullscreen":
          toggleFullscreen();
          break;
        case "escape":
          if (help) setHelp(false);
          else if (menu) setMenu(null);
          else if (win.fullscreen) toggleFullscreen(false);
          else if (win.mini) toggleMini(false);
          break;
        case "stop":
          void player.stop().catch(fail);
          break;
        case "speed-down":
        case "speed-up": {
          const list: number[] = [...SPEEDS];
          const i = list.findIndex((s) => s >= speed - 0.001);
          const next = a === "speed-up" ? (list[i + (list[i] === speed ? 1 : 0)] ?? speed) : (list[i - 1] ?? speed);
          set("speed", clampSpeed(next));
          break;
        }
        case "speed-reset":
          set("speed", 1);
          break;
        case "sub-delay-down":
          set("sub-delay", Math.round((subDelay - 0.1) * 10) / 10);
          break;
        case "sub-delay-up":
          set("sub-delay", Math.round((subDelay + 0.1) * 10) / 10);
          break;
        case "sub-toggle":
          set("sub-visibility", !subVisible);
          setSubVisible(!subVisible);
          break;
        case "sub-cycle": {
          if (subtitleOptions.length === 0) break;
          const i = subtitleOptions.findIndex((o) => o.key === currentSub);
          const next = subtitleOptions[(i + 1) % subtitleOptions.length];
          if (next) {
            selectSubtitle(next);
            showToast(`${t("item.subtitles")}: ${trackSummary(next.info, 1)}`);
          }
          break;
        }
        case "audio-cycle":
          if (tracks.length > 1) {
            const next = (Math.max(0, audioIndex) + 1) % tracks.length;
            selectAudio(next);
            const tr = tracks[next];
            showToast(`${t("item.audio")}: ${tr?.title ?? tr?.language ?? String(next + 1)}`);
          }
          break;
        case "aspect-cycle": {
          const list: string[] = [...ASPECTS];
          const next = list[(list.indexOf(aspect) + 1) % list.length] ?? "no";
          set("video-aspect-override", next);
          showToast(`${t("player.aspect")}: ${next === "no" ? t("player.aspectAuto") : next}`);
          break;
        }
        case "stats":
          setStatsOpen((v) => !v);
          break;
        case "mini":
          toggleMini(!win.mini);
          break;
        case "next-episode":
          if (near.next) playEpisode(near.next.content_id);
          break;
        case "previous-episode":
          if (near.previous) playEpisode(near.previous.content_id);
          break;
        case "show-controls":
          break;
        case "help":
          setHelp((v) => !v);
          break;
      }
    },
    [
      settings.player.seekStep,
      ctx,
      chapters,
      position,
      state.volume,
      state.muted,
      help,
      menu,
      win.fullscreen,
      win.mini,
      speed,
      subDelay,
      subVisible,
      subtitleOptions,
      currentSub,
      audioIndex,
      aspect,
      near.next,
      near.previous,
      set,
      fail,
      toggleFullscreen,
      toggleMini,
      selectSubtitle,
      selectAudio,
      playEpisode,
      showToast,
      t,
    ],
  );

  const runRef = useRef(run);
  const pokeRef = useRef(poke);
  useEffect(() => {
    runRef.current = run;
    pokeRef.current = poke;
  });

  // Keyboard while the WebView has focus.
  useEffect(() => {
    if (!active) return;
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (target && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName))) return;
      if (target?.tagName === "BUTTON" && (e.key === " " || e.key === "Enter")) return;
      const action = actionFor(keyName(e));
      if (!action) return;
      e.preventDefault();
      if (action !== "fullscreen" && action !== "mini" && action !== "escape") pokeRef.current();
      runRef.current(action);
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
    };
  }, [active]);

  // Input that reached the native video window while the overlay was hidden, and media keys.
  useEffect(() => {
    const unlisten = [
      player.onActivity((a) => {
        switch (a.kind) {
          case "key": {
            const name = keyFromActivity(a);
            const action = name ? actionFor(a.ctrl ? `Ctrl+${name}` : name) : undefined;
            if (!action) return;
            if (action !== "fullscreen" && action !== "mini" && action !== "escape") pokeRef.current();
            runRef.current(action);
            return;
          }
          case "dblclick":
            runRef.current("fullscreen");
            return;
          case "wheel":
            pokeRef.current();
            runRef.current(a.delta > 0 ? "volume-up" : "volume-down");
            return;
          default:
            pokeRef.current();
        }
      }),
      player.onMediaKey((k) => {
        const map: Record<string, PlayerAction> = {
          next: "next-episode",
          previous: "previous-episode",
          fast_forward: "seek-forward",
          rewind: "seek-back",
        };
        const action = map[k];
        if (action) runRef.current(action);
      }),
    ];
    return () => {
      unlisten.forEach((u) => {
        void u.then((f) => {
          f();
        });
      });
    };
  }, []);

  const skip =
    marker && (marker.kind === "intro" || marker.kind === "recap")
      ? { label: t(marker.kind === "intro" ? "player.skipIntro" : "player.skipRecap"), to: marker.end_seconds }
      : marker && marker.kind === "credits" && (!near.next || !isFinalCredits(marker, duration))
        ? { label: t("player.skipCredits"), to: marker.end_seconds }
        : null;
  // Next episode by hand: during credits (also mid-episode ones with an epilogue after them, next to Skip credits) and
  // again in the final stretch whenever the countdown is not running (autoplay off or cancelled).
  const showNext = near.next !== undefined && !counting && (marker?.kind === "credits" || inWindow);
  const promptKey = counting ? "up-next" : skip ? `skip:${String(skip.to)}` : showNext ? "next" : null;
  const popped = promptKey !== null && (promptKey === "up-next" || expired !== promptKey);
  useEffect(() => {
    if (promptKey === null || promptKey === "up-next") return;
    const id = setTimeout(() => {
      setExpired(promptKey);
    }, PROMPT_POP_MS);
    return () => {
      clearTimeout(id);
    };
  }, [promptKey]);
  useEffect(() => {
    if (!active) return;
    void player.overlay(visible || loading || launching || popped).catch(() => undefined);
  }, [active, visible, loading, launching, popped]);

  const contextActions: MenuAction[] = [
    {
      label: paused ? t("player.play") : t("player.pause"),
      onSelect: () => {
        run("toggle-pause");
      },
    },
    {
      label: win.fullscreen ? t("player.exitFullscreen") : t("player.fullscreen"),
      onSelect: () => {
        run("fullscreen");
      },
    },
    {
      label: win.mini ? t("player.exitMini") : t("player.mini"),
      onSelect: () => {
        run("mini");
      },
    },
    {
      label: t("player.stats"),
      onSelect: () => {
        run("stats");
      },
    },
    {
      label: t("player.shortcuts"),
      onSelect: () => {
        run("help");
      },
    },
    {
      label: t("player.stop"),
      onSelect: () => {
        run("stop");
      },
    },
  ];

  return (
    <div
      className={`fixed inset-0 z-40 flex flex-col text-white select-none ${visible ? "" : "cursor-none"}`}
      data-testid="player-view"
      onMouseMove={poke}
      onContextMenu={(e) => {
        e.preventDefault();
        setContextMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      {/* Click-through area: click toggles pause, double-click fullscreen, wheel volume. Every action has a keyboard
          equivalent in the global key map (Space, f, arrow keys), so this mouse-only surface is hidden from AT. */}
      <div
        aria-hidden="true"
        className="absolute inset-0"
        onClick={() => {
          if (menu) setMenu(null);
          else run("toggle-pause");
        }}
        onDoubleClick={() => {
          run("fullscreen");
        }}
        onWheel={(e) => {
          run(e.deltaY < 0 ? "volume-up" : "volume-down");
        }}
      />

      <div
        className={`pointer-events-none relative flex flex-1 flex-col justify-between transition-opacity duration-300 ${
          visible ? (dimmed && paused ? "opacity-40" : "opacity-100") : "opacity-0"
        }`}
      >
        {/* Top bar */}
        <div className="pointer-events-auto flex items-center gap-3 bg-gradient-to-b from-black/80 to-transparent px-4 pt-4 pb-10">
          <IconButton
            label={t("player.back")}
            onClick={() => {
              run("stop");
            }}
          >
            <ArrowLeft className="size-5" />
          </IconButton>
          {lines ? (
            <div className="min-w-0">
              <div className="truncate text-base font-semibold">{lines.title}</div>
              {lines.subtitle ? <div className="truncate text-sm text-white/70">{lines.subtitle}</div> : null}
            </div>
          ) : null}
        </div>

        {/* Bottom controls */}
        <div className="pointer-events-auto bg-gradient-to-t from-black/85 to-transparent px-6 pt-16 pb-4">
          {!win.mini ? (
            <SeekBar
              position={position}
              duration={duration}
              serverId={ctx?.serverId ?? ""}
              provider={previewProvider}
              onTileError={onTileError}
              onSeek={(s) => void player.seek(s).catch(fail)}
            />
          ) : null}
          <div className="mt-1 flex items-center gap-1">
            {near.previous && !win.mini ? (
              <IconButton
                label={t("player.previousEpisode")}
                onClick={() => {
                  run("previous-episode");
                }}
              >
                <SkipBack className="size-5" />
              </IconButton>
            ) : null}
            <IconButton
              label={paused ? t("player.play") : t("player.pause")}
              onClick={() => {
                run("toggle-pause");
              }}
            >
              {paused ? <Play className="size-6 fill-current" /> : <Pause className="size-6 fill-current" />}
            </IconButton>
            {near.next && !win.mini ? (
              <IconButton
                label={t("player.nextEpisode")}
                onClick={() => {
                  run("next-episode");
                }}
              >
                <SkipForward className="size-5" />
              </IconButton>
            ) : null}
            {!win.mini ? (
              <>
                <IconButton
                  label={state.muted ? t("player.unmute") : t("player.mute")}
                  onClick={() => {
                    run("mute");
                  }}
                >
                  {state.muted || state.volume === 0 ? <VolumeX className="size-5" /> : <Volume2 className="size-5" />}
                </IconButton>
                <input
                  type="range"
                  min={0}
                  max={100}
                  value={Math.min(100, state.volume)}
                  aria-label={t("player.volume")}
                  className="w-24 accent-[var(--primary)]"
                  onChange={(e) => {
                    set("volume", clampVolume(Number(e.target.value)));
                  }}
                />
                <span className="ml-3 text-sm tabular-nums text-white/85">
                  {formatTime(position)} / {formatTime(duration)}
                </span>
                {chapterAt(chapters, position) ? (
                  <span className="ml-3 truncate text-sm text-white/60">{chapterAt(chapters, position)?.title}</span>
                ) : null}
              </>
            ) : null}
            <div className="flex-1" />
            {!win.mini ? (
              <>
                <IconButton
                  label={t("item.audio")}
                  active={menu === "audio"}
                  onClick={() => {
                    setMenu(menu === "audio" ? null : "audio");
                  }}
                  disabled={(ctx?.version.audio_tracks.length ?? 0) === 0}
                >
                  <AudioLines className="size-5" />
                </IconButton>
                <IconButton
                  label={t("item.subtitles")}
                  active={menu === "subtitles"}
                  onClick={() => {
                    setMenu(menu === "subtitles" ? null : "subtitles");
                  }}
                >
                  <Subtitles className="size-5" />
                </IconButton>
                <IconButton
                  label={t("player.speed")}
                  active={menu === "speed"}
                  onClick={() => {
                    setMenu(menu === "speed" ? null : "speed");
                  }}
                >
                  <Gauge className="size-5" />
                </IconButton>
                <IconButton
                  label={t("player.picture")}
                  active={menu === "picture"}
                  onClick={() => {
                    setMenu(menu === "picture" ? null : "picture");
                  }}
                >
                  <RectangleHorizontal className="size-5" />
                </IconButton>
                <IconButton
                  label={t("player.stats")}
                  active={statsOpen}
                  onClick={() => {
                    run("stats");
                  }}
                >
                  <Info className="size-5" />
                </IconButton>
                <IconButton
                  label={t("player.shortcuts")}
                  active={help}
                  onClick={() => {
                    run("help");
                  }}
                >
                  <Keyboard className="size-5" />
                </IconButton>
              </>
            ) : null}
            <IconButton
              label={win.mini ? t("player.exitMini") : t("player.mini")}
              onClick={() => {
                run("mini");
              }}
            >
              <PictureInPicture2 className="size-5" />
            </IconButton>
            <IconButton
              label={win.fullscreen ? t("player.exitFullscreen") : t("player.fullscreen")}
              onClick={() => {
                run("fullscreen");
              }}
            >
              {win.fullscreen ? <Minimize className="size-5" /> : <Maximize className="size-5" />}
            </IconButton>
          </div>
        </div>
      </div>

      {loading && !p.launch ? (
        <div className="pointer-events-none absolute inset-0 grid place-items-center" aria-live="polite">
          <Loader2 className="size-12 animate-spin text-white/80" aria-label={t("player.loading")} />
        </div>
      ) : null}

      {(skip || showNext) && !counting ? (
        <div className="absolute right-8 bottom-32 flex gap-2">
          {skip ? (
            <button
              type="button"
              className="rounded-lg border border-white/30 bg-black/70 px-5 py-2.5 text-sm font-semibold text-white backdrop-blur-md transition hover:bg-white/20"
              onClick={() => void player.seek(skip.to).catch(fail)}
            >
              {skip.label}
            </button>
          ) : null}
          {showNext ? (
            <button
              type="button"
              className="rounded-lg border border-white/30 bg-black/70 px-5 py-2.5 text-sm font-semibold text-white backdrop-blur-md transition hover:bg-white/20"
              onClick={() => {
                if (near.next) playEpisode(near.next.content_id);
              }}
            >
              {t("player.nextEpisode")}
            </button>
          ) : null}
        </div>
      ) : null}

      {counting && near.next ? (
        <div className="absolute right-8 bottom-32 flex w-80 flex-col gap-3 rounded-xl border border-white/15 bg-black/85 p-4 shadow-2xl">
          <div className="text-xs tracking-wide text-white/60 uppercase">
            {t("player.upNextIn", { seconds: remaining })}
          </div>
          <div className="truncate font-semibold">{near.next.title}</div>
          <div className="flex gap-2">
            <button
              type="button"
              className="flex-1 rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground"
              onClick={() => {
                if (near.next) playEpisode(near.next.content_id);
              }}
            >
              {t("player.playNow")}
            </button>
            <button
              type="button"
              className="flex-1 rounded-lg bg-white/15 px-3 py-2 text-sm"
              onClick={() => {
                setUpNextCancelled(true);
                p.cancelAutoplay();
              }}
            >
              {t("player.cancel")}
            </button>
          </div>
        </div>
      ) : null}

      {menu === "audio" && ctx ? (
        <Popover
          title={t("item.audio")}
          onClose={() => {
            setMenu(null);
          }}
        >
          {ctx.version.audio_tracks.map((a, i) => (
            <Choice
              key={`${String(i)}-${a.language ?? ""}`}
              selected={i === audioIndex}
              onSelect={() => {
                selectAudio(i);
                setMenu(null);
              }}
            >
              <TrackRow
                info={audioInfo(a, i18n.language, { fallback: `#${String(i + 1)}`, default: t("item.default") })}
              />
            </Choice>
          ))}
        </Popover>
      ) : null}

      {menu === "subtitles" ? (
        <Popover
          title={t("item.subtitles")}
          onClose={() => {
            setMenu(null);
          }}
        >
          {subtitleOptions.map((o) => (
            <Choice
              key={o.key}
              selected={o.key === currentSub}
              onSelect={() => {
                selectSubtitle(o);
                setMenu(null);
              }}
            >
              <TrackRow info={o.info} />
            </Choice>
          ))}
          <div className="my-1 border-t border-white/10" />
          <Stepper
            label={t("player.subDelay")}
            value={`${subDelay.toFixed(1)} s`}
            onDown={() => {
              run("sub-delay-down");
            }}
            onUp={() => {
              run("sub-delay-up");
            }}
            onReset={() => {
              set("sub-delay", 0);
            }}
          />
          <Stepper
            label={t("player.subSize")}
            value={`${String(Math.round(subScale * 100))} %`}
            onDown={() => {
              set("sub-scale", Math.max(0.3, Math.round((subScale - 0.1) * 10) / 10));
            }}
            onUp={() => {
              set("sub-scale", Math.min(3, Math.round((subScale + 0.1) * 10) / 10));
            }}
            onReset={() => {
              set("sub-scale", settings.subtitles.scale);
            }}
          />
        </Popover>
      ) : null}

      {menu === "speed" ? (
        <Popover
          title={t("player.speed")}
          onClose={() => {
            setMenu(null);
          }}
        >
          {SPEEDS.map((s) => (
            <Choice
              key={s}
              selected={Math.abs(s - speed) < 0.001}
              onSelect={() => {
                set("speed", s);
                setMenu(null);
              }}
            >
              {s === 1 ? t("player.speedNormal") : `${String(s)}×`}
            </Choice>
          ))}
        </Popover>
      ) : null}

      {menu === "picture" ? (
        <Popover
          title={t("player.picture")}
          onClose={() => {
            setMenu(null);
          }}
        >
          <div className="px-3 pt-1 pb-1 text-xs text-white/60 uppercase">{t("player.aspect")}</div>
          {ASPECTS.map((a) => (
            <Choice
              key={a}
              selected={a === aspect || (a === "no" && aspect === "-1")}
              onSelect={() => {
                set("video-aspect-override", a);
              }}
            >
              {a === "no" ? t("player.aspectAuto") : a}
            </Choice>
          ))}
          <div className="my-1 border-t border-white/10" />
          <Stepper
            label={t("player.zoom")}
            value={zoom.toFixed(1)}
            onDown={() => {
              set("video-zoom", Math.max(-2, Math.round((zoom - 0.1) * 10) / 10));
            }}
            onUp={() => {
              set("video-zoom", Math.min(2, Math.round((zoom + 0.1) * 10) / 10));
            }}
            onReset={() => {
              set("video-zoom", 0);
            }}
          />
        </Popover>
      ) : null}

      {statsOpen ? (
        <div
          className="absolute top-20 left-4 max-w-md rounded-lg bg-black/80 p-3 font-mono text-xs leading-5 text-white/90"
          aria-label={t("player.stats")}
        >
          {plan ? (
            <>
              <div>
                delivery: {plan.delivery}
                {plan.direct_play ? " (direct)" : ""} · {plan.decision_reason}
              </div>
              <div>
                {plan.container ?? "?"} · {plan.video_codec ?? "?"} / {plan.audio_codec ?? "?"} ·{" "}
                {plan.dynamic_range ?? "SDR"}
              </div>
            </>
          ) : null}
          {Object.entries(stats ?? {}).map(([k, v]) => (
            <div key={k}>
              {k}: {v}
            </div>
          ))}
        </div>
      ) : null}

      {help ? (
        <div
          className="absolute inset-0 z-20 grid place-items-center bg-black/70"
          role="dialog"
          aria-label={t("player.shortcuts")}
        >
          <div className="max-h-[80vh] w-[min(40rem,90vw)] overflow-y-auto rounded-xl border border-white/10 bg-black/90 p-5">
            <div className="mb-3 flex items-center justify-between">
              <h2 className="text-lg font-semibold">{t("player.shortcuts")}</h2>
              <button
                type="button"
                aria-label={t("player.close")}
                className="rounded p-1 hover:bg-white/15"
                onClick={() => {
                  setHelp(false);
                }}
              >
                <X className="size-5" />
              </button>
            </div>
            <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-1.5 text-sm">
              {KEYMAP.map((b) => (
                <div key={b.action} className="contents">
                  <dt className="flex flex-wrap gap-1">
                    {b.keys.map((k) => (
                      <kbd
                        key={k}
                        className="rounded border border-white/25 bg-white/10 px-1.5 py-0.5 font-mono text-xs"
                      >
                        {k}
                      </kbd>
                    ))}
                  </dt>
                  <dd className="text-white/80">{t(`player.keys.${b.label}`)}</dd>
                </div>
              ))}
            </dl>
          </div>
        </div>
      ) : null}

      {p.toast ? (
        <div
          role="status"
          className="absolute top-6 left-1/2 max-w-lg -translate-x-1/2 rounded-lg bg-black/85 px-4 py-2 text-sm text-white shadow-lg"
        >
          {p.toast}
        </div>
      ) : null}

      {contextMenu ? (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          label={t("player.menu")}
          actions={contextActions}
          onClose={() => {
            setContextMenu(null);
          }}
        />
      ) : null}
    </div>
  );
}

/** Whether the last playback ended in fullscreen; the next one starts the same way. */
const PLAYER_FULLSCREEN_KEY = "granary.player.fullscreen";

function readPlayerFullscreen(): boolean | null {
  try {
    const v = localStorage.getItem(PLAYER_FULLSCREEN_KEY);
    return v === null ? null : v === "1";
  } catch {
    return null;
  }
}

function writePlayerFullscreen(on: boolean) {
  try {
    localStorage.setItem(PLAYER_FULLSCREEN_KEY, on ? "1" : "0");
  } catch {
    // Storage unavailable: the next playback starts in the window state it finds.
  }
}

/**
 * The player overlay while a playback attempt runs (remounted per file, so menus and per-file picks start fresh), and
 * the error card after a fatal end. The player and the library each keep their own window state: playback starts in
 * fullscreen when the last one ended there, and leaving it puts the window back the way browsing had it.
 */
export function PlayerView() {
  const { t } = useTranslation();
  const { state, ctx, error, dismissError, launch, setWindow } = usePlayer();
  const fileKey = ctx ? `${ctx.serverId}|${ctx.watch.content_id}|${ctx.version.file_id}` : "none";

  const wasActive = useRef(false);
  const browseFullscreen = useRef<boolean | null>(null);
  useEffect(() => {
    if (state.active) {
      if (wasActive.current) return;
      wasActive.current = true;
      void player
        .window()
        .then(async (w) => {
          browseFullscreen.current = w.fullscreen;
          const want = readPlayerFullscreen();
          if (want !== null && want !== w.fullscreen && !w.mini) setWindow(await player.fullscreen(want));
        })
        .catch(() => undefined);
      return;
    }
    // Playback over: the WebView must be visible again for the library, and a mini/fullscreen player window goes
    // back to the browsing window state (phase-6 review L7).
    void player.overlay(true).catch(() => undefined);
    if (!wasActive.current) return;
    wasActive.current = false;
    void player
      .window()
      .then(async (w) => {
        if (!w.mini) writePlayerFullscreen(w.fullscreen);
        if (w.mini) await player.mini(false);
        setWindow(await player.fullscreen(browseFullscreen.current ?? false));
      })
      .catch(() => undefined);
  }, [state.active, setWindow]);

  // A new attempt (next episode, autoplay) can start while the controls had hidden the WebView over the video: the
  // launch card lives in the WebView, so it must be shown, or the desktop shows through the transparent window.
  const launching = launch !== null;
  useEffect(() => {
    if (launching) void player.overlay(true).catch(() => undefined);
  }, [launching]);

  return (
    <>
      {state.active ? (
        <PlayerControls key={fileKey} />
      ) : error ? (
        <div className="fixed inset-0 z-50 grid place-items-center bg-background/95 p-6" role="alertdialog">
          <div className="flex max-w-md flex-col gap-4 rounded-xl border border-border bg-card p-6 shadow-2xl">
            <h2 className="text-lg font-semibold">{t("player.failed")}</h2>
            <p className="text-sm text-muted-foreground">{errorMessage(t, error)}</p>
            <button
              type="button"
              className="self-end rounded-lg bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
              onClick={dismissError}
            >
              {t("player.close")}
            </button>
          </div>
        </div>
      ) : null}
      <LaunchScreen launch={launch} />
    </>
  );
}
