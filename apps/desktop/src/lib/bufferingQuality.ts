/**
 * Lowering quality when playback keeps buffering: the client controller of
 * `silo-server/docs/design/2026-10-17-buffering-quality.md@749db96a` (ADR-0014).
 *
 * It is a pure state machine on an injected monotonic clock. Media events go in (`handle`), timers fire through
 * `advance` at `nextDeadline`, and its decisions come out through `onOutput`: a `quality_change` replan to send,
 * a pill becoming visible, or a short notice. The player owns the pills' buttons and reports them back as events.
 * `src/lib/bufferingQuality.test.ts` replays every case of the design's conformance vectors
 * (`schemas/buffering-quality/v1/vectors.json`).
 */

export type BufferingMode = "never" | "ask" | "always";

/** The plan_summary `buffering_action` of a plan. */
export type BufferingAction =
  "none" | "ignored" | "lowered" | "restored" | "accepted" | "kept" | "timed_out" | "cant_lower";

export interface QualityRung {
  label: string;
  bitrate_kbps?: number | null;
  height?: number | null;
  preserves_source?: boolean;
}

/** One adoption of a playback plan, as the controller needs it. */
export interface AdoptedPlan {
  /** A local adoption identity (a server plan id can repeat). */
  plan_id: string;
  /** `effective_recipe.bitrate_kbps`, else `source.bitrate_kbps`; 0 when unknown. */
  delivered_kbps: number;
  /** The `quality_preference` the plan was asked for (`auto` or a rung or resolution). */
  quality: string;
  /** `active_quality` (`plan_active_quality_v1`). */
  active_quality?: string | null;
  available_qualities: QualityRung[];
  /** Degradation warning codes. */
  degradation_warnings?: string[];
}

export type ReplanOperation =
  "quality_change" | "track_change" | "seek_reanchor" | "seek_failure_recovery" | "failure_recovery" | "output_change";

export interface ReplanBody {
  operation: ReplanOperation;
  quality_preference: string;
  bandwidth_estimate_kbps?: number;
  bandwidth_estimate_source?: "observed_rebuffer";
  observed_throughput_kbps?: number;
  learned_limit?: "clear";
}

export type BufferingEvent =
  | { type: "plan_adopted"; plan: AdoptedPlan; controllerInitiated?: boolean }
  | { type: "playing" }
  | { type: "waiting"; cause?: "seek" | "pause" | "reload" | "renderer" }
  | { type: "pause" }
  | { type: "seek" }
  | { type: "transport_error_start" }
  | { type: "transport_error_end" }
  | { type: "replan_failed" }
  | { type: "select_lower" }
  | { type: "select_keep" }
  | { type: "select_restore" }
  | { type: "back" }
  | { type: "pill_obscured" }
  | { type: "pill_unobscured" }
  | { type: "client_replan"; operation: ReplanOperation }
  | { type: "quality_changed" }
  | { type: "pick_sent" }
  | { type: "pick_refused" }
  | { type: "new_attempt" };

export type BufferingOutput =
  | { type: "replan"; body: ReplanBody }
  | { type: "prompt_shown" }
  | { type: "undo_shown" }
  | { type: "notice"; key: "lowered" | "cant_lower" };

/** The design's web constants (vectors.json `constants`). */
export const BQ = {
  STARTUP_GRACE_MS: 10_000,
  STALL_MIN_MS: 1_000,
  STALLS_BEFORE_TRIGGER: 2,
  STALL_WINDOW_MS: 120_000,
  LONG_STALL_MS: 6_000,
  PROMPT_MS: 15_000,
  REOFFER_MIN_MS: 120_000,
  NO_LOWER_COOLDOWN_MS: 300_000,
  REARM_AFTER_UNADOPTED_MS: 60_000,
  MIN_ESTIMATE_KBPS: 100,
} as const;

export interface ControllerOptions {
  mode: BufferingMode;
  /** Monotonic milliseconds. */
  now: () => number;
  onOutput: (output: BufferingOutput) => void;
  /**
   * Before any plan of the attempt has played, a trigger needs evidence that the plan's media arrived more slowly
   * than its delivered bitrate (the design's "Trigger"). Defaults to yes, as the vectors assume.
   */
  startEvidence?: () => boolean;
  /** The throughput the current plan received so far (kbps), sent as `observed_throughput_kbps`. */
  throughput?: () => number | undefined;
}

