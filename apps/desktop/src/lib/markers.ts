import type { FileMarkers, MarkerKind, MarkerRange } from "./catalog";

/** The segments the server stores, in the order the editor lists them. */
export const MARKER_KINDS: readonly MarkerKind[] = ["intro", "recap", "credits", "preview"];

/** Whole seconds as `m:ss` or `h:mm:ss`; empty for none (as `silo-server/web/src/lib/markers.ts@749db96a`). */
export function formatClock(seconds: number | null | undefined): string {
  if (seconds == null) return "";
  const total = Math.max(0, Math.round(seconds));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const base = `${h > 0 ? String(m).padStart(2, "0") : String(m)}:${String(s).padStart(2, "0")}`;
  return h > 0 ? `${String(h)}:${base}` : base;
}

/** `m:ss`, `h:mm:ss` or plain seconds; null for an empty field, NaN for anything else. */
export function parseClock(value: string): number | null {
  const trimmed = value.trim();
  if (trimmed === "") return null;
  const parts = trimmed.split(":");
  if (parts.length > 3) return NaN;
  let total = 0;
  for (const part of parts) {
    const n = Number(part);
    if (part.trim() === "" || !Number.isFinite(n) || n < 0) return NaN;
    total = total * 60 + n;
  }
  return total;
}

export type MarkerFields = Record<MarkerKind, { start: string; end: string }>;

export function markerFields(data: FileMarkers): MarkerFields {
  const field = (k: MarkerKind) => ({
    start: formatClock(data[k].start_seconds),
    end: formatClock(data[k].end_seconds),
  });
  return { intro: field("intro"), recap: field("recap"), credits: field("credits"), preview: field("preview") };
}

export type MarkerEdits =
  | { ok: true; set: Partial<Record<MarkerKind, MarkerRange>>; clear: MarkerKind[] }
  | { ok: false; kind: MarkerKind; problem: "needBoth" | "endBeforeStart" };

/**
 * What to save from the edited fields, as the web editor decides it: emptied fields clear a stored segment, untouched
 * ones (compared at whole seconds, the fields' precision) are left alone so they keep their source.
 */
export function markerEdits(fields: MarkerFields, data: FileMarkers): MarkerEdits {
  const set: Partial<Record<MarkerKind, MarkerRange>> = {};
  const clear: MarkerKind[] = [];
  for (const kind of MARKER_KINDS) {
    const { start: startText, end: endText } = fields[kind];
    const stored = data[kind];
    const had = stored.start_seconds != null || stored.end_seconds != null;
    if (startText.trim() === "" && endText.trim() === "") {
      if (had) clear.push(kind);
      continue;
    }
    const start = parseClock(startText);
    const end = parseClock(endText);
    if (start === null || end === null || Number.isNaN(start) || Number.isNaN(end)) {
      return { ok: false, kind, problem: "needBoth" };
    }
    if (end <= start) return { ok: false, kind, problem: "endBeforeStart" };
    const round = (v: number | null | undefined) => (v == null ? null : Math.round(v));
    if (round(stored.start_seconds) === start && round(stored.end_seconds) === end) continue;
    set[kind] = { start_seconds: start, end_seconds: end };
  }
  return { ok: true, set, clear };
}
