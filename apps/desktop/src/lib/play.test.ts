import type { FileVersion, WatchDetail } from "./catalog";
import { defaultAudio, defaultChoice, defaultSubtitle, formatTime, playRequest, resumePosition } from "./play";

const version = (over: Partial<FileVersion> = {}): FileVersion => ({
  file_id: "f1",
  container: "mkv",
  codec_video: "hevc",
  codec_audio: "eac3",
  resolution: "2160p",
  hdr: true,
  duration_seconds: 7200,
  file_size: 1,
  audio_tracks: [
    { language: "en", default: false },
    { language: "de", default: true },
  ],
  subtitle_tracks: [
    { index: 3, language: "de", forced: true, hearing_impaired: false, default: false, external: false },
    { index: 4, language: "de", forced: false, hearing_impaired: false, default: false, external: false },
    { index: 5, language: "en", forced: false, hearing_impaired: false, default: true, external: true },
  ],
  ...over,
});

const watch = (over: Partial<WatchDetail> = {}): WatchDetail => ({
  content_id: "m1",
  title: "Movie",
  type: "movie",
  versions: [version(), version({ file_id: "f2" })],
  ...over,
});

describe("play hand-off defaults", () => {
  it("prefers the last watched version", () => {
    const w = watch({
      user_data: {
        played: false,
        last_file_id: "f2",
        watched_count: 0,
        unplayed_count: 1,
        in_progress_count: 1,
      },
    });
    expect(defaultChoice(w)?.version.file_id).toBe("f2");
    expect(defaultChoice(watch())?.version.file_id).toBe("f1");
    expect(defaultChoice(watch({ versions: [] }))).toBeUndefined();
  });

  it("picks audio from the server preference, then the default flag", () => {
    expect(defaultAudio(version({ effective_audio_track_index: 0 }))).toBe(0);
    expect(defaultAudio(version({ effective_audio_track_index: 9 }))).toBe(1);
    expect(defaultAudio(version())).toBe(1);
    expect(defaultAudio(version({ audio_tracks: [] }))).toBeNull();
  });

  it("follows the profile subtitle mode and language", () => {
    const v = version();
    const w = (mode: string, language = "de") =>
      watch({ effective_subtitle_mode: mode, effective_subtitle_language: language });
    expect(defaultSubtitle(w("off"), v)).toBeNull();
    expect(defaultSubtitle(w(""), v)).toBe(5); // unset → the file's default track
    expect(defaultSubtitle(w("default"), v)).toBe(5);
    expect(defaultSubtitle(w("always", "de-CH"), v)).toBe(4);
    expect(defaultSubtitle(w("forced_only"), v)).toBe(3);
    expect(defaultSubtitle(w("forced_only", "en"), v)).toBeNull();
    expect(defaultSubtitle(w("auto"), v, 1)).toBe(3); // German audio → only forced German subtitles
    expect(defaultSubtitle(w("auto"), v, 0)).toBe(4); // English audio → full German subtitles
    expect(defaultSubtitle(w("always", "fr"), v)).toBeNull();
    expect(defaultSubtitle(w("always"), version({ subtitle_tracks: [] }))).toBeNull();
  });

  it("resumes only meaningful, unfinished positions", () => {
    const u = { played: false, watched_count: 0, unplayed_count: 1, in_progress_count: 1 };
    expect(resumePosition(watch({ user_data: { ...u, position_seconds: 754.6 } }))).toBe(754);
    expect(resumePosition(watch({ user_data: { ...u, position_seconds: 10 } }))).toBe(0);
    expect(resumePosition(watch({ user_data: { ...u, played: true, position_seconds: 754 } }))).toBe(0);
    const w = watch({ user_data: { ...u, position_seconds: 754 } });
    const choice = defaultChoice(w);
    if (!choice) throw new Error("no choice");
    expect(playRequest("s1", w, choice, false)).toEqual({
      serverId: "s1",
      itemId: "m1",
      fileId: "f1",
      startSeconds: 754,
      audioIndex: 1,
      subtitleIndex: 5,
    });
    expect(playRequest("s1", w, choice, true).startSeconds).toBe(0);
  });

  it("formats times", () => {
    expect(formatTime(5)).toBe("0:05");
    expect(formatTime(245)).toBe("4:05");
    expect(formatTime(3723)).toBe("1:02:03");
  });
});
