/** How the reader shows text and pages; kept on this computer only (a viewer convenience, not a server setting). */
export type ReaderTheme = "light" | "sepia" | "dark";

export interface ReaderLook {
  theme: ReaderTheme;
  /** Text size in percent. */
  fontSize: number;
  /** Comics read right to left (manga). */
  rtl: boolean;
}

const KEY = "granary.reader.look";
export const DEFAULT_LOOK: ReaderLook = { theme: "dark", fontSize: 110, rtl: false };
export const FONT_SIZES = [80, 90, 100, 110, 120, 135, 150, 170];

export const THEME_COLORS: Record<ReaderTheme, { bg: string; fg: string }> = {
  light: { bg: "#f6f3ee", fg: "#1d1b18" },
  sepia: { bg: "#f1e7d0", fg: "#3b2f1e" },
  dark: { bg: "#141417", fg: "#d9d6d0" },
};

export function loadLook(): ReaderLook {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULT_LOOK;
    const v = JSON.parse(raw) as Partial<ReaderLook>;
    return {
      theme: v.theme === "light" || v.theme === "sepia" || v.theme === "dark" ? v.theme : DEFAULT_LOOK.theme,
      fontSize: typeof v.fontSize === "number" && FONT_SIZES.includes(v.fontSize) ? v.fontSize : DEFAULT_LOOK.fontSize,
      rtl: v.rtl === true,
    };
  } catch {
    return DEFAULT_LOOK;
  }
}

export function saveLook(look: ReaderLook): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(look));
  } catch {
    // Storage can be unavailable; the reader still works with the defaults.
  }
}

/** Book page index for a fraction of a paged book (`page / count` is what is saved). */
export function pageFor(fraction: number, count: number): number {
  if (count <= 0) return 0;
  return Math.min(count - 1, Math.max(0, Math.ceil(fraction * count) - 1));
}
