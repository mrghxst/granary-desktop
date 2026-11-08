import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { Chapter, Episode, FileVersion, MarkerOccurrence, WatchDetail } from "./catalog";

/** Mirrors `UiState` in src-tauri/src/playback/host.rs (serde camelCase). Positions are source-timeline seconds. */
export interface PlayerState {
  active: boolean;
  itemId?: string | null;
  fileId?: string | null;
  phase?: "idle" | "loading" | "playing" | "paused" | "buffering" | "seeking" | "ended" | null;
  position: number;
  duration?: number | null;
  paused: boolean;
  buffering: boolean;
  volume: number;
  muted: boolean;
  props: Record<string, string | number | boolean | null>;
}

export const IDLE_STATE: PlayerState = {
  active: false,
  position: 0,
  paused: false,
  buffering: false,
  volume: 100,
  muted: false,
  props: {},
};

/** `SubtitleInventoryItemV3` (subset) from the plan. */
export interface PlanSubtitle {
  track_id: string;
  combined_index: number;
  source: string;
  delivery: string;
  codec?: string | null;
  language?: string | null;
  label?: string | null;
  forced: boolean;
  hearing_impaired: boolean;
  url?: string | null;
  stream_index?: number | null;
}

/** Mirrors `PlanInfo` in controller.rs. */
export interface PlanInfo {
  session_id: string;
  plan_id: string;
  delivery: string;
  decision_reason: string;
  direct_play: boolean;
  relayed: boolean;
  protocol: string;
  container?: string | null;
  video_codec?: string | null;
  audio_codec?: string | null;
  dynamic_range?: string | null;
  degradation_warnings: string[];
  subtitles: PlanSubtitle[];
}

/** Mirrors `Notice` in controller.rs (`kind`-tagged). */
export type PlayerNotice =
  | ({ kind: "plan" } & PlanInfo)
  | { kind: "progress"; sequence: number; position: number; paused: boolean; outcome: string }
  | { kind: "error"; code: string; message: string; fatal: boolean }
  | { kind: "ended" }
  | { kind: "stopped"; outcome?: string | null };

/** Input that reached the native video window while the overlay was hidden (video.rs `Activity`). */
export interface VideoActivity {
  kind: "mousemove" | "click" | "dblclick" | "contextmenu" | "wheel" | "key";
  vk?: number | null;
  /** The character in the active keyboard layout (like a DOM `KeyboardEvent.key`), when the key has one. */
  ch?: string | null;
  shift: boolean;
  ctrl: boolean;
  delta: number;
}

export interface WindowState {
  fullscreen: boolean;
  mini: boolean;
}

export type SubtitleChoice =
  { kind: "off" } | { kind: "embedded"; stream_index: number } | { kind: "external"; track_id: string };

export interface Trickplay {
  file_id: string;
  interval_ms: number;
  thumbnail_width: number;
  thumbnail_height: number;
  tile_columns: number;
  tile_rows: number;
  thumbnail_count: number;
  expires_at: string;
  sheets: { index: number; url: string }[];
}

function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri()) return Promise.reject(Object.assign(new Error("not running inside Granary"), { code: "no_host" }));
  return invoke<T>(cmd, args);
}

// eslint-disable-next-line @typescript-eslint/no-unnecessary-type-parameters -- T types the event payload for the caller
async function on<T>(event: string, f: (payload: T) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<T>(event, (e) => {
    f(e.payload);
  });
}

export const player = {
  stop: () => call<null>("player_stop"),
  pause: (paused?: boolean) => call<null>("player_pause", { paused: paused ?? null }),
  seek: (seconds: number, relative = false) => call<null>("player_seek", { seconds, relative }),
  set: (name: string, value: string | number | boolean) =>
    call<null>("player_set", {
      name,
      value: typeof value === "boolean" ? (value ? "yes" : "no") : String(value),
    }),
  audio: (index: number) => call<null>("player_audio", { index }),
  subtitle: (choice: SubtitleChoice) => call<null>("player_subtitle", { choice }),
  stats: () => call<Record<string, string>>("player_stats"),
  state: () => call<PlayerState>("player_state"),
  overlay: (visible: boolean) => call<null>("player_overlay", { visible }),
  fullscreen: (fullscreen?: boolean) => call<WindowState>("player_fullscreen", { fullscreen: fullscreen ?? null }),
  mini: (enabled: boolean) => call<WindowState>("player_mini", { enabled }),
  window: () => call<WindowState>("player_window"),
  /** JPEG data URL of the frame near `seconds` in the playing file (no server trickplay), or null. */
  preview: (seconds: number) => call<string | null>("player_preview", { seconds }),
  /** The nearest frame already decoded (never waits for the network). */
  previewNear: (seconds: number) => call<{ at: number; url: string } | null>("player_preview_near", { seconds }),
  /** Start the paced background fill of preview frames for the playing file. */
  previewWarm: () => call<null>("player_preview_warm"),
  trickplay: (serverId: string, itemId: string, fileId: string) =>
    call<Trickplay>("watch_trickplay", { serverId, itemId, fileId }),
  onState: (f: (s: PlayerState) => void) => on("player://state", f),
  onNotice: (f: (n: PlayerNotice) => void) => on("player://notice", f),
  onActivity: (f: (a: VideoActivity) => void) => on("player://activity", f),
  onWindow: (f: (w: WindowState) => void) => on("player://window", f),
  onMediaKey: (f: (k: string) => void) => on("player://media-key", f),
};

