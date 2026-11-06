import type { FileVersion, PlayRequest, SubtitleTrack, WatchDetail, WatchSubtitleSignature } from "./catalog";

export interface PlayChoice {
  version: FileVersion;
  audioIndex: number | null;
  subtitleIndex: number | null;
}

/**
 * The version to request. Mirrors `selectFileId` in `web/src/player/hooks/usePlaybackSession.ts@6791aac`: the version
 * last played if it still exists, else one of the last-played edition (`last_edition_key`, falling back to the
 * profile's `effective_version_edition_key`), else one without an edition, else the first. Which encode of that cut
 * plays is the server's decision (playback plan).
 */
export function defaultVersion(w: WatchDetail): FileVersion | undefined {
  const last = w.user_data?.last_file_id;
  const exact = last ? w.versions.find((v) => v.file_id === last) : undefined;
  if (exact) return exact;
  const edition = w.user_data?.last_edition_key ?? w.effective_version_edition_key;
  const ofEdition = edition ? w.versions.find((v) => v.edition_key === edition) : undefined;
  return ofEdition ?? w.versions.find((v) => !v.edition_key) ?? w.versions[0];
}

/** Audio track position: the server's profile-resolved choice, else the default-flagged track, else the first. */
export function defaultAudio(v: FileVersion): number | null {
  if (v.audio_tracks.length === 0) return null;
  const eff = v.effective_audio_track_index;
  if (eff !== undefined && eff !== null && eff >= 0 && eff < v.audio_tracks.length) return eff;
  const flagged = v.audio_tracks.findIndex((t) => t.default);
  return flagged >= 0 ? flagged : 0;
}

/*
 * Subtitle auto-selection, ported from `web/src/player/utils/subtitleSort.ts@6791aac` (`resolveSubtitleAutoSelect`,
 * `findPreferredSubtitleIndexWithSignature`) and `web/src/lib/languageTags.ts@6791aac`, so Granary starts with the
 * track the web app would. One deliberate difference: mpv draws every subtitle format itself, so the web app's
 * "needs burn-in" tier never applies.
 */

const LANGUAGE_ALIASES: Record<string, string> = {
  arabic: "ar",
  english: "en",
  spanish: "es",
  french: "fr",
  german: "de",
  portuguese: "pt",
  "brazilian portuguese": "pt-BR",
  "portuguese (brazil)": "pt-BR",
  "chinese (traditional)": "zh-Hant",
  "traditional chinese": "zh-Hant",
};

/** Canonical BCP 47 tag for comparison only (`eng` → `en`, `ger` → `de`); null when not a language. */
export function canonicalLanguageTag(value: string | null | undefined): string | null {
  const raw = (value ?? "").trim();
  const trimmed = (LANGUAGE_ALIASES[raw.toLowerCase()] ?? raw).trim();
  if (!trimmed) return null;
  try {
    const locale = new Intl.Locale(trimmed.replace(/_/g, "-"));
    if (!/^[a-z]{2,3}$/i.test(locale.language)) return null;
    return locale.toString();
  } catch {
    return null;
  }
}

/** Canonical primary language subtag, or "" when unknown. */
export function languageCode(value: string | null | undefined): string {
  const tag = canonicalLanguageTag(value);
  return tag ? new Intl.Locale(tag).language : "";
}

const norm = (v: string | null | undefined) => (v ?? "").trim().toLowerCase();

/** A concrete language: not empty, not `und`, not the "original language" sentinel. */
function concreteLanguage(value: string | null | undefined): string | null {
  const n = norm(value);
  return n === "" || n === "und" || n === "original" || n === "x-silo-original" ? null : n;
}

function sameLanguage(a: string | null, b: string | null): boolean {
  const left = languageCode(concreteLanguage(a));
  return left !== "" && left === languageCode(concreteLanguage(b));
}

/** 0 exact tag, 1 bare language, 2 another variant of the language, -1 no match. */
function languageMatchRank(candidate: string | null | undefined, preferred: string): number {
  const c = canonicalLanguageTag(candidate);
  const p = canonicalLanguageTag(preferred);
  if (!c || !p) return -1;
  if (c === p) return 0;
  if (languageCode(c) !== languageCode(p)) return -1;
  return c.includes("-") ? 2 : 1;
}

const BITMAP_CODECS = new Set([
  "pgs",
  "pgssub",
  "hdmv_pgs_subtitle",
  "dvd_subtitle",
  "dvdsub",
  "vobsub",
  "dvb_subtitle",
  "dvbsub",
  "dvb_teletext",
]);

type IndexedTrack = SubtitleTrack & { index: number };

const source = (t: SubtitleTrack) => (t.external ? "external" : "embedded");
const label = (t: SubtitleTrack) => t.title ?? t.language ?? "";

