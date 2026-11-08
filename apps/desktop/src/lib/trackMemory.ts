import type { AudioTrack, FileVersion, SubtitleTrack, WatchDetail } from "./catalog";
import { defaultSubtitle, sameLanguage, type PlayChoice } from "./play";

/*
 * Audio and subtitle picks carry over between episodes of a series. Track positions differ from file to file, so a
 * pick is stored as a description (language, codec, channels, flags) and matched against the next episode's tracks.
 * Kept per server and series in localStorage; a lost entry only means the profile defaults apply again.
 */

interface AudioPick {
  language: string | null;
  codec: string | null;
  channels: number | null;
  title: string | null;
}

interface SubtitlePick {
  language: string | null;
  codec: string | null;
  title: string | null;
  forced: boolean;
  hearing_impaired: boolean;
}

/** `subtitle: null` means the viewer turned subtitles off; a missing field means nothing was picked. */
export interface TrackPicks {
  audio?: AudioPick;
  subtitle?: SubtitlePick | null;
}

const key = (serverId: string, seriesId: string) => `granary.tracks.${serverId}.${seriesId}`;

function load(serverId: string, seriesId: string): TrackPicks {
  try {
    const raw = window.localStorage.getItem(key(serverId, seriesId));
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

const title = (t: { title?: string | null; embedded_title?: string | null }) => t.title ?? t.embedded_title ?? null;

/** Remember what the viewer picked on an episode (`undefined` fields are left as they were). */
export function rememberTracks(
  serverId: string,
  w: WatchDetail,
  v: FileVersion,
  pick: { audioIndex?: number | null | undefined; subtitleIndex?: number | null | undefined },
): void {
  if (!w.series_id) return;
  const picks = load(serverId, w.series_id);
  if (pick.audioIndex != null) {
    const a = v.audio_tracks[pick.audioIndex];
    if (a) {
      picks.audio = {
        language: a.language ?? null,
        codec: a.codec ?? null,
        channels: a.channels ?? null,
        title: title(a),
      };
    }
  }
  if (pick.subtitleIndex !== undefined) {
    const s = pick.subtitleIndex === null ? null : v.subtitle_tracks.find((t) => t.index === pick.subtitleIndex);
    if (s === null) picks.subtitle = null;
    else if (s) {
      picks.subtitle = {
        language: s.language ?? null,
        codec: s.codec ?? null,
        title: title(s),
        forced: s.forced,
        hearing_impaired: s.hearing_impaired,
      };
    }
  }
  try {
    window.localStorage.setItem(key(serverId, w.series_id), JSON.stringify(picks));
  } catch {
    // Storage blocked: picks just do not carry over.
  }
}

function bestAudio(tracks: AudioTrack[], p: AudioPick): number | null {
  let best: number | null = null;
  let bestScore = -1;
  tracks.forEach((t, i) => {
    if (!sameLanguage(t.language ?? null, p.language)) return;
    const score =
      (t.codec != null && t.codec === p.codec ? 2 : 0) +
      (t.channels != null && t.channels === p.channels ? 2 : 0) +
      (title(t) != null && title(t) === p.title ? 1 : 0);
    if (score > bestScore) {
      best = i;
      bestScore = score;
    }
  });
  return best;
}

function bestSubtitle(tracks: SubtitleTrack[], p: SubtitlePick): number | null {
  let best: number | null = null;
  let bestScore = -1;
  for (const t of tracks) {
    if (typeof t.index !== "number" || t.forced !== p.forced || !sameLanguage(t.language ?? null, p.language)) continue;
    const score =
      (t.hearing_impaired === p.hearing_impaired ? 4 : 0) +
      (t.codec != null && t.codec === p.codec ? 2 : 0) +
      (title(t) != null && title(t) === p.title ? 1 : 0);
    if (score > bestScore) {
      best = t.index;
      bestScore = score;
    }
  }
  return best;
}

/**
 * The episode's choice with the series' remembered picks applied. A pick with no matching track (another language
 * set on this file) falls back to the default; subtitles "off" still keeps a forced track the defaults chose.
 */
export function applyTrackPicks(serverId: string, w: WatchDetail, choice: PlayChoice): PlayChoice {
  if (!w.series_id) return choice;
  const picks = load(serverId, w.series_id);
  const v = choice.version;
  const audioIndex = (picks.audio && bestAudio(v.audio_tracks, picks.audio)) ?? choice.audioIndex;
  let subtitleIndex = audioIndex === choice.audioIndex ? choice.subtitleIndex : defaultSubtitle(w, v, audioIndex);
  if (picks.subtitle === null) {
    const forced = v.subtitle_tracks.find((t) => t.index === subtitleIndex)?.forced ?? false;
    if (!forced) subtitleIndex = null;
  } else if (picks.subtitle) {
    subtitleIndex = bestSubtitle(v.subtitle_tracks, picks.subtitle) ?? subtitleIndex;
  }
  return { version: v, audioIndex, subtitleIndex };
}
