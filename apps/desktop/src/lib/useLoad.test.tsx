import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { retryLoads } from "./events";
import { useLoad } from "./useLoad";

describe("useLoad and the retry tick", () => {
  it("replaces a load error with data when the network comes back", async () => {
    let fail = true;
    const { result } = renderHook(() =>
      useLoad("k", () => (fail ? Promise.reject(new Error("offline")) : Promise.resolve("rows"))),
    );
    await waitFor(() => {
      expect(result.current.error).toBeInstanceOf(Error);
    });
    fail = false;
    act(() => {
      window.dispatchEvent(new Event("online"));
    });
    await waitFor(() => {
      expect(result.current.data).toBe("rows");
    });
    expect(result.current.error).toBeUndefined();
  });

  it("keeps shown data when a retry fails", async () => {
    let fail = false;
    const { result } = renderHook(() =>
      useLoad("k2", () => (fail ? Promise.reject(new Error("offline")) : Promise.resolve("rows"))),
    );
    await waitFor(() => {
      expect(result.current.data).toBe("rows");
    });
    fail = true;
    act(() => {
      retryLoads();
    });
    // Let the failed reload settle; the earlier rows stay.
    await act(async () => {
      await Promise.resolve();
    });
    expect(result.current.data).toBe("rows");
    expect(result.current.error).toBeUndefined();
  });
});
