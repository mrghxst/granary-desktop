import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from "react";
import {
  BufferingController,
  type BufferingMode,
  type BufferingOutput,
  type QualityRung,
} from "../lib/bufferingQuality";
import type { PlanInfo, PlayerState } from "../lib/player";
import { quality, type QualityRequest } from "../lib/quality";

/** Silence on a progressive stream that makes a wait an outage (the design's `OUTAGE_NO_DATA_MS`). */
const OUTAGE_PROGRESSIVE_MS = 4_000;
/** The same on segmented delivery, where the server holds a segment request while it produces it. */
const OUTAGE_SEGMENTED_MS = 15_000;
const NETWORK_POLL_MS = 1_000;
/**
 * Seconds of media arrival before a start wait can count as a slow link. The first second also holds the server
 * opening the file, which on network-backed storage (Usenet, debrid) can take seconds on a fast link; one such sample
 * would lower a fast connection and leave a learned limit for a day.
 */
const START_EVIDENCE_MS = 3_000;

type Status = "none" | "playing" | "paused" | "seeking" | "loading" | "waiting" | "other";

function statusOf(s: PlayerState): Status {
  if (!s.active) return "none";
  if (s.paused) return "paused";
  if (s.phase === "seeking") return "seeking";
  if (s.phase === "loading") return "loading";
  if (s.buffering || s.phase === "buffering") return "waiting";
  if (s.phase === "playing") return "playing";
  return "other";
}

function requestFor(body: Extract<BufferingOutput, { type: "replan" }>["body"]): QualityRequest {
  const r: QualityRequest = { quality: body.quality_preference };
  if (body.bandwidth_estimate_kbps !== undefined) r.estimateKbps = body.bandwidth_estimate_kbps;
  if (body.bandwidth_estimate_source) r.estimateSource = body.bandwidth_estimate_source;
  if (body.observed_throughput_kbps !== undefined) r.observedKbps = body.observed_throughput_kbps;
  if (body.learned_limit === "clear") r.clearLearnedLimit = true;
  return r;
}

interface View {
  pill: { kind: "prompt" | "undo"; remainingMs: number } | null;
  deadline: number | null;
  busy: boolean;
}

const IDLE: View = { pill: null, deadline: null, busy: false };

/** What the controller shows, as a store React reads without touching the controller during render. */
function viewStore() {
  let snap = IDLE;
  const listeners = new Set<() => void>();
  return {
    subscribe: (l: () => void) => {
      listeners.add(l);
      return () => {
        listeners.delete(l);
      };
    },
    get: () => snap,
    set: (c: BufferingController | null) => {
      const pill = c?.visiblePill() ?? null;
      const next: View = { pill, deadline: c?.nextDeadline() ?? null, busy: c?.busy() ?? false };
      const same =
        next.deadline === snap.deadline &&
        next.busy === snap.busy &&
        next.pill?.kind === snap.pill?.kind &&
        Math.ceil((next.pill?.remainingMs ?? 0) / 1000) === Math.ceil((snap.pill?.remainingMs ?? 0) / 1000);
      if (same) return;
      snap = next;
      for (const l of listeners) l();
    },
  };
}

export interface BufferingQualityOptions {
  plan: PlanInfo | null;
  state: PlayerState;
  /** The title playing (the controller's answer and notices belong to it). */
  title: string | null;
  /** A video plays outside a Watch Party room (the controller does not act on audiobooks or in rooms). */
  enabled: boolean;
  mode: BufferingMode;
  /** The pill cannot be seen: a menu, another pill or the mini player covers it. */
  obscured: boolean;
  onNotice: (key: "lowered" | "cant_lower") => void;
  onError: (e: unknown) => void;
}

/**
 * Runs the buffering-quality controller (ADR-0014) on what mpv reports: waits for the cache, seeks, pauses and
 * reloads become its media events, and media arriving (mpv's cache speed) tells a slow link from an outage and
 * measures the throughput a lowering reports. Its replans go to the host as `quality_change`.
 */
