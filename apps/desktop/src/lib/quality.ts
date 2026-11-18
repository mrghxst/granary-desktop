import { invoke, isTauri } from "@tauri-apps/api/core";
import type { BufferingMode } from "./bufferingQuality";
import type { PlanInfo, PlanSource } from "./player";

/**
 * Streaming quality (ADR-0014): the profile's settings on this computer, this device's connection reading, quality
 * changes while playing, and how a plan is explained to the viewer (Direct Play, Direct Stream or Transcoding).
 */

export interface QualitySettings {
  /** `auto`, `480p`, `720p`, `1080p` or `2160p`. */
  preferredQuality: string;
  maxBitrateKbps: number | null;
  /** `null`: this server does not have the setting (the controller runs `always`). */
  bufferingMode: BufferingMode | null;
  deviceOverrides: string[];
}

export interface LearnedLimit {
  kbps: number;
  observed_at: string;
  expires_at: string;
}

export interface PlaybackConnection {
  connection: string;
  learned_limit?: LearnedLimit | null;
  probe?: { kbps: number; lower_bound: boolean; measured_at?: string | null; transport?: string | null } | null;
  playback_sample?: { kbps: number; measured_at?: string | null } | null;
  auto_budget_kbps?: number | null;
}

export interface QualityRequest {
  quality: string;
  estimateKbps?: number;
  estimateSource?: "observed_rebuffer";
  observedKbps?: number;
  clearLearnedLimit?: boolean;
}

export interface NetworkReading {
  /** Bytes per second. */
  cacheSpeed?: number | null;
  cacheAhead?: number | null;
  cacheIdle: boolean;
}

function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri()) return Promise.reject(Object.assign(new Error("not running inside Granary"), { code: "no_host" }));
  return invoke<T>(cmd, args);
}

export const QUALITY_KEYS = {
  preferred: "playback.preferred_quality",
  maxBitrate: "playback.max_bitrate_kbps",
  buffering: "playback.buffering_quality_mode",
} as const;

const CACHE_PREFIX = "granary.quality.";

function remember(serverId: string, s: QualitySettings) {
  try {
    localStorage.setItem(CACHE_PREFIX + serverId, JSON.stringify(s));
  } catch {
    // Storage unavailable: the next start reads the server again.
  }
}

export const DEFAULT_QUALITY: QualitySettings = {
  preferredQuality: "auto",
  maxBitrateKbps: null,
  bufferingMode: "always",
  deviceOverrides: [],
};

/**
 * The last settings seen for this server on this computer. A start never waits for the settings read (the design's
 * "The setting"); it uses these and refreshes them in the background.
 */
export function cachedQuality(serverId: string): QualitySettings {
  try {
    const raw = localStorage.getItem(CACHE_PREFIX + serverId);
    if (raw) return { ...DEFAULT_QUALITY, ...(JSON.parse(raw) as Partial<QualitySettings>) };
  } catch {
    // Unreadable: the defaults.
  }
  return DEFAULT_QUALITY;
}

export const quality = {
  settings: async (serverId: string) => {
    const s = await call<QualitySettings>("quality_settings", { serverId });
    remember(serverId, s);
    return s;
  },
  set: async (serverId: string, key: string, value: string | number | null | undefined) => {
    const s = await call<QualitySettings>("quality_setting_set", { serverId, key, value: value ?? null, clear: false });
    remember(serverId, s);
    return s;
  },
  /** Clear this computer's value so the profile's applies. */
  reset: async (serverId: string, key: string) => {
    const s = await call<QualitySettings>("quality_setting_set", { serverId, key, clear: true });
    remember(serverId, s);
    return s;
  },
  connection: (serverId: string) => call<PlaybackConnection | null>("playback_connection", { serverId }),
  resetLearnedLimit: (serverId: string) => call<null>("playback_learned_limit_reset", { serverId }),
  /** Ask the server for another quality for what plays (`quality_change`). */
  change: (request: QualityRequest) => call<null>("player_quality", { request }),
  network: () => call<NetworkReading | null>("player_network"),
};

