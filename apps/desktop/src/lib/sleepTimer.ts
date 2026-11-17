/**
 * The player's sleep timer and the "Are you still watching?" streak, as in the web app
 * (`silo-server/web/src/player/components/SleepTimerMenu.tsx`, `web/src/player/stillWatching.ts`, @749db96a).
 */

export type SleepSetting =
  | { kind: "off" }
  /** Counts down only while the video plays, so a pause does not use up the time. */
  | { kind: "time"; remainingMs: number }
  /** Stop at the end of what is playing now: no next episode. */
  | { kind: "episode" };

export const SLEEP_OFF: SleepSetting = { kind: "off" };
export const SLEEP_MINUTES = [15, 30, 45, 60, 90, 120];

/** The timer after `elapsedMs` of playing; `expired` when it just ran out. */
export function sleepTick(s: SleepSetting, elapsedMs: number): { next: SleepSetting; expired: boolean } {
  if (s.kind !== "time") return { next: s, expired: false };
  const remainingMs = s.remainingMs - elapsedMs;
  return remainingMs <= 0
    ? { next: SLEEP_OFF, expired: true }
    : { next: { kind: "time", remainingMs }, expired: false };
}

/** `m:ss`, or `h:mm:ss` from an hour on. */
export function sleepCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, "0");
  return h > 0 ? `${String(h)}:${String(m).padStart(2, "0")}:${s}` : `${String(m)}:${s}`;
}

/**
 * With a limit of N, N automatic next episodes in a row start on their own and the next one waits for the viewer.
 * Any start the viewer makes clears the streak. 0 turns the question off.
 */
export function stillWatchingDue(automaticStarts: number, limit: number): boolean {
  return limit > 0 && automaticStarts >= limit;
}
