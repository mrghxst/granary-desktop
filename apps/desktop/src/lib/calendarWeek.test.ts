import { describe, expect, it } from "vitest";
import { addDays, isDay, weekDays, weekRange, weekStart } from "./calendarWeek";

describe("calendarWeek", () => {
  it("starts the week on Monday", () => {
    expect(weekStart(new Date(2026, 10, 17))).toBe("2026-11-16"); // a Tuesday
    expect(weekStart(new Date(2026, 10, 16))).toBe("2026-11-16"); // the Monday itself
    expect(weekStart(new Date(2026, 10, 22, 23, 59))).toBe("2026-11-16"); // Sunday night
    expect(weekStart(new Date(2027, 0, 1))).toBe("2026-12-28"); // across the year
  });

  it("steps over month ends and clock changes", () => {
    expect(addDays("2026-11-30", 1)).toBe("2026-12-01");
    expect(addDays("2026-03-02", -7)).toBe("2026-02-23");
    // The last week of March has a 23-hour day in Europe.
    expect(weekDays("2026-03-23")).toEqual([
      "2026-03-23",
      "2026-03-24",
      "2026-03-25",
      "2026-03-26",
      "2026-03-27",
      "2026-03-28",
      "2026-03-29",
    ]);
    expect(addDays("2026-03-29", 1)).toBe("2026-03-30");
  });

  it("labels a week", () => {
    // The spaces around the dash differ between ICU versions (none, thin or plain).
    expect(weekRange("2026-11-16", "en")).toMatch(/^Nov 16\s?–\s?22, 2026$/);
    expect(weekRange("2026-11-16", "en-GB")).toMatch(/^16\s?–\s?22 Nov 2026$/);
    expect(weekRange("2026-11-30", "en-GB")).toMatch(/^30 Nov\s?–\s?6 Dec 2026$/);
    expect(weekRange("2026-12-28", "en-GB")).toMatch(/^28 Dec 2026\s?–\s?3 Jan 2027$/);
  });

  it("tells a day from anything else", () => {
    expect(isDay("2026-11-16")).toBe(true);
    expect(isDay("16.11.2026")).toBe(false);
    expect(isDay(null)).toBe(false);
  });
});