/* ---------------------------------------------------------------- formatting */

/**
 * A rate as the design's Settings shows it, rounded down so a limit or budget never overstates the link: kbps below
 * 1 Mbps, one decimal below 10 Mbps, whole Mbps from 10.
 */
export function formatKbps(kbps: number | null | undefined): string {
  if (kbps == null || !Number.isFinite(kbps) || kbps <= 0) return "";
  if (kbps < 1000) return `${String(Math.floor(kbps))} kbps`;
  if (kbps < 10_000) return `${(Math.floor(kbps / 100) / 10).toFixed(1).replace(/\.0$/, "")} Mbps`;
  return `${String(Math.floor(kbps / 1000))} Mbps`;
}

/** "4K", "1080p", "720p", ... from a picture size: the server's resolution classes (§10). */
export function resolutionLabel(width?: number | null, height?: number | null): string {
  if (!height || height <= 0) return "";
  const w = width && width > 0 ? width : Math.round((height * 16) / 9);
  if (w <= 640 && height <= 360) return "360p";
  if (w <= 854 && height <= 480) return "480p";
  if (w <= 1280 && height <= 962) return "720p";
  if (w <= 2560 && height <= 1440) return "1080p";
  if (w <= 4096 && height <= 3072) return "4K";
  return "8K";
}

const CODECS: Record<string, string> = {
  h264: "H.264",
  avc: "H.264",
  hevc: "HEVC",
  h265: "HEVC",
  av1: "AV1",
  vp9: "VP9",
  mpeg2video: "MPEG-2",
  vc1: "VC-1",
  aac: "AAC",
  ac3: "Dolby Digital",
  eac3: "Dolby Digital+",
  truehd: "TrueHD",
  dts: "DTS",
  flac: "FLAC",
  opus: "Opus",
  mp3: "MP3",
};

export function codecLabel(codec?: string | null): string {
  if (!codec) return "";
  return CODECS[codec.toLowerCase()] ?? codec.toUpperCase();
}

export function rangeLabel(range?: string | null): string {
  switch ((range ?? "").toLowerCase()) {
    case "":
      return "";
    case "sdr":
      return "SDR";
    case "hdr10":
      return "HDR10";
    case "hdr10plus":
    case "hdr10+":
      return "HDR10+";
    case "hlg":
      return "HLG";
    case "dolby_vision":
    case "dovi":
      return "Dolby Vision";
    default:
      return range?.toUpperCase() ?? "";
  }
}

function channelsLabel(n?: number | null): string {
  if (!n) return "";
  return n === 1 ? "mono" : n === 2 ? "stereo" : n === 6 ? "5.1" : n === 8 ? "7.1" : `${String(n)} ch`;
}

/** One line about a stream: "4K · HEVC · HDR10 · 19.1 Mbps". */
export function streamLine(s: PlanSource | null | undefined, kbps?: number | null): string {
  if (!s) return "";
  return [
    resolutionLabel(s.width, s.height),
    codecLabel(s.video_codec),
    rangeLabel(s.dynamic_range),
    formatKbps(kbps ?? s.bitrate_kbps),
  ]
    .filter(Boolean)
    .join(" · ");
}

export function audioLine(s: PlanSource | null | undefined): string {
  if (!s) return "";
  return [codecLabel(s.audio_codec), channelsLabel(s.audio_channels)].filter(Boolean).join(" ");
}

/* ---------------------------------------------------------------- what plays and why */

export type PlayMethod = "direct_play" | "direct_stream" | "transcode";

