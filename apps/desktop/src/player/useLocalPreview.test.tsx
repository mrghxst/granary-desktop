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
  it("waits for the hover to settle, keeps up to three requests in flight, then fetches the newest spot", async () => {
    const resolvers: ((url: string) => void)[] = [];
    const asked: number[] = [];
    vi.spyOn(player, "previewWarm").mockResolvedValue(null);
    vi.spyOn(player, "previewNear").mockResolvedValue(null);
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
    // Rests while 40 still loads are fetched alongside it, up to three at once.
    rerender({ at: 100 });
    await settle();
    rerender({ at: 250 });
    await settle();
    expect(asked).toEqual([40, 100, 250]);
    rerender({ at: 300 });
    await settle();
    rerender({ at: 420 });
    await settle();
    expect(asked).toEqual([40, 100, 250]);
    // A slot frees: only the spot under the pointer is fetched, not the ones passed on the way.
    await act(async () => {
      resolvers[0]?.("data:a");
      await Promise.resolve();
    });
    expect(asked).toEqual([40, 100, 250, 420]);
    // The host had no stand-in either.
    expect(result.current).toBeNull();
    await act(async () => {
      resolvers[3]?.("data:b");
      await Promise.resolve();
    });
    expect(result.current).toEqual({ url: "data:b", approx: false });
    // A cached spot is answered without another request.
    rerender({ at: 49 });
    expect(result.current).toEqual({ url: "data:a", approx: false });
    expect(asked).toEqual([40, 100, 250, 420]);
  });

  it("starts the background fill once and shows the host's nearest frame until the exact one arrives", async () => {
    const warm = vi.spyOn(player, "previewWarm").mockResolvedValue(null);
    vi.spyOn(player, "previewNear").mockResolvedValue({ at: 120, url: "data:near" });
    let resolve: (url: string) => void = () => undefined;
    vi.spyOn(player, "preview").mockImplementation(() => new Promise<string | null>((r) => (resolve = r)));
    const { result, rerender } = renderHook(({ at }) => useLocalPreview(true, at, 4000), {
      initialProps: { at: 300 },
    });
    await act(async () => {
      await Promise.resolve();
    });
    expect(warm).toHaveBeenCalledTimes(1);
    expect(result.current).toEqual({ url: "data:near", approx: true });
    await settle();
    await act(async () => {
      resolve("data:exact");
      await Promise.resolve();
    });
    expect(result.current).toEqual({ url: "data:exact", approx: false });
    // Too far from any decoded frame (more than a twelfth of the file): no stand-in.
    rerender({ at: 3000 });
    expect(result.current).toBeNull();
    expect(warm).toHaveBeenCalledTimes(1);
  });

  it("does nothing when disabled or not hovering", async () => {
    const spy = vi.spyOn(player, "preview");
    const warm = vi.spyOn(player, "previewWarm").mockResolvedValue(null);
    const initialProps: { on: boolean; at: number | null } = { on: false, at: 5 };
    const { result, rerender } = renderHook(({ on, at }) => useLocalPreview(on, at, 600), { initialProps });
    expect(warm).not.toHaveBeenCalled();
    rerender({ on: true, at: null });
    await settle();
    expect(result.current).toBeNull();
    expect(spy).not.toHaveBeenCalled();
  });
});
