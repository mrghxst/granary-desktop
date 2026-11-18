import { describe, expect, it } from "vitest";
import type { PlanInfo } from "./player";
import { explainPlan, formatKbps, playMethod, resolutionChoices } from "./quality";

function plan(over: Partial<PlanInfo>): PlanInfo {
  return {
    session_id: "s",
    plan_id: "p",
    delivery: "original_http",
    decision_reason: "direct_play",
    direct_play: true,
    relayed: true,
    protocol: "http",
    degradation_warnings: [],
    subtitles: [],
    attempt_id: "a",
    adoption: 1,
    cause: "start",
    quality_preference: "auto",
    available_qualities: [
      { label: "1080p-high", display_name: "1080p High", bitrate_kbps: 10_000, preserves_source: false },
    ],
    source: { video_codec: "hevc", width: 3840, height: 2160, bitrate_kbps: 77_367, dynamic_range: "dolby_vision" },
    delivered: { video_codec: "hevc", width: 3840, height: 2160, bitrate_kbps: 77_367, dynamic_range: "dolby_vision" },
    delivered_kbps: 77_367,
    transformations: [],
    warnings: [],
    buffering_feature: true,
    ...over,
  };
}

describe("explainPlan", () => {
  it("says the player handles Dolby Vision when the server leaves it to the client", () => {
    // Seen on a real server: direct play with reason client_managed_dynamic_range.
    const p = plan({ decision_reason: "client_managed_dynamic_range" });
    expect(playMethod(p)).toBe("direct_play");
    expect(explainPlan(p).reasons).toEqual([expect.objectContaining({ key: "quality.why.clientRange" })]);
    expect(explainPlan(p).changes).toEqual([]);
  });

  it("lists a picked transcode's changes and does not repeat the tone-mapping warning", () => {
    const p = plan({
      delivery: "server_transcode_hls",
      decision_reason: "quality_fixed_rung",
      direct_play: false,
      quality_preference: "1080p-high",
      active_quality: "1080p-high",
      delivered: { video_codec: "hevc", width: 1920, height: 1080, bitrate_kbps: 10_000, dynamic_range: "sdr" },
      delivered_kbps: 10_000,
      transformations: ["video_to_hevc", "audio_to_aac", "hdr_to_sdr_tonemap"],
      warnings: [{ code: "hdr_tone_mapped", message: "HDR video is tone-mapped to SDR for this playback route." }],
    });
    const e = explainPlan(p);
    expect(playMethod(p)).toBe("transcode");
    expect(e.reasons[0]?.key).toBe("quality.why.picked");
    expect(e.reasons[0]?.values?.quality).toBe("1080p High");
    expect(e.changes.map((c) => c.key)).toEqual([
      "quality.change.resolution",
      "quality.change.bitrate",
      "quality.change.videoHevc",
      "quality.change.audioAac",
      "quality.change.toneMap",
    ]);
    expect(e.warnings).toEqual([]);
  });

  it("shows server warnings it has no translation for as written", () => {
    const p = plan({ warnings: [{ code: "something_new", message: "A new server warning." }] });
    expect(explainPlan(p).warnings).toEqual(["A new server warning."]);
  });
});

describe("formatting", () => {
  it("rounds rates down as the design's Settings does", () => {
    expect(formatKbps(800)).toBe("800 kbps");
    expect(formatKbps(7_399)).toBe("7.3 Mbps");
    expect(formatKbps(6_000)).toBe("6 Mbps");
    expect(formatKbps(19_173)).toBe("19 Mbps");
    expect(formatKbps(null)).toBe("");
  });

  it("offers resolutions up to the file's own", () => {
    expect(resolutionChoices("4K")).toEqual(["2160p", "1080p", "720p", "480p"]);
    expect(resolutionChoices("1080p")).toEqual(["1080p", "720p", "480p"]);
  });
});
