import type { Episode, WatchDetail } from "./catalog";
import {
  KEYMAP,
  actionFor,
  activeMarker,
  chapterAt,
  chapterTarget,
  clampSpeed,
  clampVolume,
  episodesAround,
  keyFromActivity,
  keyFromVk,
  keyName,
  mediaLines,
  neighbours,
  trickplayTile,
  upNextAnchorFor,
  upNextRemaining,
  upNextStart,
  type PlaybackContext,
  type Trickplay,
} from "./player";

const key = (k: string, mods: Partial<{ shiftKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  shiftKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe("keyboard map", () => {
  it("normalises DOM keys", () => {
    expect(keyName(key(" "))).toBe("Space");
    expect(keyName(key("ArrowLeft", { shiftKey: true }))).toBe("Shift+ArrowLeft");
    // Shift is part of the character for printable keys.
    expect(keyName(key("Z", { shiftKey: true }))).toBe("Z");
    expect(keyName(key("?", { shiftKey: true }))).toBe("?");
  });

  it("maps keys to actions like mpv", () => {
    expect(actionFor("Space")).toBe("toggle-pause");
    expect(actionFor("ArrowRight")).toBe("seek-forward");
    expect(actionFor("Shift+ArrowLeft")).toBe("seek-back-small");
    expect(actionFor("f")).toBe("fullscreen");
    expect(actionFor("m")).toBe("mute");
    expect(actionFor("i")).toBe("stats");
    expect(actionFor("j")).toBe("sub-cycle");
    expect(actionFor("Ctrl+q")).toBeUndefined();
  });

  it("binds every key once", () => {
    const all = KEYMAP.flatMap((b) => b.keys);
    expect(new Set(all).size).toBe(all.length);
  });

  it("maps virtual-key codes from the video window", () => {
    expect(keyFromVk(0x20, false)).toBe("Space");
    expect(keyFromVk(0x25, true)).toBe("Shift+ArrowLeft");
    expect(keyFromVk(0x46, false)).toBe("f");
    expect(keyFromVk(0x54, true)).toBe("T");
    expect(keyFromVk(0x33, true)).toBe("#");
    expect(keyFromVk(0xdb, false)).toBe("[");
    expect(keyFromVk(0xbf, true)).toBe("?");
    expect(keyFromVk(0x07, false)).toBeUndefined();
    expect(actionFor(keyFromVk(0x4d, false) ?? "")).toBe("mute");
  });

  it("uses the layout character from the video window first (German keyboard)", () => {
    // QWERTZ: the # key is VK_OEM_2 (0xBF), which the US map reads as "/" (volume down).
    expect(actionFor(keyFromActivity({ vk: 0xbf, ch: "#", shift: false }) ?? "")).toBe("audio-cycle");
    // Shift+ß gives "?" on QWERTZ.
    expect(actionFor(keyFromActivity({ vk: 0xdb, ch: "?", shift: true }) ?? "")).toBe("help");
    // Shift+3 gives "§", which is unbound, not "#".
    expect(keyFromActivity({ vk: 0x33, ch: "§", shift: true })).toBe("§");
    expect(keyFromActivity({ vk: 0x20, ch: " ", shift: false })).toBe("Space");
    // Named keys have no character and use the virtual-key map.
    expect(keyFromActivity({ vk: 0x25, ch: null, shift: true })).toBe("Shift+ArrowLeft");
    expect(keyFromActivity({ vk: 0x0d, shift: false })).toBe("Enter");
  });
});

const chapters = [
  { index: 0, title: "Opening", start_seconds: 0, end_seconds: 3, source: "embedded" },
  { index: 1, title: "Middle", start_seconds: 3, end_seconds: 6, source: "embedded" },
  { index: 2, title: "Finale", start_seconds: 6, end_seconds: 10, source: "embedded" },
];

describe("chapters and markers", () => {
  it("finds the chapter at a position", () => {
    expect(chapterAt(chapters, 0)?.title).toBe("Opening");
    expect(chapterAt(chapters, 4.2)?.title).toBe("Middle");
    expect(chapterAt(chapters, 9.9)?.title).toBe("Finale");
  });

  it("jumps to the next chapter and restarts the current one within 3 s", () => {
    expect(chapterTarget(chapters, 1, 1)).toBe(3);
    expect(chapterTarget(chapters, 9, 1)).toBeUndefined();
    expect(chapterTarget(chapters, 7.5, -1)).toBe(3);
    expect(chapterTarget(chapters, 9.5, -1)).toBe(6);
  });

  it("reports the active marker", () => {
    const markers = [
      { kind: "intro", start_seconds: 0.5, end_seconds: 3 },
      { kind: "credits", start_seconds: 8, end_seconds: 10 },
    ];
    expect(activeMarker(markers, 0.2)).toBeUndefined();
    expect(activeMarker(markers, 1)?.kind).toBe("intro");
    expect(activeMarker(markers, 2.9)).toBeUndefined();
    expect(activeMarker(markers, 9)?.kind).toBe("credits");
  });

  it("starts up-next at the credits, never later than end − 12 s", () => {
    expect(upNextStart([{ kind: "credits", start_seconds: 1200, end_seconds: 1300 }], 1320)).toBe(1200);
    expect(upNextStart([{ kind: "credits", start_seconds: 1315, end_seconds: 1320 }], 1320)).toBe(1308);
    expect(upNextStart([], 1320)).toBe(1300);
  });

  it("anchors the up-next countdown where it is first seen, so a resume inside the window gets the full countdown", () => {
    // Resumed at 1310 with the window from 1300: counting starts at 1310, not already 10 s in.
    const a = upNextAnchorFor(null, true, 1310, 1300);
    expect(a).toBe(1310);
    expect(upNextRemaining(a, 1310)).toBe(10);
    expect(upNextRemaining(a, 1315.5)).toBe(5);
    expect(upNextRemaining(a, 1320)).toBe(0);
    // Not eligible (paused, loading, cancelled): no anchor yet; a later position keeps the first anchor.
    expect(upNextAnchorFor(null, false, 1310, 1300)).toBeNull();
    expect(upNextAnchorFor(1310, true, 1312, 1300)).toBe(1310);
    // A seek back before the window clears it; never more than the full countdown.
    expect(upNextAnchorFor(1310, true, 1200, 1300)).toBeNull();
    expect(upNextRemaining(1310, 1305)).toBe(10);
    expect(upNextRemaining(null, 1305)).toBe(10);
  });
});

describe("trickplay tiles", () => {
  const tp: Trickplay = {
    file_id: "f",
    interval_ms: 10_000,
    thumbnail_width: 320,
    thumbnail_height: 180,
    tile_columns: 10,
    tile_rows: 10,
    thumbnail_count: 150,
    expires_at: "2026-11-08T00:00:00Z",
    sheets: [
      { index: 0, url: "/s0.jpg" },
      { index: 1, url: "/s1.jpg" },
    ],
  };

  it("locates the tile for a position", () => {
    expect(trickplayTile(tp, 0)).toEqual({ url: "/s0.jpg", x: 0, y: 0, width: 320, height: 180 });
    expect(trickplayTile(tp, 125)).toEqual({ url: "/s0.jpg", x: 640, y: 180, width: 320, height: 180 });
    expect(trickplayTile(tp, 1005)).toEqual({ url: "/s1.jpg", x: 0, y: 0, width: 320, height: 180 });
  });

  it("clamps past the last thumbnail and rejects bad metadata", () => {
    expect(trickplayTile(tp, 99_999)?.url).toBe("/s1.jpg");
    expect(trickplayTile({ ...tp, interval_ms: 0 }, 5)).toBeUndefined();
    expect(trickplayTile({ ...tp, sheets: [] }, 5)).toBeUndefined();
  });
});

const ep = (n: number): Episode => ({
  content_id: `e${String(n)}`,
  title: `Episode ${String(n)}`,
  season_number: 1,
  episode_number: n,
  runtime: 20,
});

const watch = (over: Partial<WatchDetail> = {}): WatchDetail => ({
  content_id: "e2",
  title: "Episode 2",
  type: "episode",
  versions: [],
  ...over,
});

describe("episodes", () => {
  it("finds previous and next in the season", () => {
    const ctx = { watch: watch(), episodes: [ep(1), ep(2), ep(3)] } as unknown as PlaybackContext;
    expect(neighbours(ctx).previous?.content_id).toBe("e1");
    expect(neighbours(ctx).next?.content_id).toBe("e3");
    expect(neighbours({ ...ctx, episodes: [ep(2)] })).toEqual({ previous: undefined, next: undefined });
    expect(neighbours(null)).toEqual({});
  });

  it("builds media flyout lines", () => {
    expect(mediaLines(watch({ series_title: "Show", season_number: 1, episode_number: 2 }))).toEqual({
      title: "Show",
      subtitle: "S1 E2 · Episode 2",
    });
    expect(mediaLines(watch({ title: "A Movie", type: "movie" }))).toEqual({ title: "A Movie" });
  });
});

describe("clamps", () => {
  it("keeps volume and speed in the host's accepted ranges", () => {
    expect(clampVolume(170)).toBe(150);
    expect(clampVolume(-3)).toBe(0);
    expect(clampSpeed(9)).toBe(4);
    expect(clampSpeed(0.1)).toBe(0.25);
  });
});

describe("episodesAround", () => {
  const ep = (season: number, n: number) =>
    ({ content_id: `s${String(season)}e${String(n)}`, title: "", season_number: season, episode_number: n }) as Episode;
  const data: Record<number, Episode[]> = {
    0: [ep(0, 1)],
    1: [ep(1, 1), ep(1, 2)],
    2: [ep(2, 1), ep(2, 2), ep(2, 3)],
    3: [ep(3, 1)],
  };
  const seasons = () => Promise.resolve([1, 2, 3]);
  const episodesOf = (s: number) => Promise.resolve(data[s] ?? []);
  const ids = (xs: Episode[]) => xs.map((e) => e.content_id);

  it("adds the next season's first episode after the last one", async () => {
    expect(ids(await episodesAround("s1e2", 1, seasons, episodesOf))).toEqual(["s1e1", "s1e2", "s2e1"]);
  });

  it("adds the previous season's last episode before the first one", async () => {
    expect(ids(await episodesAround("s2e1", 2, seasons, episodesOf))).toEqual(["s1e2", "s2e1", "s2e2", "s2e3"]);
  });

  it("does not fetch seasons in the middle of a season", async () => {
    let asked = false;
    const spy = () => {
      asked = true;
      return seasons();
    };
    expect(ids(await episodesAround("s2e2", 2, spy, episodesOf))).toEqual(["s2e1", "s2e2", "s2e3"]);
    expect(asked).toBe(false);
  });

  it("never crosses out of specials or past the last season", async () => {
    expect(ids(await episodesAround("s0e1", 0, seasons, episodesOf))).toEqual(["s0e1"]);
    expect(ids(await episodesAround("s3e1", 3, seasons, episodesOf))).toEqual(["s2e3", "s3e1"]);
  });

  it("keeps the season when the season list fails", async () => {
    expect(ids(await episodesAround("s1e2", 1, () => Promise.reject(new Error("x")), episodesOf))).toEqual([
      "s1e1",
      "s1e2",
    ]);
  });
});