interface Stall {
  end: number;
  dur: number;
}

type Wait =
  | { kind: "exempt" }
  | { kind: "renderer" }
  /** `from`: when the stall counts from (null while an outage suspends it). */
  | { kind: "stall"; from: number | null; long: boolean };

interface Pill {
  kind: "prompt" | "undo";
  /** Milliseconds left on its timer. */
  remaining: number;
  /** Shown to the viewer (not obscured) since; null while it waits or is hidden. */
  shownAt: number | null;
  /** The timer runs since (shown and not paused). */
  runningSince: number | null;
  firstShownAt: number | null;
}

interface Pending {
  kind: "lowered" | "accepted" | "restore";
  fromDelivered: number;
  sentAt: number;
}

/** floor(delivered x 7 / 10) in exact integer arithmetic. */
export function stepDownEstimate(deliveredKbps: number): number {
  return Math.floor((Math.trunc(deliveredKbps) * 7) / 10);
}

/** The highest menu entry below what plays (the web's `lowerQualityOption`). */
export function lowerRung(plan: AdoptedPlan): QualityRung | null {
  const named = plan.active_quality
    ? plan.available_qualities.find((q) => q.label === plan.active_quality)?.bitrate_kbps
    : undefined;
  const ceiling = named != null && named > 0 ? Math.min(named, plan.delivered_kbps) : plan.delivered_kbps;
  let best: QualityRung | null = null;
  for (const q of plan.available_qualities) {
    const k = q.bitrate_kbps ?? 0;
    if (k > 0 && k < ceiling && (best === null || k > (best.bitrate_kbps ?? 0))) best = q;
  }
  return best;
}

export class BufferingController {
  private mode: BufferingMode;
  private readonly opts: ControllerOptions;

  // The title: survives a new attempt.
  private answered = false;
  private cantLowerShown = false;
  private reofferUntil: number | null = null;
  // The attempt.
  private keptEstimate: number | null = null;
  private attemptPlayed = false;
  private attemptGraceEnd: number | null = null;
  // The plan.
  private plan: AdoptedPlan | null = null;
  private planPlayed = false;
  private graceEnd: number | null = null;
  private history: Stall[] = [];
  private wait: Wait | null = null;
  private seekPending = false;
  private paused = false;
  private outage = false;
  private cooldownUntil: number | null = null;
  private rearmAt: number | null = null;
  // Requests and pills.
  private pending: Pending | null = null;
  private pickPending = false;
  private qualityChange = false;
  private pill: Pill | null = null;
  private obscured = false;
  private readonly actions = new Map<string, BufferingAction>();

  constructor(opts: ControllerOptions) {
    this.opts = opts;
    this.mode = opts.mode;
  }

  setMode(mode: BufferingMode) {
    this.mode = mode;
  }

  /** The plan_summary `buffering_action` of an adoption. */
  bufferingAction(planId: string): BufferingAction {
    return this.actions.get(planId) ?? "none";
  }

  /** The pill on screen now, with its time left (ms). */
  visiblePill(): { kind: "prompt" | "undo"; remainingMs: number } | null {
    const p = this.pill;
    if (!p || p.shownAt === null) return null;
    const ran = p.runningSince === null ? 0 : this.now() - p.runningSince;
    return { kind: p.kind, remainingMs: Math.max(0, p.remaining - ran) };
  }

  /** The kept step-down estimate the client's own Auto replans repeat. */
  keptEstimateKbps(): number | null {
    return this.keptEstimate;
  }

  /** A lowering, pick or restore is waiting for its plan. */
  busy(): boolean {
    return this.pending !== null || this.pickPending;
  }

  private now() {
    return this.opts.now();
  }

  private emit(o: BufferingOutput) {
    this.opts.onOutput(o);
  }

  private setAction(a: BufferingAction) {
    if (this.plan) this.actions.set(this.plan.plan_id, a);
  }

  /* ------------------------------------------------------------ timers */

  private longDeadline(): number | null {
    const w = this.wait;
    return w?.kind === "stall" && w.from !== null && !w.long ? w.from + BQ.LONG_STALL_MS : null;
  }

  private pillDeadline(): number | null {
    const p = this.pill;
    return p?.runningSince != null ? p.runningSince + p.remaining : null;
  }

