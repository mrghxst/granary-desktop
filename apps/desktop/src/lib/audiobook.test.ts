import { describe, expect, it } from "vitest";
import { bookChapters, bookDuration, bookParts, chapterAt, partAt } from "./audiobook";
import type { FileVersion } from "./catalog";

const v = (id: string, d: number, chapters: { title: string; start_seconds: number }[] = []): FileVersion =>
  ({
    file_id: id,
    duration_seconds: d,
    chapters: chapters.map((c, i) => ({ index: i, end_seconds: 0, source: "", ...c })),
  }) as unknown as FileVersion;

const versions = [
  v("p1", 3192, [{ title: "Prologue", start_seconds: 0 }]),
  v("p2", 3586, [
    { title: "One", start_seconds: 0 },
    { title: "", start_seconds: 1800 },
  ]),
  v("p3", 4035),
];

describe("audiobook timeline", () => {
  it("lays the parts end to end", () => {
    const parts = bookParts(versions);
    expect(parts.map((p) => p.start)).toEqual([0, 3192, 6778]);
    expect(bookDuration(parts)).toBe(10813);
  });

  it("finds the part and the position inside it", () => {
    const parts = bookParts(versions);
    expect(partAt(parts, 0)).toEqual({ index: 0, local: 0 });
    expect(partAt(parts, 3200)).toEqual({ index: 1, local: 8 });
    expect(partAt(parts, 99999)).toEqual({ index: 2, local: 4034 });
    expect(partAt([], 5).index).toBe(-1);
  });

  it("lists chapters on the book timeline, naming untitled ones", () => {
    const parts = bookParts(versions);
    const chapters = bookChapters(versions, parts, (n) => `Chapter ${String(n)}`);
    expect(chapters.map((c) => [c.title, c.start, c.part])).toEqual([
      ["Prologue", 0, 0],
      ["One", 3192, 1],
      ["Chapter 3", 4992, 1],
      ["Chapter 4", 6778, 2],
    ]);
    expect(chapterAt(chapters, 5000)).toBe(2);
    expect(chapterAt(chapters, 0)).toBe(0);
  });
});
