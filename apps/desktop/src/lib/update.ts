import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

/** Mirrors `UpdateStatus` in apps/desktop/src-tauri/src/update.rs (ADR-0015). */
export type UpdateStatus =
  | { phase: "idle" | "unavailable" | "checking" | "upToDate" }
  | { phase: "downloading"; version: string; notes: string | null; received: number; total: number | null }
  | { phase: "ready"; version: string; notes: string | null }
  | { phase: "installing"; version: string }
  | { phase: "failed"; message: string };

const IDLE: UpdateStatus = { phase: "idle" };

export const updates = {
  status: (): Promise<UpdateStatus> =>
    isTauri() ? invoke<UpdateStatus>("update_status") : Promise.resolve({ phase: "unavailable" }),
  check: (): Promise<UpdateStatus> =>
    isTauri() ? invoke<UpdateStatus>("update_check") : Promise.resolve({ phase: "unavailable" }),
  /** Closes Granary, installs the downloaded version and reopens it. */
  install: (): Promise<null> => invoke<null>("update_install"),
};

/** Download progress 0..1, or null when the size is unknown. */
export function downloadShare(s: UpdateStatus): number | null {
  if (s.phase !== "downloading" || !s.total) return null;
  return Math.max(0, Math.min(1, s.received / s.total));
}

/** The live update status: read once, then kept current from the host's `update-status` events. */
export function useUpdateStatus(): UpdateStatus {
  const [status, setStatus] = useState<UpdateStatus>(IDLE);
  useEffect(() => {
    if (!isTauri()) return;
    let alive = true;
    let unlisten: (() => void) | undefined;
    void listen<UpdateStatus>("update-status", (e) => {
      setStatus(e.payload);
    }).then((u) => {
      if (alive) unlisten = u;
      else u();
    });
    void updates
      .status()
      .then((s) => {
        if (alive) setStatus(s);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);
  return status;
}

const SEEN_KEY = "granary.lastVersion";

/**
 * The version Granary was updated to since the last start, once: compared with the version remembered in this
 * WebView's storage. A first start (nothing remembered) is not an update.
 */
export function takeUpdatedFrom(current: string): string | null {
  try {
    const seen = localStorage.getItem(SEEN_KEY);
    localStorage.setItem(SEEN_KEY, current);
    return seen && seen !== current && current !== "dev" ? seen : null;
  } catch {
    return null;
  }
}