/** How the file reaches the player (the server's three delivery families). */
export function playMethod(plan: Pick<PlanInfo, "delivery" | "direct_play" | "transformations">): PlayMethod {
  if (plan.direct_play || plan.delivery === "original_http") return "direct_play";
  if (plan.delivery.startsWith("server_transcode")) return "transcode";
  // A remux that converts something (an audio-only adaptation) is still a conversion.
  if (plan.transformations.some((t) => t.startsWith("video_to") || t === "audio_to_aac")) return "transcode";
  return "direct_stream";
}

/** Whether a transcode converts the picture or only the sound. */
export function convertsVideo(plan: Pick<PlanInfo, "transformations" | "source" | "delivered">): boolean {
  const t = plan.transformations;
  if (t.some((x) => x.startsWith("video_to") || x === "hdr_to_sdr_tonemap")) return true;
  const a = plan.source;
  const b = plan.delivered;
  return Boolean(
    (a.video_codec && b.video_codec && a.video_codec !== b.video_codec) ||
    (a.height && b.height && b.height < a.height),
  );
}

/** The method's name: "Direct Play", "Direct Stream", "Transcoding" or "Transcoding audio". */
export function methodName(
  t: (k: string) => string,
  plan: Pick<PlanInfo, "delivery" | "direct_play" | "transformations" | "source" | "delivered">,
): string {
  const m = playMethod(plan);
  if (m === "transcode" && !convertsVideo(plan)) return t("quality.method.transcodeAudio");
  return t(`quality.method.${m}`);
}

/** "1080p · 6 Mbps" for what the player receives. */
export function deliveredSummary(plan: Pick<PlanInfo, "delivered" | "delivered_kbps">): string {
  const d = plan.delivered;
  return [resolutionLabel(d.width, d.height), rangeIfHdr(d.dynamic_range), formatKbps(plan.delivered_kbps)]
    .filter(Boolean)
    .join(" · ");
}

function rangeIfHdr(range?: string | null): string {
  const r = (range ?? "").toLowerCase();
  if (!r || r === "sdr") return "";
  return r === "dolby_vision" || r === "dovi" ? "DV" : r === "hdr10plus" ? "HDR10+" : r.toUpperCase();
}

/** A translation key with its values (rendered by the caller's `t`). */
export interface Reason {
  key: string;
  values?: Record<string, string | number>;
}

/** The rung the plan plays, by its menu name. */
export function qualityName(plan: Pick<PlanInfo, "available_qualities">, label: string | null | undefined): string {
  if (!label) return "";
  return plan.available_qualities.find((q) => q.label === label)?.display_name ?? label;
}

const DECISION_REASONS: Record<string, string> = {
  quality_fixed_rung: "quality.why.picked",
  quality_bandwidth_cap: "quality.why.cap",
  quality_bandwidth_limit: "quality.why.bandwidth",
  quality_device_limit: "quality.why.device",
  container_normalization: "quality.why.container",
  hls_packaging_required: "quality.why.packaging",
  audio_adaptation: "quality.why.audio",
  subtitle_burn_in_required: "quality.why.burnIn",
  client_managed_dynamic_range: "quality.why.dynamicRange",
  client_dv8_base_layer: "quality.why.dynamicRange",
  client_dv7_to_hdr10: "quality.why.dynamicRange",
  client_dv7_to_dv81: "quality.why.dynamicRange",
  evidence_insufficient_for_direct: "quality.why.unconfirmed",
  copy_exhausted: "quality.why.unconfirmed",
};

const DIRECT_REASONS = new Set([
  "direct_play",
  "validated_original_playback",
  "quality_auto_source",
  "quality_original",
  "",
]);

const TRANSFORMATIONS: Record<string, string> = {
  video_to_h264: "quality.change.videoH264",
  video_to_hevc: "quality.change.videoHevc",
  audio_to_aac: "quality.change.audioAac",
  hdr_to_sdr_tonemap: "quality.change.toneMap",
  server_dv7_to_hdr10: "quality.change.dvToHdr10",
  server_dv7_to_dv81: "quality.change.dvToDv81",
  client_dv7_to_hdr10: "quality.change.dvToHdr10",
  client_dv7_to_dv81: "quality.change.dvToDv81",
};

