import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { player } from "../lib/player";
import { useLocalPreview } from "./useLocalPreview";

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useLocalPreview", () => {
  it("keeps one request in flight and then fetches only the newest position", async () => {
    const resolvers: ((url: string) => void)[] = [];
    const asked: number[] = [];
    vi.spyOn(player, "preview").mockImplementation((s: number) => {
      asked.push(s);
      return new Promise<string | null>((r) => resolvers.push(r));
    });
    const { result, rerender } = renderHook(({ at }) => useLocalPreview(true, at), {
      initialProps: { at: 10 },
    });
    // Sweep over three more buckets while the first request is still open.
    rerender({ at: 30 });
    rerender({ at: 50 });
    rerender({ at: 71 });
    expect(asked).toEqual([10]);
    resolvers[0]?.("data:a");
    await waitFor(() => {
      expect(asked).toEqual([10, 70]);
    });
    // While 70 loads, nothing within five buckets is cached, so nothing is shown.
    expect(result.current).toBeNull();
    resolvers[1]?.("data:b");
    await waitFor(() => {
      expect(result.current).toBe("data:b");
    });
    // A cached bucket is answered without another request; a near miss shows the neighbour while it loads.
    rerender({ at: 10.5 });
    expect(result.current).toBe("data:a");
    rerender({ at: 14 });
    expect(result.current).toBe("data:a");
    expect(asked).toEqual([10, 70, 14]);
  });

  it("does nothing when disabled or not hovering", () => {
    const spy = vi.spyOn(player, "preview");
    const initialProps: { on: boolean; at: number | null } = { on: false, at: 5 };
    const { result, rerender } = renderHook(({ on, at }) => useLocalPreview(on, at), { initialProps });
    rerender({ on: true, at: null });
    expect(result.current).toBeNull();
    expect(spy).not.toHaveBeenCalled();
  });
});
