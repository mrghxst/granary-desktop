import { useEffect, useRef, useState } from "react";
import { player } from "../lib/player";

/** Seconds per preview bucket (matches the host cache, playback/preview.rs). */
const BUCKET = 2;
/** While a frame loads, show a decoded neighbour at most this many buckets away. */
const NEAR = 5;

/**
 * Seek-bar previews decoded from the playing file when the server has no trickplay sheets (ADR-0009). At most one
 * request is in flight; when it returns, the newest hover position is fetched next, so sweeping the bar never queues
 * a backlog. Frames are cached per bucket for the lifetime of the controls (they are keyed by file).
 */
export function useLocalPreview(enabled: boolean, at: number | null): string | null {
  const cache = useRef(new Map<number, string | null>());
  const inflight = useRef(false);
  const mounted = useRef(true);
  const wanted = useRef<number | null>(null);
  // The ref serves the request pump; the state copy is what render reads.
  const [frames, setFrames] = useState<ReadonlyMap<number, string | null>>(() => new Map());
  const bucket = enabled && at !== null && Number.isFinite(at) && at >= 0 ? Math.floor(at / BUCKET) : null;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    wanted.current = bucket;
    const pump = () => {
      const b = wanted.current;
      if (!mounted.current || inflight.current || b === null || cache.current.has(b)) return;
      inflight.current = true;
      void player
        .preview(b * BUCKET)
        .catch(() => null)
        .then((url) => {
          cache.current.set(b, url);
          inflight.current = false;
          if (!mounted.current) return;
          setFrames(new Map(cache.current));
          pump();
        });
    };
    pump();
  }, [bucket]);

  if (bucket === null) return null;
  const exact = frames.get(bucket);
  if (exact !== undefined) return exact;
  let best: string | null = null;
  let dist = Infinity;
  for (const [b, url] of frames) {
    const d = Math.abs(b - bucket);
    if (url && d < dist && d <= NEAR) {
      dist = d;
      best = url;
    }
  }
  return best;
}