/** What is playing, as the UI knows it (for menus, chapters, markers and next/previous episode). */
export interface PlaybackContext {
  serverId: string;
  watch: WatchDetail;
  version: FileVersion;
  audioIndex: number | null;
  subtitleIndex: number | null;
  /** Episodes of the season in order, plus the edge episode of the neighbouring season at a boundary (series only). */
  episodes: Episode[];
}

export function neighbours(ctx: PlaybackContext | null): {
  previous?: Episode | undefined;
  next?: Episode | undefined;
} {
  if (!ctx || ctx.episodes.length === 0) return {};
  const i = ctx.episodes.findIndex((e) => e.content_id === ctx.watch.content_id);
  if (i < 0) return {};
  return { previous: ctx.episodes[i - 1], next: ctx.episodes[i + 1] };
}

/**
 * Episodes around the one playing: its season, plus the last episode of the previous regular season when it is the
 * first, and the first episode of the next regular season when it is the last. There is no next-up endpoint upstream
 * (silo-api.md open question 9), so next/previous across a season boundary is built from listSeasons and
 * listSeasonEpisodes. `regularSeasons` returns the season numbers without specials, in order; specials never cross.
 */
export async function episodesAround(
  contentId: string,
  season: number,
  regularSeasons: () => Promise<number[]>,
  episodesOf: (season: number) => Promise<Episode[]>,
): Promise<Episode[]> {
  const none = (): Episode[] => [];
  const here = await episodesOf(season).catch(none);
  const i = here.findIndex((e) => e.content_id === contentId);
  if (i < 0 || (i > 0 && i < here.length - 1)) return here;
  const seasons = await regularSeasons().catch((): number[] => []);
  const k = seasons.indexOf(season);
  if (k < 0) return here;
  const prevSeason = i === 0 ? seasons[k - 1] : undefined;
  const nextSeason = i === here.length - 1 ? seasons[k + 1] : undefined;
  const before = prevSeason === undefined ? [] : (await episodesOf(prevSeason).catch(none)).slice(-1);
  const after = nextSeason === undefined ? [] : (await episodesOf(nextSeason).catch(none)).slice(0, 1);
  return [...before, ...here, ...after];
}

export function chapterAt(chapters: Chapter[], position: number): Chapter | undefined {
  return [...chapters].reverse().find((c) => position >= c.start_seconds);
}

/** Next/previous chapter start for PgUp/PgDn (previous restarts the current chapter after 3 s, like mpv). */
export function chapterTarget(chapters: Chapter[], position: number, direction: 1 | -1): number | undefined {
  const starts = chapters.map((c) => c.start_seconds).sort((a, b) => a - b);
  if (direction === 1) return starts.find((s) => s > position + 0.5);
  return [...starts].reverse().find((s) => s < position - 3);
}

/** The marker the position is in (intro, recap, credits, preview). */
export function activeMarker(markers: MarkerOccurrence[], position: number): MarkerOccurrence | undefined {
  return markers.find((m) => position >= m.start_seconds && position < m.end_seconds - 0.25);
}

/** Thumbnail for a seek position: sheet URL plus the tile offset (sheet `floor(n / (cols*rows))`, UNVERIFIED). */
export function trickplayTile(
  t: Trickplay,
  seconds: number,
): { url: string; x: number; y: number; width: number; height: number } | undefined {
  if (t.interval_ms <= 0 || t.thumbnail_count <= 0) return undefined;
  const n = Math.min(t.thumbnail_count - 1, Math.max(0, Math.floor((seconds * 1000) / t.interval_ms)));
  const perSheet = t.tile_columns * t.tile_rows;
  if (perSheet <= 0) return undefined;
  const sheet = t.sheets.find((s) => s.index === Math.floor(n / perSheet));
  if (!sheet) return undefined;
  const k = n % perSheet;
  return {
    url: sheet.url,
    x: (k % t.tile_columns) * t.thumbnail_width,
    y: Math.floor(k / t.tile_columns) * t.thumbnail_height,
    width: t.thumbnail_width,
    height: t.thumbnail_height,
  };
}

