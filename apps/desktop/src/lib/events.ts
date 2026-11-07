import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

/** Mirrors `ServerEventView` in src-tauri/src/playback/events.rs. */
export interface ServerEvent {
  serverId: string;
  /** `user_state.changed`, `catalog.item.changed`, ... or `access_changed`. */
  event: string;
  channel: string;
  data: unknown;
}

export const serverEvents = {
  connect: (serverId: string) => (isTauri() ? invoke<null>("events_connect", { serverId }) : Promise.resolve(null)),
  disconnect: () => (isTauri() ? invoke<null>("events_disconnect") : Promise.resolve(null)),
  on: async (f: (e: ServerEvent) => void): Promise<UnlistenFn> => {
    if (!isTauri()) return () => undefined;
    return listen<ServerEvent>("server://event", (e) => {
      f(e.payload);
    });
  },
};

/*
 * A refresh counter: bumped (debounced) when the server says user state or the catalog changed, and when playback
 * stops (progress moved). Screens put it in their `useLoad` refresh argument to reload in place.
 */
let tick = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
const subscribers = new Set<() => void>();

export function bumpRefresh(delayMs = 400): void {
  if (timer !== undefined) clearTimeout(timer);
  timer = setTimeout(() => {
    timer = undefined;
    tick += 1;
    subscribers.forEach((f) => {
      f();
    });
  }, delayMs);
}

function subscribe(f: () => void): () => void {
  subscribers.add(f);
  return () => {
    subscribers.delete(f);
  };
}

export function useRefreshTick(): number {
  return useSyncExternalStore(
    subscribe,
    () => tick,
    () => tick,
  );
}

/*
 * A retry counter: bumped when Windows reports the network is back and by the Retry button of a load error. Every
 * `useLoad` and the home rows reload on it, so a page that failed while offline recovers without navigating away.
 */
let retryTick = 0;
const retrySubscribers = new Set<() => void>();

export function retryLoads(): void {
  retryTick += 1;
  retrySubscribers.forEach((f) => {
    f();
  });
}

if (typeof window !== "undefined") {
  window.addEventListener("online", retryLoads);
}

function subscribeRetry(f: () => void): () => void {
  retrySubscribers.add(f);
  return () => {
    retrySubscribers.delete(f);
  };
}

export function useRetryTick(): number {
  return useSyncExternalStore(
    subscribeRetry,
    () => retryTick,
    () => retryTick,
  );
}