export function useBufferingQuality(o: BufferingQualityOptions) {
  const [store] = useState(viewStore);
  const view = useSyncExternalStore(store.subscribe, store.get);
  const ctl = useRef<BufferingController | null>(null);
  const rerender = useCallback(() => {
    store.set(ctl.current);
  }, [store]);
  const awaiting = useRef<"controller" | "pick" | null>(null);
  const adoption = useRef("");
  const attempt = useRef("");
  const status = useRef<Status>("none");
  const planPlayed = useRef(false);
  const net = useRef({ lastDataAt: 0, bytes: 0, ms: 0, outage: false });
  const latest = useRef(o);
  useEffect(() => {
    latest.current = o;
  });

  const throughput = useCallback((): number | undefined => {
    const n = net.current;
    return n.ms >= 1000 ? (n.bytes * 8) / n.ms : undefined;
  }, []);

  const onOutput = useCallback(
    (out: BufferingOutput) => {
      const opts = latest.current;
      if (out.type === "notice") {
        opts.onNotice(out.key);
      } else if (out.type === "replan") {
        awaiting.current = "controller";
        quality.change(requestFor(out.body)).catch((e: unknown) => {
          awaiting.current = null;
          ctl.current?.handle({ type: "replan_failed" });
          opts.onError(e);
          rerender();
        });
      }
      rerender();
    },
    [rerender],
  );

  // One controller per title while it may act.
  const { title, enabled } = o;
  useEffect(() => {
    if (!title || !enabled) {
      ctl.current = null;
      rerender();
      return;
    }
    ctl.current = new BufferingController({
      mode: latest.current.mode,
      now: () => performance.now(),
      onOutput,
      throughput,
      // Before the first frame only a link measured slower than the plan, over a few seconds, counts.
      startEvidence: () => {
        const k = net.current.ms >= START_EVIDENCE_MS ? throughput() : undefined;
        const delivered = latest.current.plan?.delivered_kbps ?? 0;
        return k !== undefined && delivered > 0 && k < delivered;
      },
    });
    adoption.current = "";
    attempt.current = "";
    status.current = "none";
    rerender();
    return () => {
      ctl.current = null;
    };
  }, [title, enabled, onOutput, throughput, rerender]);

  useEffect(() => {
    ctl.current?.setMode(o.mode);
  }, [o.mode]);

  // Plan adoptions.
  const { plan } = o;
  useEffect(() => {
    const c = ctl.current;
    if (!plan || !c) return;
    const key = `${plan.attempt_id}:${String(plan.adoption)}`;
    if (key === adoption.current) return; // an in-place update (a subtitle list refresh)
    if (attempt.current && attempt.current !== plan.attempt_id) c.handle({ type: "new_attempt" });
    attempt.current = plan.attempt_id;
    adoption.current = key;
    net.current = { lastDataAt: performance.now(), bytes: 0, ms: 0, outage: false };
    planPlayed.current = false;
    status.current = "none";
    const adopted = {
      plan_id: key,
      delivered_kbps: plan.delivered_kbps ?? 0,
      quality: plan.quality_preference,
      active_quality: plan.active_quality ?? null,
      available_qualities: plan.available_qualities as QualityRung[],
      degradation_warnings: plan.degradation_warnings,
    };
    const waiting = awaiting.current;
    awaiting.current = null;
    if (plan.cause === "quality_change" && waiting === "pick") {
      c.handle({ type: "quality_changed" });
      c.handle({ type: "plan_adopted", plan: adopted });
    } else if (plan.cause === "quality_change" && waiting === "controller") {
      c.handle({ type: "plan_adopted", plan: adopted });
    } else {
      c.handle({ type: "plan_adopted", plan: adopted, controllerInitiated: false });
    }
    rerender();
  }, [plan, rerender]);

  // Media events from the player's state.
  const { state } = o;
  useEffect(() => {
    const c = ctl.current;
    if (!c) return;
    const next = statusOf(state);
    const prev = status.current;
    if (next === prev) return;
    status.current = next;
    switch (next) {
      case "playing":
        planPlayed.current = true;
        c.handle({ type: "playing" });
        break;
      case "paused":
      case "other":
        c.handle({ type: "pause" });
        break;
      case "seeking":
        c.handle({ type: "seek" });
        c.handle({ type: "waiting", cause: "seek" });
        break;
      case "loading":
        // A reload of a source that played; before that, the start's own wait.
        c.handle(planPlayed.current ? { type: "waiting", cause: "reload" } : { type: "waiting" });
        break;
      case "waiting":
        c.handle({ type: "waiting" });
        break;
      case "none":
        break;
    }
    rerender();
  }, [state, rerender]);

  // How media arrives: outages and the plan's throughput.
  const active = state.active && enabled;
  const delivery = plan?.delivery ?? "";
  useEffect(() => {
    if (!active) return;
    const limit = delivery.includes("hls") ? OUTAGE_SEGMENTED_MS : OUTAGE_PROGRESSIVE_MS;
    let alive = true;
    const id = setInterval(() => {
      quality
        .network()
        .then((r) => {
          if (!alive || !r) return;
          const n = net.current;
          const now = performance.now();
          const speed = r.cacheSpeed ?? 0;
          if (speed > 0) {
            n.bytes += speed * (NETWORK_POLL_MS / 1000);
            n.ms += NETWORK_POLL_MS;
          }
          // A demuxer that has all it wants is not a silent link.
          if (speed > 0 || r.cacheIdle) n.lastDataAt = now;
          const waitingNow = status.current === "waiting" || status.current === "loading";
          if (!n.outage && waitingNow && now - n.lastDataAt >= limit) {
            n.outage = true;
            ctl.current?.handle({ type: "transport_error_start" });
          } else if (n.outage && speed > 0) {
            n.outage = false;
            ctl.current?.handle({ type: "transport_error_end" });
          }
        })
        .catch(() => undefined);
    }, NETWORK_POLL_MS);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, [active, delivery]);

  // Pills that cannot be seen wait.
  const { obscured } = o;
  useEffect(() => {
    ctl.current?.handle({ type: obscured ? "pill_obscured" : "pill_unobscured" });
    rerender();
  }, [obscured, rerender]);

  // Timers: fire the controller's next deadline; tick while a pill counts down.
  const { pill, deadline, busy } = view;
  useEffect(() => {
    if (deadline === null) return;
    const id = setTimeout(
      () => {
        ctl.current?.advance();
        rerender();
      },
      Math.max(0, deadline - performance.now()) + 5,
    );
    return () => {
      clearTimeout(id);
    };
  }, [deadline, rerender]);
  const counting = pill !== null;
  useEffect(() => {
    if (!counting) return;
    const id = setInterval(rerender, 250);
    return () => {
      clearInterval(id);
    };
  }, [counting, rerender]);

  const act = useCallback(
    (kind: "lower" | "keep" | "restore" | "back") => {
      const map = { lower: "select_lower", keep: "select_keep", restore: "select_restore", back: "back" } as const;
      ctl.current?.handle({ type: map[kind] });
      rerender();
    },
    [rerender],
  );

  /** A pick from the quality menu: holds for this playback only. */
  const pick = useCallback(
    async (label: string) => {
      ctl.current?.handle({ type: "pick_sent" });
      awaiting.current = "pick";
      rerender();
      try {
        await quality.change({ quality: label });
      } catch (e) {
        awaiting.current = null;
        ctl.current?.handle({ type: "pick_refused" });
        rerender();
        throw e;
      }
    },
    [rerender],
  );

  return { pill, act, pick, busy };
}