/* ---------- keyboard: mpv-like defaults (README "Keyboard shortcuts" is generated from this table) ---------- */

export type PlayerAction =
  | "toggle-pause"
  | "seek-back"
  | "seek-forward"
  | "seek-back-small"
  | "seek-forward-small"
  | "seek-back-large"
  | "seek-forward-large"
  | "chapter-previous"
  | "chapter-next"
  | "volume-down"
  | "volume-up"
  | "mute"
  | "fullscreen"
  | "escape"
  | "stop"
  | "speed-down"
  | "speed-up"
  | "speed-reset"
  | "sub-delay-down"
  | "sub-delay-up"
  | "sub-toggle"
  | "sub-cycle"
  | "audio-cycle"
  | "aspect-cycle"
  | "stats"
  | "mini"
  | "next-episode"
  | "previous-episode"
  | "show-controls"
  | "help";

export interface KeyBinding {
  keys: string[];
  action: PlayerAction;
  /** i18n key under `player.keys`. */
  label: string;
}

export const KEYMAP: KeyBinding[] = [
  { keys: ["Space", "p", "k"], action: "toggle-pause", label: "pause" },
  { keys: ["ArrowLeft"], action: "seek-back", label: "seekBack" },
  { keys: ["ArrowRight"], action: "seek-forward", label: "seekForward" },
  { keys: ["Shift+ArrowLeft"], action: "seek-back-small", label: "seekBackSmall" },
  { keys: ["Shift+ArrowRight"], action: "seek-forward-small", label: "seekForwardSmall" },
  { keys: ["ArrowDown"], action: "seek-back-large", label: "seekBackLarge" },
  { keys: ["ArrowUp"], action: "seek-forward-large", label: "seekForwardLarge" },
  { keys: ["PageDown"], action: "chapter-previous", label: "chapterPrevious" },
  { keys: ["PageUp"], action: "chapter-next", label: "chapterNext" },
  { keys: ["9", "/"], action: "volume-down", label: "volumeDown" },
  { keys: ["0", "*"], action: "volume-up", label: "volumeUp" },
  { keys: ["m"], action: "mute", label: "mute" },
  { keys: ["f", "Enter"], action: "fullscreen", label: "fullscreen" },
  { keys: ["Escape"], action: "escape", label: "escape" },
  { keys: ["q"], action: "stop", label: "stop" },
  { keys: ["["], action: "speed-down", label: "speedDown" },
  { keys: ["]"], action: "speed-up", label: "speedUp" },
  { keys: ["Backspace"], action: "speed-reset", label: "speedReset" },
  { keys: ["z"], action: "sub-delay-down", label: "subDelayDown" },
  { keys: ["x", "Z"], action: "sub-delay-up", label: "subDelayUp" },
  { keys: ["v"], action: "sub-toggle", label: "subToggle" },
  { keys: ["j"], action: "sub-cycle", label: "subCycle" },
  { keys: ["#", "a"], action: "audio-cycle", label: "audioCycle" },
  { keys: ["A"], action: "aspect-cycle", label: "aspectCycle" },
  { keys: ["i", "I"], action: "stats", label: "stats" },
  { keys: ["T"], action: "mini", label: "mini" },
  { keys: [">", "N"], action: "next-episode", label: "nextEpisode" },
  { keys: ["<", "P"], action: "previous-episode", label: "previousEpisode" },
  { keys: ["o"], action: "show-controls", label: "showControls" },
  { keys: ["?", "F1"], action: "help", label: "help" },
];

/** Normalised key name for a DOM keyboard event: `Space`, `Shift+ArrowLeft`, `x`, `Z`, `?`. */
export function keyName(e: { key: string; shiftKey: boolean; ctrlKey: boolean; altKey: boolean }): string {
  const key = e.key === " " ? "Space" : e.key;
  // Shift only matters for keys whose `key` does not already change with it.
  const named = key.length > 1;
  return `${e.ctrlKey ? "Ctrl+" : ""}${e.altKey ? "Alt+" : ""}${e.shiftKey && named ? "Shift+" : ""}${key}`;
}

export function actionFor(name: string): PlayerAction | undefined {
  return KEYMAP.find((b) => b.keys.includes(name))?.action;
}