/** Lower is better: full beats forced, plain beats SDH, text beats bitmap, embedded beats external. */
function trackPriority(t: SubtitleTrack): number {
  return (
    (t.forced ? 16 : 0) +
    (t.hearing_impaired ? 8 : 0) +
    (BITMAP_CODECS.has(norm(t.codec)) ? 4 : 0) +
    (t.external ? 1 : 0)
  );
}

function matchesSignature(t: SubtitleTrack, s: WatchSubtitleSignature): boolean {
  return (
    norm(source(t)) === norm(s.source) &&
    norm(t.language) === norm(s.language) &&
    norm(t.codec) === norm(s.codec) &&
    (norm(s.label) === "" || norm(label(t)) === norm(s.label)) &&
    t.forced === s.forced &&
    t.hearing_impaired === s.hearing_impaired
  );
}

function signatureScore(t: SubtitleTrack, s: WatchSubtitleSignature | null | undefined): number {
  if (!s) return 0;
  return (
    (norm(source(t)) === norm(s.source) ? 4 : 0) +
    (t.forced === s.forced ? 2 : 0) +
    (t.hearing_impaired === s.hearing_impaired ? 2 : 0) +
    (norm(t.codec) === norm(s.codec) ? 1 : 0) +
    (norm(label(t)) === norm(s.label) ? 1 : 0)
  );
}

function bestForLanguage(
  tracks: IndexedTrack[],
  language: string,
  signature: WatchSubtitleSignature | null | undefined,
): number | null {
  let best: IndexedTrack | null = null;
  let bestRank = 3;
  let bestScore = -1;
  let bestPriority = Infinity;
  for (const t of tracks) {
    const rank = languageMatchRank(t.language, language);
    if (rank < 0) continue;
    const score = signatureScore(t, signature);
    const priority = trackPriority(t);
    if (
      best === null ||
      rank < bestRank ||
      (rank === bestRank && (score > bestScore || (score === bestScore && priority < bestPriority)))
    ) {
      best = t;
      bestRank = rank;
      bestScore = score;
      bestPriority = priority;
    }
  }
  return best?.index ?? null;
}

/**
 * The subtitle to start with, from the server-resolved profile preferences (`effective_subtitle_mode`, absent →
 * `auto` as in `watchRouteHelpers.ts@6791aac` L297-301; `effective_show_forced_subtitles`, absent → true):
 * - `off`: only the forced track in the audio language (when forced subtitles show);
 * - `always`: the track last chosen on this item (exact signature), else the best track in the subtitle language;
 * - `auto`: no subtitle language set, or audio already in it → the forced track for the audio language only; else
 *   the best full track in the subtitle language; an explicit empty language means none;
 * - anything else (the vocabulary is free-form upstream, `internal/apiv2/profiles.go@6791aac` L43): none.
 */
export function defaultSubtitle(
  w: WatchDetail,
  v: FileVersion,
  audioIndex: number | null = defaultAudio(v),
): number | null {
  const tracks = v.subtitle_tracks.filter((t): t is IndexedTrack => typeof t.index === "number");
  if (tracks.length === 0) return null;
  const mode = norm(w.effective_subtitle_mode ?? "auto");
  const preferred = w.effective_subtitle_language ?? null;
  const signature = w.effective_subtitle_track_signature;
  const showForced = w.effective_show_forced_subtitles ?? true;
  const audioLang = concreteLanguage(audioIndex === null ? null : v.audio_tracks[audioIndex]?.language);
  const forcedForAudio = () =>
    showForced && audioLang
      ? bestForLanguage(
          tracks.filter((t) => t.forced),
          audioLang,
          null,
        )
      : null;

  switch (mode) {
    case "off":
      return forcedForAudio();
    case "always": {
      const exact = signature ? tracks.find((t) => matchesSignature(t, signature)) : undefined;
      if (exact) return exact.index;
      return preferred ? bestForLanguage(tracks, preferred, signature) : null;
    }
    case "auto": {
      if (preferred === "") return null;
      const want = concreteLanguage(preferred);
      if (!want || sameLanguage(audioLang, want)) return forcedForAudio();
      return bestForLanguage(tracks, want, signature);
    }
    default:
      return null;
  }
}

/**
 * Resume position in seconds. Watched items store position 0, so any nonzero position is a live resume point,
 * including a rewatch in progress (`watchRouteHelpers.ts@6791aac` L376-378); resume thresholds are applied by the
 * server (`playback.min_resume_threshold`).
 */
export function resumePosition(w: WatchDetail): number {
  const p = w.user_data?.position_seconds ?? 0;
  return p > 0 ? Math.floor(p) : 0;
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
