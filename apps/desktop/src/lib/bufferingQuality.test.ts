import { describe, expect, it } from "vitest";
import {
  BufferingController,
  lowerRung,
  stepDownEstimate,
  type BufferingEvent,
  type BufferingMode,
  type BufferingOutput,
} from "./bufferingQuality";
// The design's conformance vectors, copied unchanged from
// silo-server/docs/design/schemas/buffering-quality/v1/vectors.json@749db96a.
import vectors from "./fixtures/buffering-quality-v1.json";

interface VectorCase {
  name: string;
  mode?: BufferingMode;
  until: number;
  events: ({ t: number } & Record<string, unknown>)[];
  expect: ({ t: number } & Record<string, unknown>)[];
  buffering_action?: Record<string, string>;
}

function replay(c: VectorCase) {
  let clock = 0;
  const out: ({ t: number } & BufferingOutput)[] = [];
  const ctl = new BufferingController({
    mode: c.mode ?? (vectors.defaults.mode as BufferingMode),
    now: () => clock,
    onOutput: (o) => out.push({ t: clock, ...o }),
  });
  const runTo = (t: number) => {
    for (;;) {
      const due = ctl.nextDeadline();
      if (due === null || due > t) break;
      clock = Math.max(clock, due);
      ctl.advance();
    }
    clock = t;
  };
  for (const { t, ...event } of c.events) {
    runTo(t);
    ctl.handle(event as unknown as BufferingEvent);
  }
  runTo(c.until);
  return { out, ctl };
}

describe("buffering-quality conformance vectors", () => {
  it("covers every case", () => {
    expect(vectors.version).toBe(1);
    expect(vectors.cases.length).toBe(66);
  });
  for (const c of vectors.cases as VectorCase[]) {
    it(c.name, () => {
      const { out, ctl } = replay(c);
      expect(out).toEqual(c.expect);
      for (const [plan, action] of Object.entries(c.buffering_action ?? {})) {
        expect(ctl.bufferingAction(plan), `${plan} buffering_action`).toBe(action);
      }
    });
  }
});

describe("buffering-quality arithmetic", () => {
  it("steps down in exact integer arithmetic", () => {
    expect(stepDownEstimate(22_120)).toBe(15_484);
    expect(stepDownEstimate(63_000)).toBe(44_100);
    expect(stepDownEstimate(142)).toBe(99);
  });

  it("lowers from the lower of the named rung and what plays", () => {
    const menu = [
      { label: "1080p-high", bitrate_kbps: 10_000 },
      { label: "1080p-medium", bitrate_kbps: 6_000 },
      { label: "720p-high", bitrate_kbps: 4_000 },
    ];
    expect(lowerRung({ plan_id: "p", delivered_kbps: 10_000, quality: "x", available_qualities: menu })?.label).toBe(
      "1080p-medium",
    );
    expect(
      lowerRung({
        plan_id: "p",
        delivered_kbps: 5_000,
        quality: "1080p-high",
        active_quality: "1080p-high",
        available_qualities: menu,
      })?.label,
    ).toBe("720p-high");
    expect(lowerRung({ plan_id: "p", delivered_kbps: 4_000, quality: "x", available_qualities: menu })).toBeNull();
  });

  it("sends the measured throughput with a lowering and needs evidence before the first frame", () => {
    let clock = 0;
    const out: BufferingOutput[] = [];
    let slow = false;
    const ctl = new BufferingController({
      mode: "always",
      now: () => clock,
      onOutput: (o) => out.push(o),
      startEvidence: () => slow,
      throughput: () => 21_000.6,
    });
    ctl.handle({
      type: "plan_adopted",
      plan: {
        plan_id: "p1",
        delivered_kbps: 63_000,
        quality: "auto",
        available_qualities: [{ label: "1080p-high", bitrate_kbps: 10_000 }],
      },
    });
    ctl.handle({ type: "waiting" });
    clock = 20_000;
    ctl.advance();
    expect(out).toEqual([]);
    slow = true;
    ctl.handle({ type: "transport_error_start" });
    ctl.handle({ type: "transport_error_end" });
    clock = 26_000;
    ctl.advance();
    expect(out).toEqual([
      {
        type: "replan",
        body: {
          operation: "quality_change",
          quality_preference: "auto",
          bandwidth_estimate_kbps: 44_100,
          bandwidth_estimate_source: "observed_rebuffer",
          observed_throughput_kbps: 21_000,
        },
      },
    ]);
    expect(ctl.busy()).toBe(true);
    expect(ctl.keptEstimateKbps()).toBe(44_100);
  });
});
