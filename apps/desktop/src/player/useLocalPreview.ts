import { useEffect, useRef, useState } from "react";
import { player } from "../lib/player";

/** Hover must rest this long on an uncached spot before a frame is requested (a sweep over the bar asks nothing). */
const SETTLE_MS = 140;
/** While a frame loads, show a decoded neighbour at most this many intervals away. */
const NEAR = 2;

/**
 * Seconds between preview frames: at least 30 s, and about 120 frames over the whole file for long ones (rounded to
 * 10 s, at most 2 min). Every frame is a seek in the remote file, so fewer, coarser frames keep previews quick on
 * slow or high-latency sources and keep the playing stream undisturbed.
 */
export function previewInterval(duration: number): number {
  if (!Number.isFinite(duration) || duration <= 0) return 30;
  return Math.min(120, Math.max(30, Math.ceil(duration / 120 / 10) * 10));
}

/**
 * Seek-bar previews decoded from the playing file when the server has no trickplay sheets (ADR-0009). Hover positions
 * snap to `previewInterval`; at most one request is in flight and, when it returns, the newest hover position is
 * fetched next, so sweeping the bar never queues a backlog. Frames are cached for the lifetime of the controls.
 */
export function useLocalPreview(enabled: boolean, at: number | null, duration: number): string | null {
  const cache = useRef(new Map<number, string | null>());
  const inflight = useRef(false);
  const mounted = useRef(true);
  const wanted = useRef<number | null>(null);
  // The ref serves the request pump; the state copy is what render reads.
  const [frames, setFrames] = useState<ReadonlyMap<number, string | null>>(() => new Map());
  const step = previewInterval(duration);
  const spot = enabled && at !== null && Number.isFinite(at) && at >= 0 ? Math.floor(at / step) * step : null;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    wanted.current = spot;
    const pump = () => {
      const s = wanted.current;
      if (!mounted.current || inflight.current || s === null || cache.current.has(s)) return;
      inflight.current = true;
      void player
        .preview(s)
        .catch(() => null)
        .then((url) => {
          cache.current.set(s, url);
          inflight.current = false;
          if (!mounted.current) return;
          setFrames(new Map(cache.current));
          pump();
        });
    };
    if (spot === null || cache.current.has(spot)) return;
    const id = setTimeout(pump, SETTLE_MS);
    return () => {
      clearTimeout(id);
    };
  }, [spot]);

  if (spot === null) return null;
  const exact = frames.get(spot);
  if (exact !== undefined) return exact;
  let best: string | null = null;
  let dist = Infinity;
  for (const [s, url] of frames) {
    const d = Math.abs(s - spot);
    if (url && d < dist && d <= NEAR * step) {
      dist = d;
      best = url;
    }
  }
  return best;
}
