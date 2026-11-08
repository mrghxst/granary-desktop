import { afterEach, describe, expect, it } from "vitest";
import type { FileVersion, WatchDetail } from "./catalog";
import { applyTrackPicks, rememberTracks } from "./trackMemory";

const sub = (index: number, language: string, forced = false) => ({
  index,
  language,
  forced,
  hearing_impaired: false,
  default: false,
  external: false,
});

const episode1: FileVersion = {
  file_id: "e1",
  container: "mkv",
  codec_video: "hevc",
  codec_audio: "aac",
  resolution: "1080p",
  hdr: false,
  duration_seconds: 1440,
  file_size: 1,
  audio_tracks: [
    { language: "ja", codec: "flac", channels: 2, default: true },
    { language: "en", codec: "aac", channels: 2, default: false },
  ],
  subtitle_tracks: [sub(2, "en", true), sub(3, "en"), sub(4, "de")],
};

// The next episode orders its tracks differently.
const episode2: FileVersion = {
  ...episode1,
  file_id: "e2",
  audio_tracks: [
    { language: "en", codec: "aac", channels: 2, default: true },
    { language: "ja", codec: "flac", channels: 2, default: false },
  ],
  subtitle_tracks: [sub(5, "de"), sub(6, "en"), sub(7, "en", true)],
};

const watch = (id: string, v: FileVersion): WatchDetail => ({
  content_id: id,
  title: id,
  type: "episode",
  series_id: "s1",
  versions: [v],
});

afterEach(() => {
  window.localStorage.clear();
});

describe("track memory", () => {
  it("carries audio and subtitle picks to the next episode by description, not position", () => {
    rememberTracks("srv", watch("ep1", episode1), episode1, { audioIndex: 0, subtitleIndex: 4 });
    const next = applyTrackPicks("srv", watch("ep2", episode2), {
      version: episode2,
      audioIndex: 0,
      subtitleIndex: null,
    });
    expect(next).toEqual({ version: episode2, audioIndex: 1, subtitleIndex: 5 });
  });

  it("keeps subtitles off, but not a forced track's absence, and ignores films", () => {
    rememberTracks("srv", watch("ep1", episode1), episode1, { subtitleIndex: null });
    expect(
      applyTrackPicks("srv", watch("ep2", episode2), { version: episode2, audioIndex: 0, subtitleIndex: 6 }),
    ).toMatchObject({ subtitleIndex: null });
    expect(
      applyTrackPicks("srv", watch("ep2", episode2), { version: episode2, audioIndex: 0, subtitleIndex: 7 }),
    ).toMatchObject({ subtitleIndex: 7 });
    const film = { ...watch("m1", episode2), series_id: null };
    expect(applyTrackPicks("srv", film, { version: episode2, audioIndex: 0, subtitleIndex: 6 })).toMatchObject({
      subtitleIndex: 6,
    });
  });

  it("falls back to the defaults when the language is missing", () => {
    rememberTracks("srv", watch("ep1", episode1), episode1, { audioIndex: 0 });
    const dubOnly = { ...episode2, audio_tracks: [{ language: "en", codec: "aac", channels: 2, default: true }] };
    expect(
      applyTrackPicks("srv", watch("ep2", dubOnly), { version: dubOnly, audioIndex: 0, subtitleIndex: 6 }),
    ).toEqual({ version: dubOnly, audioIndex: 0, subtitleIndex: 6 });
  });
});
