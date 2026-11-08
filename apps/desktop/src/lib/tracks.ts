/*
 * Track labels shared by the detail page and the player: the language is always the headline (the web app often
 * shows a release-group title instead, which hides the one thing people pick by), details go into small badges.
 */

/** Human language name ("Deutsch", "English") for a code like `ger`, `de`, `pt-BR`; "" when unknown. */
export function languageName(code: string | null | undefined, locale: string): string {
  const raw = (code ?? "").trim();
  if (!raw || raw === "und" || raw === "zxx" || raw === "mul") return "";
  try {
    const name = new Intl.DisplayNames([locale], { type: "language", fallback: "none" }).of(
      ISO639_2[raw.toLowerCase()] ?? raw.replace(/_/g, "-"),
    );
    if (name) return name.charAt(0).toLocaleUpperCase(locale) + name.slice(1);
  } catch {
    /* not a language tag: fall through */
  }
  return raw.length <= 3 ? raw.toUpperCase() : raw;
}

/* Bibliographic ISO 639-2 codes Intl does not know (it handles the terminology ones). */
const ISO639_2: Record<string, string> = {
  ger: "de",
  fre: "fr",
  dut: "nl",
  chi: "zh",
  cze: "cs",
  gre: "el",
  per: "fa",
  rum: "ro",
  slo: "sk",
  ice: "is",
  mac: "mk",
  may: "ms",
  bur: "my",
  arm: "hy",
  baq: "eu",
  geo: "ka",
  alb: "sq",
  wel: "cy",
  tib: "bo",
};

const CODECS: Record<string, string> = {
  truehd: "TrueHD",
  eac3: "DD+",
  ac3: "DD",
  dts: "DTS",
  "dts-hd ma": "DTS-HD MA",
  dtshd: "DTS-HD",
  aac: "AAC",
  flac: "FLAC",
  opus: "Opus",
  mp3: "MP3",
  vorbis: "Vorbis",
  pcm_s16le: "PCM",
  pcm_s24le: "PCM",
  subrip: "SRT",
  srt: "SRT",
  ass: "ASS",
  ssa: "SSA",
  hdmv_pgs_subtitle: "PGS",
  pgssub: "PGS",
  pgs: "PGS",
  dvd_subtitle: "VobSub",
  dvdsub: "VobSub",
  mov_text: "TX3G",
  webvtt: "VTT",
};

export const codecName = (codec: string | null | undefined): string | null => {
  if (!codec) return null;
  return CODECS[codec.toLowerCase()] ?? codec.toUpperCase();
};

/** "7.1", "5.1", "Stereo", "Mono" from a layout string or channel count. */
export function channelName(layout: string | null | undefined, channels: number | null | undefined): string | null {
  const l = (layout ?? "").toLowerCase();
  if (l.includes("7.1")) return "7.1";
  if (l.includes("5.1")) return "5.1";
  if (l === "stereo") return "Stereo";
  if (l === "mono") return "Mono";
  switch (channels) {
    case 1:
      return "Mono";
    case 2:
      return "Stereo";
    case 6:
      return "5.1";
    case 8:
      return "7.1";
    default:
      return channels ? `${String(channels)}ch` : layout || null;
  }
}

export interface TrackInfo {
  /** Language name, or the title when there is no language. */
  headline: string;
  /** The track's own title when it adds something (commentary, release name). */
  detail: string | null;
  badges: string[];
}

interface TrackLike {
  language?: string | null;
  title?: string | null;
  embedded_title?: string | null;
  codec?: string | null;
}

function base(tr: TrackLike, locale: string, fallback: string): Pick<TrackInfo, "headline" | "detail"> {
  const lang = languageName(tr.language, locale);
  const title = (tr.title ?? tr.embedded_title ?? "").trim();
  const headline = lang || title || fallback;
  const detail = title && title.toLowerCase() !== headline.toLowerCase() ? title : null;
  return { headline, detail };
}

export function audioInfo(
  a: TrackLike & { layout?: string | null; channels?: number | null; default?: boolean; profile?: string | null },
  locale: string,
  labels: { fallback: string; default: string },
): TrackInfo {
  const atmos = /atmos/i.test(`${a.profile ?? ""} ${a.title ?? ""} ${a.embedded_title ?? ""}`);
  return {
    ...base(a, locale, labels.fallback),
    badges: [
      codecName(a.codec),
      atmos ? "Atmos" : null,
      channelName(a.layout, a.channels),
      a.default ? labels.default : null,
    ].filter((x): x is string => Boolean(x)),
  };
}

export function subtitleInfo(
  s: TrackLike & { forced?: boolean; hearing_impaired?: boolean; default?: boolean; external?: boolean },
  locale: string,
  labels: { fallback: string; forced: string; sdh: string; default: string; external: string },
): TrackInfo {
  return {
    ...base(s, locale, labels.fallback),
    badges: [
      s.forced ? labels.forced : null,
      s.hearing_impaired ? labels.sdh : null,
      codecName(s.codec),
      s.external ? labels.external : null,
      s.default ? labels.default : null,
    ].filter((x): x is string => Boolean(x)),
  };
}

/** One-line summary for a closed picker ("English · TrueHD · 7.1"). */
export const trackSummary = (i: TrackInfo, max = 2) => [i.headline, ...i.badges.slice(0, max)].join(" · ");