/**
 * Key name for a key press forwarded from the native video window. The layout character from the host wins, so `#` on
 * a German keyboard is `#` as it is in the webview; the virtual-key map covers named keys (arrows, Enter, F1 ...).
 */
export function keyFromActivity(a: Pick<VideoActivity, "vk" | "ch" | "shift">): string | undefined {
  // The host sends exactly one character (video.rs `layout_char`).
  if (a.ch) return a.ch === " " ? "Space" : a.ch;
  return a.vk != null ? keyFromVk(a.vk, a.shift) : undefined;
}

/** Key name for a virtual-key code (US layout for punctuation): the fallback when the host sends no character. */
export function keyFromVk(vk: number, shift: boolean): string | undefined {
  const named: Record<number, string> = {
    0x08: "Backspace",
    0x0d: "Enter",
    0x1b: "Escape",
    0x20: "Space",
    0x21: "PageUp",
    0x22: "PageDown",
    0x25: "ArrowLeft",
    0x26: "ArrowUp",
    0x27: "ArrowRight",
    0x28: "ArrowDown",
    0x70: "F1",
    0x6a: "*",
    0x6f: "/",
  };
  if (named[vk]) {
    const k = named[vk];
    return shift && k.length > 1 && k !== "Space" ? `Shift+${k}` : k;
  }
  if (vk >= 0x41 && vk <= 0x5a) {
    const c = String.fromCharCode(vk);
    return shift ? c : c.toLowerCase();
  }
  if (vk >= 0x30 && vk <= 0x39) {
    if (shift && vk === 0x33) return "#";
    return String.fromCharCode(vk);
  }
  const punct: Record<number, [string, string]> = {
    0xdb: ["[", "{"],
    0xdd: ["]", "}"],
    0xbe: [".", ">"],
    0xbc: [",", "<"],
    0xbf: ["/", "?"],
  };
  const p = punct[vk];
  return p ? p[shift ? 1 : 0] : undefined;
}

export const ASPECTS = ["no", "16:9", "4:3", "2.35:1"] as const;
export const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2] as const;

export function clampVolume(v: number): number {
  return Math.round(Math.min(150, Math.max(0, v)));
}

export function clampSpeed(v: number): number {
  return Math.round(Math.min(4, Math.max(0.25, v)) * 100) / 100;
}

export const UP_NEXT_SECONDS = 10;

/** More than this much video after the credits is an epilogue (or a preview) worth watching. */
export const EPILOGUE_SECONDS = 30;

/** Credits that run to (nearly) the end of the file; credits followed by an epilogue are not final. */
export function isFinalCredits(m: MarkerOccurrence, duration: number): boolean {
  return m.kind === "credits" && m.end_seconds >= duration - EPILOGUE_SECONDS;
}

/**
 * Media time the up-next countdown starts: the final credits, else 20 s before the end; never later than end − 12 s.
 * Credits with an epilogue after them do not start it (they get a skip button), so the epilogue is not cut off.
 */
export function upNextStart(markers: MarkerOccurrence[], duration: number): number {
  const credits = markers
    .filter((m) => isFinalCredits(m, duration))
    .sort((a, b) => b.start_seconds - a.start_seconds)[0]?.start_seconds;
  return Math.min(credits ?? duration - 20, duration - (UP_NEXT_SECONDS + 2));
}

/** Media flyout lines: "Series" / "S1 E2 · Title" for episodes, the title alone otherwise. */
export function mediaLines(w: WatchDetail): { title: string; subtitle?: string } {
  if (w.series_title) {
    const se =
      w.season_number != null && w.episode_number != null
        ? `S${String(w.season_number)} E${String(w.episode_number)} · `
        : "";
    return { title: w.series_title, subtitle: `${se}${w.title}` };
  }
  return { title: w.title };
}

/** Where the up-next countdown is anchored: the first position seen while eligible; cleared on a seek back before the
 *  window. Anchoring (instead of counting from the window start) gives a file that starts inside the window the full
 *  countdown rather than an instant jump to the next episode. */
export function upNextAnchorFor(
  anchor: number | null,
  eligible: boolean,
  position: number,
  from: number,
): number | null {
  if (anchor !== null && position < from) return null;
  if (anchor === null && eligible) return position;
  return anchor;
}

/** Whole seconds left on the up-next countdown, in media time (so it pauses with the video). */
export function upNextRemaining(anchor: number | null, position: number): number {
  if (anchor === null) return UP_NEXT_SECONDS;
  return Math.max(0, Math.min(UP_NEXT_SECONDS, Math.ceil(UP_NEXT_SECONDS - (position - anchor))));
}
