import type { FileVersion, PlayRequest, SubtitleTrack, WatchDetail } from "./catalog";

/** Below this many seconds a stored position is not offered as "resume" (matches the web app's behaviour of
 * treating a few seconds in as "not started"). */
export const MIN_RESUME_SECONDS = 30;

export interface PlayChoice {
  version: FileVersion;
  audioIndex: number | null;
  subtitleIndex: number | null;
}

/** The version to play: the one last watched if it still exists, else the first (the server orders them). */
export function defaultVersion(w: WatchDetail): FileVersion | undefined {
  const last = w.user_data?.last_file_id;
  return w.versions.find((v) => v.file_id === last) ?? w.versions[0];
}

/** Audio track position: the server's profile-resolved choice, else the default-flagged track, else the first. */
export function defaultAudio(v: FileVersion): number | null {
  if (v.audio_tracks.length === 0) return null;
  const eff = v.effective_audio_track_index;
  if (eff !== undefined && eff !== null && eff >= 0 && eff < v.audio_tracks.length) return eff;
  const flagged = v.audio_tracks.findIndex((t) => t.default);
  return flagged >= 0 ? flagged : 0;
}

const lang = (l: string | null | undefined) => (l ?? "").toLowerCase().split("-")[0] ?? "";

/**
 * Subtitle choice from the profile-resolved preference. Vocabulary per OAS `Profile.subtitle_mode`@6791aac ("auto,
 * always, off, default, forced_only; empty when unset"): `off` → none; `forced_only` → forced track in the
 * language; `always` → full track in the language; `auto` → forced track when the audio is already in that
 * language, else the full track; `default`/empty → the track the file flags as default.
 */
export function defaultSubtitle(
  w: WatchDetail,
  v: FileVersion,
  audioIndex: number | null = defaultAudio(v),
): number | null {
  const mode = (w.effective_subtitle_mode ?? "").toLowerCase();
  const tracks = v.subtitle_tracks.filter((t): t is SubtitleTrack & { index: number } => typeof t.index === "number");
  if (tracks.length === 0 || mode === "off") return null;
  const want = lang(w.effective_subtitle_language);
  const inLang = want === "" ? [] : tracks.filter((t) => lang(t.language) === want);
  const forced = inLang.find((t) => t.forced)?.index ?? null;
  const full = inLang.find((t) => !t.forced)?.index ?? null;
  switch (mode) {
    case "forced_only":
      return forced;
    case "always":
      return full ?? forced;
    case "auto": {
      const audioLang = audioIndex === null ? "" : lang(v.audio_tracks[audioIndex]?.language);
      return audioLang !== "" && audioLang === want ? forced : (full ?? forced);
    }
    default:
      return tracks.find((t) => t.default)?.index ?? null;
  }
}

/** Resume position in seconds, or 0 when the item is finished or barely started. */
export function resumePosition(w: WatchDetail): number {
  const u = w.user_data;
  if (!u || u.played) return 0;
  const p = u.position_seconds ?? 0;
  return p >= MIN_RESUME_SECONDS ? Math.floor(p) : 0;
}

export function defaultChoice(w: WatchDetail): PlayChoice | undefined {
  const version = defaultVersion(w);
  if (!version) return undefined;
  const audioIndex = defaultAudio(version);
  return { version, audioIndex, subtitleIndex: defaultSubtitle(w, version, audioIndex) };
}

export function playRequest(serverId: string, w: WatchDetail, choice: PlayChoice, fromStart: boolean): PlayRequest {
  return {
    serverId,
    itemId: w.content_id,
    fileId: choice.version.file_id,
    startSeconds: fromStart ? 0 : resumePosition(w),
    audioIndex: choice.audioIndex,
    subtitleIndex: choice.subtitleIndex,
  };
}

/** "1:02:03" / "4:05". */
export function formatTime(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m);
  return `${h > 0 ? `${String(h)}:` : ""}${mm}:${String(sec).padStart(2, "0")}`;
}
