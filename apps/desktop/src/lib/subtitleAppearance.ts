/**
 * The profile's subtitle appearance (`playback.subtitle_appearance`,
 * `silo-server/contracts/settings/v1/schemas/subtitle-appearance.json@749db96a`): a sparse JSON object the server
 * stores per profile, and per device as an override. The host turns it into mpv options; this module reads and edits it.
 */

export interface SubtitleAppearance {
  fontSize?: "small" | "medium" | "large" | "xlarge" | "xxlarge";
  fontFamily?: string;
  fontColor?: string;
  backgroundColor?: string;
  backgroundStyle?: "box" | "shadow" | "outline" | "none";
  backgroundOpacity?: number;
  textOpacity?: number;
  textOutline?: boolean;
  textOutlineColor?: string;
  position?: "bottom" | "lower-third" | "top";
}

/** The contract's complete default (`contracts/settings/v1/manifest.json@749db96a`, revision 14). */
export const DEFAULT_APPEARANCE: Required<SubtitleAppearance> = {
  fontSize: "large",
  fontFamily: "sans-serif",
  fontColor: "#ffffff",
  backgroundColor: "#000000",
  backgroundStyle: "box",
  backgroundOpacity: 75,
  textOpacity: 100,
  textOutline: false,
  textOutlineColor: "#000000",
  position: "bottom",
};

export const FONT_SIZES = ["small", "medium", "large", "xlarge", "xxlarge"] as const;
export const BACKGROUND_STYLES = ["box", "shadow", "outline", "none"] as const;
export const POSITIONS = ["bottom", "lower-third", "top"] as const;
/** Text colours offered in the player; the stored value may be any `#rrggbb`. */
export const FONT_COLORS = ["#ffffff", "#ffff00", "#00ffff", "#00ff00", "#ff00ff"] as const;

/** A stored value (empty when unset); anything that is not a JSON object reads as no override. */
export function parseAppearance(value: string): SubtitleAppearance {
  try {
    const v: unknown = JSON.parse(value);
    return v !== null && typeof v === "object" && !Array.isArray(v) ? v : {};
  } catch {
    return {};
  }
}

/** What a member reads as: the stored value, else the default. */
export function effective<K extends keyof SubtitleAppearance>(
  a: SubtitleAppearance,
  key: K,
): Required<SubtitleAppearance>[K] {
  return (a[key] ?? DEFAULT_APPEARANCE[key]) as Required<SubtitleAppearance>[K];
}

/** `a` with one member changed, as the JSON document to store. */
export function withMember<K extends keyof SubtitleAppearance>(
  a: SubtitleAppearance,
  key: K,
  value: SubtitleAppearance[K],
): string {
  return JSON.stringify({ ...a, [key]: value });
}
