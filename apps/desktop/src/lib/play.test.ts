import type { FileVersion, WatchDetail } from "./catalog";
import {
  defaultAudio,
  defaultChoice,
  defaultSubtitle,
  formatTime,
  playRequest,
  protocolSubtitleIndex,
  resumePosition,
} from "./play";

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
  it("maps catalog subtitle indexes to the protocol ordinal: externals first, then embedded", () => {
    const v = version();
    expect(protocolSubtitleIndex(v, 5)).toBe(0);
    expect(protocolSubtitleIndex(v, 3)).toBe(1);
    expect(protocolSubtitleIndex(v, 4)).toBe(2);
    expect(protocolSubtitleIndex(v, null)).toBeNull();
    expect(protocolSubtitleIndex(v, 9)).toBeNull();
    const noExternal = version({ subtitle_tracks: v.subtitle_tracks.filter((t) => !t.external) });
    expect(protocolSubtitleIndex(noExternal, 4)).toBe(1);
  });

  it("forced_only picks only forced tracks, never another language", () => {
    const v = version();
    const w = (language?: string) =>
      watch({ effective_subtitle_mode: "forced_only", effective_subtitle_language: language ?? null });
    expect(defaultSubtitle(w("de"), v)).toBe(3);
    expect(defaultSubtitle(w("en"), v)).toBeNull();
    // No subtitle language: the audio's (default track, German).
    expect(defaultSubtitle(w(), v)).toBe(3);
    expect(defaultSubtitle(w(), v, 0)).toBeNull();
  });

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

  it("follows the web app's subtitle auto-selection", () => {
    const v = version();
    const w = (over: Partial<WatchDetail>) => watch(over);
    // auto: German subtitle language, English audio -> best full German track; German audio -> forced German only.
    expect(defaultSubtitle(w({ effective_subtitle_mode: "auto", effective_subtitle_language: "de" }), v, 0)).toBe(4);
    expect(defaultSubtitle(w({ effective_subtitle_mode: "auto", effective_subtitle_language: "de" }), v, 1)).toBe(3);
    // Unset mode is auto; with no subtitle language only the forced track for the audio language shows.
    expect(defaultSubtitle(w({}), v, 1)).toBe(3);
    expect(defaultSubtitle(w({}), v, 0)).toBeNull();
    expect(defaultSubtitle(w({ effective_show_forced_subtitles: false }), v, 1)).toBeNull();
    // Explicit empty language means no subtitles.
    expect(defaultSubtitle(w({ effective_subtitle_mode: "auto", effective_subtitle_language: "" }), v, 0)).toBeNull();
    // off still shows forced subtitles for the audio language, unless they are turned off.
    expect(defaultSubtitle(w({ effective_subtitle_mode: "off" }), v, 1)).toBe(3);
    expect(
      defaultSubtitle(w({ effective_subtitle_mode: "off", effective_show_forced_subtitles: false }), v, 1),
    ).toBeNull();
    // always: the language's best track, preferring full over forced, region variants match.
    expect(defaultSubtitle(w({ effective_subtitle_mode: "always", effective_subtitle_language: "de-CH" }), v)).toBe(4);
    expect(defaultSubtitle(w({ effective_subtitle_mode: "always", effective_subtitle_language: "fr" }), v)).toBeNull();
    // always: the track last chosen on this item wins (exact signature).
    expect(
      defaultSubtitle(
        w({
          effective_subtitle_mode: "always",
          effective_subtitle_language: "de",
          effective_subtitle_track_signature: {
            language: "de",
            forced: true,
            hearing_impaired: false,
            source: "embedded",
          },
        }),
        v,
      ),
    ).toBe(3);
    // Unknown modes select nothing (forced_only has its own test).
    expect(
      defaultSubtitle(w({ effective_subtitle_mode: "default", effective_subtitle_language: "en" }), v, 1),
    ).toBeNull();
    expect(defaultSubtitle(w({ effective_subtitle_mode: "always" }), version({ subtitle_tracks: [] }))).toBeNull();
  });

  it("matches ISO 639-2 and 639-1 language tags and ranks plain text above SDH and bitmap", () => {
    const v = version({
      audio_tracks: [{ language: "eng", default: true }],
      subtitle_tracks: [
        {
          index: 7,
          language: "ger",
          codec: "hdmv_pgs_subtitle",
          forced: false,
          hearing_impaired: false,
          default: false,
          external: false,
        },
        {
          index: 8,
          language: "deu",
          codec: "subrip",
          forced: false,
          hearing_impaired: true,
          default: false,
          external: false,
        },
        {
          index: 9,
          language: "deu",
          codec: "subrip",
          forced: false,
          hearing_impaired: false,
          default: false,
          external: true,
        },
      ],
    });
    const w = watch({ effective_subtitle_mode: "auto", effective_subtitle_language: "de" });
    expect(defaultSubtitle(w, v, 0)).toBe(9);
    // English audio tagged eng counts as the English subtitle language -> no full subtitles.
    expect(
      defaultSubtitle(watch({ effective_subtitle_mode: "auto", effective_subtitle_language: "en" }), v, 0),
    ).toBeNull();
  });

  it("resumes any nonzero position, including a rewatch in progress", () => {
    const u = { played: false, watched_count: 0, unplayed_count: 1, in_progress_count: 1 };
    expect(resumePosition(watch({ user_data: { ...u, position_seconds: 754.6 } }))).toBe(754);
    expect(resumePosition(watch({ user_data: { ...u, position_seconds: 10 } }))).toBe(10);
    expect(resumePosition(watch({ user_data: { ...u, played: true, position_seconds: 754 } }))).toBe(754);
    expect(resumePosition(watch({ user_data: { ...u, played: true, position_seconds: 0 } }))).toBe(0);
    expect(resumePosition(watch())).toBe(0);
    const w = watch({
      effective_subtitle_mode: "auto",
      effective_subtitle_language: "en",
      user_data: { ...u, position_seconds: 754 },
    });
    const choice = defaultChoice(w);
    if (!choice) throw new Error("no choice");
    expect(playRequest("s1", w, choice, false)).toEqual({
      serverId: "s1",
      itemId: "m1",
      fileId: "f1",
      startSeconds: 754,
      audioIndex: 1,
      // Catalog index 5 is the only external subtitle: protocol ordinal 0 (externals count first).
      subtitleIndex: 0,
    });
    expect(playRequest("s1", w, choice, true).startSeconds).toBe(0);
  });

  it("prefers the last played edition when the last file is gone", () => {
    const w = watch({
      versions: [
        version({ file_id: "a", edition_key: "theatrical" }),
        version({ file_id: "b", edition_key: "extended" }),
      ],
      effective_version_edition_key: "extended",
    });
    expect(defaultChoice(w)?.version.file_id).toBe("b");
    const plain = watch({ versions: [version({ file_id: "a", edition_key: "extended" }), version({ file_id: "c" })] });
    expect(defaultChoice(plain)?.version.file_id).toBe("c");
  });

  it("formats times", () => {
    expect(formatTime(5)).toBe("0:05");
    expect(formatTime(245)).toBe("4:05");
    expect(formatTime(3723)).toBe("1:02:03");
  });
});
