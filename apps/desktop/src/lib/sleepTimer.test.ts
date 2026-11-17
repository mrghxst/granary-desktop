import { describe, expect, it } from "vitest";
import { sleepCountdown, sleepTick, stillWatchingDue } from "./sleepTimer";

describe("sleep timer", () => {
  it("counts down and runs out once", () => {
    const first = sleepTick({ kind: "time", remainingMs: 1500 }, 1000);
    expect(first).toEqual({ next: { kind: "time", remainingMs: 500 }, expired: false });
    expect(sleepTick(first.next, 1000)).toEqual({ next: { kind: "off" }, expired: true });
    expect(sleepTick({ kind: "episode" }, 1000)).toEqual({ next: { kind: "episode" }, expired: false });
  });

  it("formats the time left", () => {
    expect(sleepCountdown(61_000)).toBe("1:01");
    expect(sleepCountdown(500)).toBe("0:01");
    expect(sleepCountdown(3_725_000)).toBe("1:02:05");
  });
});

describe("still watching", () => {
  it("asks after the limit of automatic starts, never with 0", () => {
    expect(stillWatchingDue(2, 3)).toBe(false);
    expect(stillWatchingDue(3, 3)).toBe(true);
    expect(stillWatchingDue(50, 0)).toBe(false);
  });
});
