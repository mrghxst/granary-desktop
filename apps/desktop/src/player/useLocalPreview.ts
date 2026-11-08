import { useEffect, useRef, useState } from "react";
import { player, type PreviewReply } from "../lib/player";

/** A frame from elsewhere in the file stands in for the exact one only up to this share of the file away. */
const NEAR_SHARE = 1 / 8;

/**
 * Seconds between preview spots: about 400 over the file, between 2 s and 10 s. The host answers repeated spots inside
 * one keyframe from its cache.
 */
export function previewInterval(duration: number): number {
  if (!Number.isFinite(duration) || duration <= 0) return 10;
  return Math.min(10, Math.max(2, Math.ceil(duration / 400)));
}

export interface LocalPreview {
  url: string;
  /** A frame from near the spot, shown while the exact one loads (or when it could not be decoded). */
  approx: boolean;
}

interface Cache {
  generation: number | null;
  /** Keyframe time (ms) → frame. */
  frames: ReadonlyMap<number, string>;
  /** Spot (s) → keyframe time (ms) it shows. */
  keys: ReadonlyMap<number, number>;
}

const EMPTY: Cache = { generation: null, frames: new Map(), keys: new Map() };
const ms = (s: number) => Math.round(s * 1000);

/** The cache for `generation`: a new file starts empty, and answers about an earlier file are dropped (null). */
function forGeneration(c: Cache, generation: number): Cache | null {
  if (c.generation === generation) return c;
  if (c.generation !== null && generation < c.generation) return null;
  return { ...EMPTY, generation };
}

/**
 * Seek-bar previews decoded from the playing file when the server has no trickplay sheets (ADR-0009 revision 5):
 * - every new spot under the pointer is sent to the host at once; the host lets the newest request win, fetches
 *   keyframes in parallel and pushes each frame as soon as it is decoded;
 * - until the exact frame is there, the latest frame the host sent (or the nearest it had) stays up, so the tile
 *   follows a sweep without blanking;
 * - spots seen before are answered from the page's own cache, without asking.
 */
export function useLocalPreview(enabled: boolean, at: number | null, duration: number): LocalPreview | null {
  const [cache, setCache] = useState<Cache>(EMPTY);
  const [shown, setShown] = useState<{ at: number; url: string } | null>(null);
  const latest = useRef(cache);
  const step = previewInterval(duration);
  const spot = enabled && at !== null && Number.isFinite(at) && at >= 0 ? Math.floor(at / step) * step : null;

  useEffect(() => {
    latest.current = cache;
  }, [cache]);

  useEffect(() => {
    if (enabled && duration > 0) void player.previewWarm().catch(() => undefined);
  }, [enabled, duration]);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    let stop: (() => void) | undefined;
    void player
      .onPreview((p) => {
        if (!live || !forGeneration(latest.current, p.generation)) return;
        setCache((c) => {
          const g = forGeneration(c, p.generation);
          if (!g) return c;
          return { ...g, frames: new Map(g.frames).set(ms(p.at), p.url) };
        });
        setShown({ at: p.at, url: p.url });
      })
      .then((u) => {
        if (live) stop = u;
        else u();
      });
    return () => {
      live = false;
      stop?.();
    };
  }, [enabled]);

  useEffect(() => {
    if (spot === null) return;
    const c = latest.current;
    const known = c.keys.get(spot);
    if (known !== undefined && c.frames.has(known)) return;
    let live = true;
    void player
      .previewWant(spot)
      .catch(() => null)
      .then((r: PreviewReply | null) => {
        if (!r) return;
        setCache((c) => {
          const g = forGeneration(c, r.generation);
          if (!g || r.key === null) return g ?? c;
          const keys = new Map(g.keys).set(spot, ms(r.key));
          const frames = r.url ? new Map(g.frames).set(ms(r.key), r.url) : g.frames;
          return { ...g, keys, frames };
        });
        if (!live) return;
        if (r.url && r.key !== null) setShown({ at: r.key, url: r.url });
        else if (r.near) {
          const near = r.near;
          // Keep what is up when it is at least as close (a frame the host pushed meanwhile, say).
          setShown((s) => (s && Math.abs(s.at - spot) <= Math.abs(near.at - spot) ? s : near));
        }
      });
    return () => {
      live = false;
    };
  }, [spot]);

  if (spot === null) return null;
  const key = cache.keys.get(spot);
  const exact = key === undefined ? undefined : cache.frames.get(key);
  if (exact) return { url: exact, approx: false };
  if (shown && Math.abs(shown.at - spot) <= Math.max(2 * step, duration * NEAR_SHARE)) {
    return { url: shown.url, approx: true };
  }
  return null;
}