  /** The next moment a timer fires, if any. */
  nextDeadline(): number | null {
    const all = [this.longDeadline(), this.pillDeadline(), this.cooldownUntil, this.reofferUntil, this.rearmAt].filter(
      (t): t is number => t !== null,
    );
    return all.length ? Math.min(...all) : null;
  }

  /** Fire every timer due at the clock's time, earliest first. */
  advance() {
    const now = this.now();
    for (let guard = 0; guard < 32; guard++) {
      const due = this.nextDeadline();
      if (due === null || due > now) return;
      if (this.longDeadline() === due && this.wait?.kind === "stall") {
        this.wait.long = true;
        this.evaluate();
      } else if (this.pillDeadline() === due) {
        this.pillExpired();
      } else if (this.cooldownUntil === due) {
        this.cooldownUntil = null;
        this.reevaluateLongStall();
      } else if (this.reofferUntil === due) {
        this.reofferUntil = null;
        this.reevaluateLongStall();
      } else if (this.rearmAt === due) {
        this.rearmAt = null;
        this.restartHistory();
      } else {
        return;
      }
    }
  }

  /* ------------------------------------------------------------ stalls and triggers */

  private anchorGrace() {
    if (this.graceEnd !== null) return;
    this.graceEnd = this.now() + BQ.STARTUP_GRACE_MS;
    if (!this.attemptPlayed && this.attemptGraceEnd === null) this.attemptGraceEnd = this.graceEnd;
  }

  /** Fresh stall history with the startup grace over; a stall in progress is timed from now (timeout, rearm). */
  private restartHistory() {
    this.history = [];
    this.graceEnd = Number.NEGATIVE_INFINITY;
    if (this.wait?.kind === "stall") {
      this.wait = { kind: "stall", from: this.outage ? null : this.now(), long: false };
    }
  }

  private triggerFires(): boolean {
    const now = this.now();
    const recent = this.history.filter((s) => s.end >= now - BQ.STALL_WINDOW_MS);
    const counted = recent.filter((s) => s.dur >= BQ.STALL_MIN_MS).length >= BQ.STALLS_BEFORE_TRIGGER;
    const long = recent.some((s) => s.dur >= BQ.LONG_STALL_MS) || (this.wait?.kind === "stall" && this.wait.long);
    if (!counted && !long) return false;
    // Before the attempt has played, only a link measured too slow lowers (a start failing for another reason
    // must not drive Auto and the learned limit down).
    return this.attemptPlayed || (this.opts.startEvidence?.() ?? true);
  }

  private suppressed(): boolean {
    const now = this.now();
    return (
      this.answered ||
      this.pending !== null ||
      this.pickPending ||
      this.pill !== null ||
      (this.cooldownUntil !== null && now < this.cooldownUntil) ||
      (this.rearmAt !== null && now < this.rearmAt) ||
      (this.reofferUntil !== null && now < this.reofferUntil)
    );
  }

  private reevaluateLongStall() {
    if (this.wait?.kind === "stall" && this.wait.long) this.evaluate();
  }

  private evaluate() {
    const plan = this.plan;
    if (!plan || this.outage || !this.triggerFires() || this.suppressed()) return;
    if (this.mode === "never") {
      this.setAction("ignored");
      return;
    }
    const lower = lowerRung(plan);
    if (plan.quality === "auto") {
      const estimate = stepDownEstimate(plan.delivered_kbps);
      if (plan.delivered_kbps <= 0 || estimate < BQ.MIN_ESTIMATE_KBPS || !lower) {
        this.cantLower();
      } else if (this.mode === "always") {
        this.sendLowering("lowered");
      } else {
        this.showPill("prompt");
      }
      return;
    }
    // A quality the viewer picked is never lowered without asking.
    if (!lower) this.cantLower();
    else this.showPill("prompt");
  }

  private cantLower() {
    this.setAction("cant_lower");
    if (!this.cantLowerShown) {
      this.cantLowerShown = true;
      this.emit({ type: "notice", key: "cant_lower" });
    }
  }

