import type { CatalogItemOverlay } from "./catalog";
import { codecName } from "./tracks";

/** "1h 50m" / "45m". */
export function runtimeLabel(minutes: number | null | undefined): string | null {
  if (!minutes || minutes <= 0) return null;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return h > 0 ? `${String(h)}h${m > 0 ? ` ${String(m)}m` : ""}` : `${String(m)}m`;
}

/** Technical tags (resolution, HDR, audio, edition) from an item's overlay summary. */
export function qualityTags(o: CatalogItemOverlay | null | undefined): string[] {
  if (!o) return [];
  const audio = [o.audio ? (/^[a-z0-9-]+$/i.test(o.audio) ? codecName(o.audio) : o.audio) : null, o.audio_channels]
    .filter(Boolean)
    .join(" ");
  return [o.resolution, o.hdr, audio || null, o.edition].filter((x): x is string => Boolean(x));
}
