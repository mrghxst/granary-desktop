import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { player, type PreviewReply, type PushedPreview } from "../lib/player";
import { previewInterval, useLocalPreview } from "./useLocalPreview";

afterEach(() => {
  vi.restoreAllMocks();
});

const flush = async () => {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
};

/** Mocks the host: `reply` answers each request; `push` sends a decoded frame. */
function host(reply: (s: number) => PreviewReply | null) {
  const asked: number[] = [];
  let listener: ((p: PushedPreview) => void) | null = null;
  const warm = vi.spyOn(player, "previewWarm").mockResolvedValue(null);
  vi.spyOn(player, "previewWant").mockImplementation((s: number) => {
    asked.push(s);
    return Promise.resolve(reply(s));
  });
  vi.spyOn(player, "onPreview").mockImplementation((f) => {
    listener = f;
    return Promise.resolve(() => {
      listener = null;
    });
  });
  const push = async (p: PushedPreview) => {
    await act(async () => {
      listener?.(p);
      await Promise.resolve();
    });
  };
  return { asked, warm, push };
}

const pending = (key: number, generation = 1): PreviewReply => ({ generation, key, url: null, near: null });

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
  it("asks for every new spot at once and shows frames as the host pushes them", async () => {
    // 4000 s: spots every 10 s; keyframes every 5 s.
    const { asked, warm, push } = host((s) => pending(Math.floor(s / 5) * 5));
    const { result, rerender } = renderHook(({ at }) => useLocalPreview(true, at, 4000), {
      initialProps: { at: 40 },
    });
    await flush();
    expect(warm).toHaveBeenCalledTimes(1);
    expect(asked).toEqual([40]);
    // A sweep asks for each spot it crosses, without waiting for the pointer to rest.
    rerender({ at: 101 });
    rerender({ at: 255 });
    await flush();
    expect(asked).toEqual([40, 100, 250]);
    expect(result.current).toBeNull();
    // The host pushes the frame for an earlier spot first: it stands in for the spot under the pointer.
    await push({ generation: 1, at: 100, url: "data:100" });
    expect(result.current).toEqual({ url: "data:100", approx: true });
    await push({ generation: 1, at: 250, url: "data:250" });
    expect(result.current).toEqual({ url: "data:250", approx: false });
    // A spot seen before is answered from the page's cache, without asking.
    rerender({ at: 109 });
    await flush();
    expect(result.current).toEqual({ url: "data:100", approx: false });
    expect(asked).toEqual([40, 100, 250]);
  });

  it("shows the host's cached or nearest frame from the answer", async () => {
    const { push } = host((s) =>
      s === 300
        ? { generation: 2, key: 300, url: "data:300", near: null }
        : { generation: 2, key: s, url: null, near: { at: 290, url: "data:290" } },
    );
    const { result, rerender } = renderHook(({ at }) => useLocalPreview(true, at, 4000), {
      initialProps: { at: 300 },
    });
    await flush();
    expect(result.current).toEqual({ url: "data:300", approx: false });
    rerender({ at: 310 });
    await flush();
    // The frame on screen (300) is closer than the host's nearest (290): it stays.
    expect(result.current).toEqual({ url: "data:300", approx: true });
    // Frames from an earlier file are ignored.
    await push({ generation: 1, at: 310, url: "data:old" });
    expect(result.current).toEqual({ url: "data:300", approx: true });
    // Too far from any frame: no stand-in.
    rerender({ at: 3000 });
    await flush();
    expect(result.current).toBeNull();
  });

  it("does nothing when disabled or not hovering", async () => {
    const { asked, warm } = host(() => null);
    const initialProps: { on: boolean; at: number | null } = { on: false, at: 5 };
    const { result, rerender } = renderHook(({ on, at }) => useLocalPreview(on, at, 600), { initialProps });
    await flush();
    expect(warm).not.toHaveBeenCalled();
    rerender({ on: true, at: null });
    await flush();
    expect(result.current).toBeNull();
    expect(asked).toEqual([]);
  });
});