  private sendLowering(kind: "lowered" | "accepted") {
    const plan = this.plan;
    if (!plan) return;
    const estimate = stepDownEstimate(plan.delivered_kbps);
    const body: ReplanBody = {
      operation: "quality_change",
      quality_preference: "auto",
      bandwidth_estimate_kbps: estimate,
      bandwidth_estimate_source: "observed_rebuffer",
    };
    const observed = this.opts.throughput?.();
    if (observed !== undefined && observed >= 100)
      body.observed_throughput_kbps = Math.min(1_000_000, Math.floor(observed));
    this.keptEstimate = estimate;
    this.pending = { kind, fromDelivered: plan.delivered_kbps, sentAt: this.now() };
    this.setAction(kind);
    this.emit({ type: "replan", body });
  }

  /* ------------------------------------------------------------ pills */

  private showPill(kind: "prompt" | "undo") {
    this.pill = { kind, remaining: BQ.PROMPT_MS, shownAt: null, runningSince: null, firstShownAt: null };
    this.revealPill();
  }

  private revealPill() {
    const p = this.pill;
    if (!p || p.shownAt !== null || this.obscured) return;
    const now = this.now();
    p.shownAt = now;
    p.firstShownAt ??= now;
    p.runningSince = this.paused ? null : now;
    this.emit({ type: p.kind === "prompt" ? "prompt_shown" : "undo_shown" });
  }

  private freezePill() {
    const p = this.pill;
    if (p?.runningSince != null) {
      p.remaining -= this.now() - p.runningSince;
      p.runningSince = null;
    }
  }

  private hidePill() {
    this.freezePill();
    if (this.pill) this.pill.shownAt = null;
  }

  private pillExpired() {
    const p = this.pill;
    this.pill = null;
    if (!p) return;
    if (p.kind === "prompt") {
      // Not an answer: history clears and a new prompt waits for the re-offer time.
      this.setAction("timed_out");
      this.reofferUntil = (p.firstShownAt ?? this.now()) + BQ.REOFFER_MIN_MS;
      this.restartHistory();
    } else {
      this.reevaluateLongStall();
    }
  }

  private visible(kind: "prompt" | "undo"): boolean {
    return this.pill?.kind === kind && this.pill.shownAt !== null;
  }

  /* ------------------------------------------------------------ events */

