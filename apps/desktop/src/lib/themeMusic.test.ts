import { describe, expect, it, vi } from "vitest";
import { ThemeMusic } from "./themeMusic";

function fakeContext() {
  const sources: { loop: boolean; start: ReturnType<typeof vi.fn>; stop: ReturnType<typeof vi.fn> }[] = [];
  const gains: { linearRampToValueAtTime: ReturnType<typeof vi.fn> }[] = [];
  const context = {
    state: "running",
    currentTime: 10,
    destination: {},
    decodeAudioData: vi.fn(() => Promise.resolve({})),
    resume: vi.fn(() => Promise.resolve()),
    close: vi.fn(() => Promise.resolve()),
    createBufferSource: () => {
      const s = { buffer: null, loop: false, start: vi.fn(), stop: vi.fn(), connect: (n: unknown) => n };
      sources.push(s);
      return s;
    },
    createGain: () => {
      const g = {
        gain: {
          value: 0.35,
          setValueAtTime: vi.fn(),
          linearRampToValueAtTime: vi.fn(),
          cancelScheduledValues: vi.fn(),
        },
        connect: (n: unknown) => n,
      };
      gains.push(g.gain);
      return g;
    },
  };
  return { context: context as unknown as AudioContext, sources, gains, raw: context };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("ThemeMusic", () => {
  it("fades a tune in and keeps it going for the same title", async () => {
    const f = fakeContext();
    const load = vi.fn(() => Promise.resolve(new ArrayBuffer(8)));
    const music = new ThemeMusic(load, () => f.context);
    music.play("s1", "t1", false);
    await settle();
    expect(load).toHaveBeenCalledWith("s1", "t1");
    expect(f.sources).toHaveLength(1);
    expect(f.sources[0]?.start).toHaveBeenCalled();
    expect(f.gains[0]?.linearRampToValueAtTime).toHaveBeenCalledWith(0.35, 10.6);
    // The season page of the same series: no second load, the loop setting follows.
    music.play("s1", "t1", true);
    await settle();
    expect(load).toHaveBeenCalledTimes(1);
    expect(f.sources[0]?.loop).toBe(true);
  });

  it("fades out for another page and cuts when playback starts", async () => {
    const f = fakeContext();
    const music = new ThemeMusic(
      () => Promise.resolve(new ArrayBuffer(8)),
      () => f.context,
    );
    music.play("s1", "t1", false);
    await settle();
    music.stop();
    expect(f.sources[0]?.stop).toHaveBeenCalledWith(10.6);
    music.play("s2", "t2", false);
    await settle();
    music.stop(true);
    expect(f.sources[1]?.stop).toHaveBeenCalledWith();
  });

  it("drops a tune that arrives after the page was left, and survives one that does not decode", async () => {
    const f = fakeContext();
    let deliver: (b: ArrayBuffer) => void = () => undefined;
    const music = new ThemeMusic(
      () =>
        new Promise<ArrayBuffer>((r) => {
          deliver = r;
        }),
      () => f.context,
    );
    music.play("s1", "t1", false);
    music.stop();
    deliver(new ArrayBuffer(8));
    await settle();
    expect(f.sources).toHaveLength(0);

    f.raw.decodeAudioData.mockRejectedValueOnce(new Error("bad audio"));
    music.play("s3", "t3", false);
    deliver(new ArrayBuffer(8));
    await settle();
    expect(f.sources).toHaveLength(0);
  });
});
