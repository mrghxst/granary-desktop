import { describe, expect, it } from "vitest";
import type { FileMarkers } from "./catalog";
import { formatClock, markerEdits, markerFields, parseClock } from "./markers";

const stored: FileMarkers = {
  file_id: "5",
  intro: { start_seconds: 0, end_seconds: 12.4, source: "chapters" },
  credits: { start_seconds: 1290, end_seconds: 1380 },
  recap: {},
  preview: {},
  marker_segments: [],
};

describe("clock fields", () => {
  it("formats and parses times", () => {
    expect(formatClock(12.4)).toBe("0:12");
    expect(formatClock(3725)).toBe("1:02:05");
    expect(formatClock(undefined)).toBe("");
    expect(parseClock("1:02:05")).toBe(3725);
    expect(parseClock(" 90 ")).toBe(90);
    expect(parseClock("")).toBeNull();
    expect(parseClock("1::2")).toBeNaN();
    expect(parseClock("a")).toBeNaN();
  });
});

describe("markerEdits", () => {
  it("saves nothing for untouched fields", () => {
    expect(markerEdits(markerFields(stored), stored)).toEqual({ ok: true, set: {}, clear: [] });
  });

  it("sets edited segments and clears emptied ones", () => {
    const fields = markerFields(stored);
    fields.intro = { start: "0:05", end: "1:30" };
    fields.credits = { start: "", end: "" };
    fields.recap = { start: "0", end: "45" };
    expect(markerEdits(fields, stored)).toEqual({
      ok: true,
      set: { intro: { start_seconds: 5, end_seconds: 90 }, recap: { start_seconds: 0, end_seconds: 45 } },
      clear: ["credits"],
    });
  });

  it("refuses half-filled and backwards segments", () => {
    const fields = markerFields(stored);
    fields.preview = { start: "21:00", end: "" };
    expect(markerEdits(fields, stored)).toEqual({ ok: false, kind: "preview", problem: "needBoth" });
    fields.preview = { start: "21:00", end: "20:00" };
    expect(markerEdits(fields, stored)).toEqual({ ok: false, kind: "preview", problem: "endBeforeStart" });
  });
});
