import type { FileVersion } from "./catalog";

/**
 * An audiobook's timeline across its files (`silo-server/web/src/pages/audiobooks/player/useAudiobookPlayback.ts@749db96a`):
 * the item's versions are its parts in order, each with its own chapters; the book position runs across them.
 */
export interface BookPart {
  fileId: string;
  /** Book seconds where the part starts and ends. */
  start: number;
  end: number;
}

export interface BookChapter {
  title: string;
  /** Book seconds. */
  start: number;
  part: number;
}

const finite = (v: number) => (Number.isFinite(v) && v > 0 ? v : 0);

export function bookParts(versions: readonly FileVersion[]): BookPart[] {
  let at = 0;
  return versions.map((v) => {
    const d = finite(v.duration_seconds);
    const part = { fileId: v.file_id, start: at, end: at + d };
    at += d;
    return part;
  });
}

export function bookDuration(parts: readonly BookPart[]): number {
  return parts.length ? (parts[parts.length - 1]?.end ?? 0) : 0;
}

/** The part playing book second `at`, and the position inside it. */
export function partAt(parts: readonly BookPart[], at: number): { index: number; local: number } {
  if (parts.length === 0) return { index: -1, local: 0 };
  const t = Math.max(0, finite(at));
  let index = parts.findIndex((p) => t >= p.start && t < Math.max(p.end, p.start + 1));
  if (index < 0) index = t >= (parts[parts.length - 1]?.end ?? 0) ? parts.length - 1 : 0;
  const part = parts[index];
  return { index, local: part ? Math.max(0, Math.min(t - part.start, Math.max(0, part.end - part.start - 1))) : 0 };
}

/** Every chapter on the book timeline; a part without chapters counts as one, called `fallback(n)`. */
export function bookChapters(
  versions: readonly FileVersion[],
  parts: readonly BookPart[],
  fallback: (n: number) => string,
): BookChapter[] {
  const out: BookChapter[] = [];
  versions.forEach((v, i) => {
    const base = parts[i]?.start ?? 0;
    const list = v.chapters ?? [];
    if (list.length === 0) {
      out.push({ title: fallback(out.length + 1), start: base, part: i });
      return;
    }
    for (const c of list) {
      out.push({ title: c.title.trim() || fallback(out.length + 1), start: base + finite(c.start_seconds), part: i });
    }
  });
  return out;
}

/** The chapter playing at book second `at` (the last one that started). */
export function chapterAt(chapters: readonly BookChapter[], at: number): number {
  let index = -1;
  chapters.forEach((c, i) => {
    if (c.start <= at + 0.25) index = i;
  });
  return index;
}

/** Speeds the audiobook player offers. */
export const BOOK_SPEEDS = [0.75, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];
