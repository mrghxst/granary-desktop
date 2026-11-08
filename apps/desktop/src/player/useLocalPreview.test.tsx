import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { player } from "../lib/player";
import { previewInterval, useLocalPreview } from "./useLocalPreview";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const settle = async () => {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(200);
  });
};

describe("previewInterval", () => {
  it("keeps about 400 spots between 2 s and 10 s", () => {
    expect(previewInterval(0)).toBe(10);
    expect(previewInterval(Number.NaN)).toBe(10);
    expect(previewInterval(300)).toBe(2);
    expect(previewInterval(24 * 60)).toBe(4);
    expect(previewInterval(45 * 60)).toBe(7);
    expect(previewInterval(3 * 3600)).toBe(10);
  });
});

describe("useLocalPreview", () => {
  it("waits for the hover to settle, keeps one request in flight, then fetches only the newest spot", async () => {
    const resolvers: ((url: string) => void)[] = [];
    const asked: number[] = [];
    vi.spyOn(player, "preview").mockImplementation((s: number) => {
      asked.push(s);
      return new Promise<string | null>((r) => resolvers.push(r));
    });
    const { result, rerender } = renderHook(({ at }) => useLocalPreview(true, at, 4000), {
      initialProps: { at: 10 },
    });
    // A quick sweep asks nothing until the pointer rests.
    rerender({ at: 40 });
    expect(asked).toEqual([]);
    await settle();
    expect(asked).toEqual([40]);
    // Move on while 40 is still loading: only the newest spot is fetched afterwards.
    rerender({ at: 100 });
    await settle();
    rerender({ at: 250 });
    await settle();
    expect(asked).toEqual([40]);
    await act(async () => {
      resolvers[0]?.("data:a");
      await Promise.resolve();
    });
    expect(asked).toEqual([40, 250]);
    // Nothing within two intervals of 250 is cached yet.
    expect(result.current).toBeNull();
    await act(async () => {
      resolvers[1]?.("data:b");
      await Promise.resolve();
    });
    expect(result.current).toBe("data:b");
    // A cached spot is answered without another request; a near miss shows the neighbour while it loads.
    rerender({ at: 49 });
    expect(result.current).toBe("data:a");
    rerender({ at: 65 });
    expect(result.current).toBe("data:a");
    await settle();
    expect(asked).toEqual([40, 250, 60]);
  });

  it("does nothing when disabled or not hovering", async () => {
    const spy = vi.spyOn(player, "preview");
    const initialProps: { on: boolean; at: number | null } = { on: false, at: 5 };
    const { result, rerender } = renderHook(({ on, at }) => useLocalPreview(on, at, 600), { initialProps });
    rerender({ on: true, at: null });
    await settle();
    expect(result.current).toBeNull();
    expect(spy).not.toHaveBeenCalled();
  });
});