const WARNINGS: Record<string, string> = {
  subtitle_burn_in: "quality.warning.burnIn",
  bandwidth_cap_applied: "quality.warning.cap",
  quality_reduction_unavailable: "quality.warning.noLower",
  quality_preference_normalized: "quality.warning.normalized",
  hdr_range_assumed_hdr10: "quality.warning.assumedHdr10",
  dolby_vision_enhancement_layer_discarded: "quality.warning.dvLayer",
  dolby_vision_strip_unsupported_by_source: "quality.warning.dvStrip",
};

/**
 * Why the plan plays the way it does, as plain-language lines: the server's reason, then what was changed (size,
 * bitrate, codec, dynamic range, audio) and what the server warned about. Server warning text without a translation
 * is shown as the server wrote it.
 */
export function explainPlan(plan: PlanInfo): { reasons: Reason[]; changes: Reason[]; warnings: (Reason | string)[] } {
  const reasons: Reason[] = [];
  const method = playMethod(plan);
  const src = plan.source;
  const out = plan.delivered;
  const reason = plan.decision_reason;
  const key = DECISION_REASONS[reason];
  if (key) {
    reasons.push({
      key,
      values: {
        quality: qualityName(plan, plan.active_quality ?? plan.quality_preference),
        cap: formatKbps(plan.cap_kbps),
        source: formatKbps(src.bitrate_kbps),
      },
    });
  } else if (method === "direct_play" || DIRECT_REASONS.has(reason)) {
    reasons.push({ key: plan.quality_preference === "auto" ? "quality.why.autoOriginal" : "quality.why.original" });
  } else {
    reasons.push({ key: "quality.why.other", values: { reason } });
  }
  if (plan.bandwidth_limit) {
    reasons.push({ key: "quality.why.learned", values: { limit: formatKbps(plan.bandwidth_limit.kbps) } });
  }
  const changes: Reason[] = [];
  if (method !== "direct_play") {
    const from = resolutionLabel(src.width, src.height);
    const to = resolutionLabel(out.width, out.height);
    if (from && to && from !== to) changes.push({ key: "quality.change.resolution", values: { from, to } });
    const fromK = src.bitrate_kbps ?? 0;
    const toK = plan.delivered_kbps ?? 0;
    if (fromK > 0 && toK > 0 && toK < fromK * 0.95) {
      changes.push({ key: "quality.change.bitrate", values: { from: formatKbps(fromK), to: formatKbps(toK) } });
    }
  }
  for (const name of plan.transformations) {
    changes.push(
      TRANSFORMATIONS[name] ? { key: TRANSFORMATIONS[name] } : { key: "quality.change.other", values: { name } },
    );
  }
  if (method === "direct_stream" && changes.length === 0) changes.push({ key: "quality.change.remux" });
  const warnings = plan.warnings.map((w) => (WARNINGS[w.code] ? { key: WARNINGS[w.code] ?? "" } : w.message || w.code));
  return { reasons, changes, warnings };
}

/** Plain resolutions offered before a file plays, up to its own (the setting's vocabulary). */
export function resolutionChoices(sourceResolution?: string | null): string[] {
  const r = (sourceResolution ?? "").toLowerCase();
  const height = r.includes("4k") || r.includes("2160") || r.includes("uhd") ? 2160 : Number.parseInt(r, 10) || 2160;
  return ["2160p", "1080p", "720p", "480p"].filter((q) => Number.parseInt(q, 10) <= Math.max(480, height));
}

/** Maximum bitrate presets in kbps (the design's 500 kbps link and up). */
export const BITRATE_PRESETS = [120_000, 80_000, 40_000, 20_000, 12_000, 8_000, 4_000, 2_000, 1_500, 800, 400];