  handle(e: BufferingEvent) {
    this.advance();
    const now = this.now();
    switch (e.type) {
      case "plan_adopted":
        this.adopt(e.plan, e.controllerInitiated !== false);
        break;
      case "playing": {
        this.paused = false;
        this.outage = false;
        const w = this.wait;
        if (w?.kind === "stall" && w.from !== null) {
          const dur = now - w.from;
          // A start wait without evidence of a slow link does not count.
          if (dur > 0 && (this.attemptPlayed || (this.opts.startEvidence?.() ?? true))) {
            this.history.push({ end: now, dur });
          }
        }
        this.wait = null;
        this.seekPending = false;
        this.anchorGrace();
        this.planPlayed = true;
        this.attemptPlayed = true;
        if (this.pill?.shownAt != null && this.pill.runningSince === null) this.pill.runningSince = now;
        this.evaluate();
        break;
      }
      case "waiting":
        this.anchorGrace();
        if (e.cause === "seek" || e.cause === "pause" || e.cause === "reload" || this.seekPending) {
          this.wait = { kind: "exempt" };
        } else if (e.cause === "renderer") {
          this.wait = { kind: "renderer" };
        } else if (this.wait === null || this.wait.kind === "renderer") {
          this.wait = {
            kind: "stall",
            from: this.outage ? null : Math.max(now, this.graceEnd ?? now),
            long: false,
          };
        }
        break;
      case "pause":
        this.paused = true;
        this.freezePill();
        // A pause ends a wait uncounted.
        this.wait = null;
        break;
      case "seek":
        // Until the plan first runs, a seek (the start position included) does not end the start's wait, and
        // buffering after it still belongs to the start.
        if (this.planPlayed) {
          this.seekPending = true;
          if (this.wait !== null) this.wait = { kind: "exempt" };
        }
        break;
      case "transport_error_start":
        this.outage = true;
        if (this.wait?.kind === "stall") this.wait = { kind: "stall", from: null, long: false };
        break;
      case "transport_error_end":
        this.outage = false;
        if (this.wait?.kind === "stall" && this.wait.from === null)
          this.wait = { kind: "stall", from: now, long: false };
        break;
      case "replan_failed":
        if (this.pending) {
          this.rearmAt = this.pending.sentAt + BQ.REARM_AFTER_UNADOPTED_MS;
          this.pending = null;
        }
        break;
      case "select_lower": {
        if (!this.visible("prompt") || !this.plan) break;
        this.pill = null;
        const plan = this.plan;
        if (plan.quality === "auto") {
          this.sendLowering("accepted");
        } else {
          const lower = lowerRung(plan);
          if (!lower) break;
          this.pending = { kind: "accepted", fromDelivered: plan.delivered_kbps, sentAt: now };
          this.setAction("accepted");
          this.emit({ type: "replan", body: { operation: "quality_change", quality_preference: lower.label } });
        }
        break;
      }
      case "select_keep":
        if (!this.visible("prompt")) break;
        this.pill = null;
        this.answered = true;
        this.setAction("kept");
        break;
      case "select_restore":
        if (!this.visible("undo")) break;
        this.pill = null;
        this.keptEstimate = null;
        this.answered = true;
        this.setAction("restored");
        this.pending = { kind: "restore", fromDelivered: this.plan?.delivered_kbps ?? 0, sentAt: now };
        this.emit({
          type: "replan",
          body: { operation: "quality_change", quality_preference: "auto", learned_limit: "clear" },
        });
        break;
      case "back":
        if (this.visible("prompt")) {
          this.pill = null;
          this.answered = true;
          this.setAction("kept");
        } else if (this.visible("undo")) {
          this.pill = null;
          this.reevaluateLongStall();
        }
        break;
      case "pill_obscured":
        this.obscured = true;
        this.hidePill();
        break;
      case "pill_unobscured":
        this.obscured = false;
        this.revealPill();
        break;
      case "client_replan": {
        const quality = this.plan?.quality ?? "auto";
        const body: ReplanBody = { operation: e.operation, quality_preference: quality };
        if (quality === "auto" && this.keptEstimate !== null) body.bandwidth_estimate_kbps = this.keptEstimate;
        this.emit({ type: "replan", body });
        break;
      }
      case "quality_changed":
        this.qualityChange = true;
        this.pill = null;
        break;
      case "pick_sent":
        this.pill = null;
        this.pickPending = true;
        break;
      case "pick_refused":
        this.pickPending = false;
        this.reevaluateLongStall();
        break;
      case "new_attempt":
        // A new attempt of the same title: the answer, the notice and the re-offer wait belong to the title.
        this.keptEstimate = null;
        this.attemptPlayed = false;
        this.attemptGraceEnd = null;
        this.pending = null;
        this.pickPending = false;
        this.qualityChange = false;
        this.pill = null;
        break;
    }
  }

  private adopt(next: AdoptedPlan, maybeInitiated: boolean) {
    const now = this.now();
    const initiated = maybeInitiated && this.pending !== null && !this.qualityChange;
    const pending = this.pending;
    this.pending = null;
    this.pill = null;
    this.cooldownUntil = null;
    this.rearmAt = null;
    if (!initiated && this.qualityChange) {
      // The viewer changed quality: a new answer, no re-offer wait, no kept estimate.
      this.qualityChange = false;
      this.pickPending = false;
      this.answered = false;
      this.reofferUntil = null;
      this.keptEstimate = null;
    }
    this.plan = next;
    this.planPlayed = false;
    this.history = [];
    this.wait = null;
    this.seekPending = false;
    this.outage = false;
    // A replacement adopted before the attempt has played keeps the start's grace.
    this.graceEnd = !this.attemptPlayed && this.attemptGraceEnd !== null ? this.attemptGraceEnd : null;
    if (!initiated || !pending || pending.kind === "restore") return;
    const warnings = next.degradation_warnings ?? [];
    const lower =
      !warnings.includes("quality_reduction_unavailable") &&
      (next.delivered_kbps > 0 ? next.delivered_kbps < pending.fromDelivered : true);
    if (lower) {
      if (this.pickPending) return;
      if (pending.kind === "lowered") this.showPill("undo");
      else this.emit({ type: "notice", key: "lowered" });
      return;
    }
    this.setAction("cant_lower");
    this.cooldownUntil = now + BQ.NO_LOWER_COOLDOWN_MS;
    if (!this.pickPending) {
      this.cantLowerShown = true;
      this.emit({ type: "notice", key: "cant_lower" });
    }
  }
}
