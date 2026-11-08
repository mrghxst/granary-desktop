import { describe, expect, it, vi } from "vitest";
import type { Trickplay } from "./player";
import { DEFAULT_SETTINGS } from "./settings";
import { enabledSources, resolvePreviewProvider, seekrTile, type SeekrTrack } from "./previews";

const track: SeekrTrack = {
  scale: 1,
  cues: [0, 10, 20].map((start, i) => ({
    start,
    end: start + 10,
    url: "https://sprites.seekr.tv/s.jpg",
    x: i * 320,
    y: 0,
    w: 320,
    h: 180,
  })),
};

const trickplay = { thumbnail_count: 10 } as Trickplay;

describe("seek preview sources", () => {
  it("finds the Seekr sprite for a position, honouring scale", () => {
    expect(seekrTile(track, 0)?.x).toBe(0);
    expect(seekrTile(track, 15)?.x).toBe(320);
    expect(seekrTile(track, 999)?.x).toBe(640);
    // A file running 2 % longer than Seekr's source: client 20.4 s is source 20 s.
    expect(seekrTile({ ...track, scale: 1.02 }, 20.39)?.x).toBe(320);
    expect(seekrTile({ ...track, scale: 1.02 }, 20.41)?.x).toBe(640);
    expect(seekrTile({ scale: 1, cues: [] }, 5)).toBeUndefined();
  });

  it("uses server previews first, then Seekr, then local decoding by default", async () => {
    const order = enabledSources(DEFAULT_SETTINGS.player);
    expect(order).toEqual(["server", "seekr", "local"]);
    const seekr = vi.fn().mockResolvedValue(track);
    expect(await resolvePreviewProvider(order, { server: () => Promise.resolve(trickplay), seekr })).toEqual({
      kind: "server",
      trickplay,
    });
    expect(seekr).not.toHaveBeenCalled();
    // No server sheets: Seekr.
    expect((await resolvePreviewProvider(order, { server: () => Promise.resolve(null), seekr })).kind).toBe("seekr");
    // Seekr has nothing / limit reached / failed: local.
    const none = { server: () => Promise.resolve(null), seekr: () => Promise.resolve(null) };
    expect((await resolvePreviewProvider(order, none)).kind).toBe("local");
    const failing = { server: () => Promise.reject(new Error("x")), seekr: () => Promise.reject(new Error("y")) };
    expect((await resolvePreviewProvider(order, failing)).kind).toBe("local");
  });

  it("follows the user's order and switches", async () => {
    const player = {
      ...DEFAULT_SETTINGS.player,
      previewSources: [
        { id: "seekr" as const, enabled: true },
        { id: "local" as const, enabled: false },
        { id: "server" as const, enabled: true },
      ],
    };
    expect(enabledSources(player)).toEqual(["seekr", "server"]);
    expect(enabledSources({ ...player, seekPreviews: false })).toEqual([]);
    const deps = { server: () => Promise.resolve(null), seekr: () => Promise.resolve(null) };
    expect((await resolvePreviewProvider(["seekr", "server"], deps)).kind).toBe("none");
  });
});
