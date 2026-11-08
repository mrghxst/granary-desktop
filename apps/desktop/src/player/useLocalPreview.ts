import { useEffect, useRef, useState } from "react";
import { player } from "../lib/player";

/** Hover must rest this long on an uncached spot before the exact frame is requested (a sweep asks nothing). */
const SETTLE_MS = 60;
/** A nearby frame stands in for the exact one only up to this share of the file away. */
const NEAR_SHARE = 1 / 12;
/**
 * Exact frames asked for at once. The host downloads keyframes in parallel and decodes one at a time (about 0.1 s for
 * a 4K frame), so a quick series of rests does not queue behind one slow download.
 */
const MAX_INFLIGHT = 3;

/**
 * Seconds between preview spots: about 400 over the file, between 2 s and 10 s. Matroska files are previewed from their
 * keyframe index (one small range read per frame, cached per keyframe on the host), so spots can be close together;
 * the host answers repeated spots inside one keyframe from its cache.
 */
export function previewInterval(duration: number): number {
  if (!Number.isFinite(duration) || duration <= 0) return 10;
  return Math.min(10, Math.max(2, Math.ceil(duration / 400)));
}

export interface LocalPreview {
  url: string;
  /** A nearby frame shown while the exact one loads (or when it could not be decoded). */
  approx: boolean;
}

/**
 * Seek-bar previews decoded from the playing file when the server has no trickplay sheets (ADR-0009 revision 4):
 * - once per file the host starts a paced background fill of a coarse-to-fine keyframe grid;
 * - every hover shows the nearest frame the host already has, at once (a cache read, no network);
 * - when the pointer rests, the exact frame for the spot is fetched and
 *   replaces the stand-in. Up to {@link MAX_INFLIGHT} spots load at once; the spot under the pointer always goes next.
 */
export function useLocalPreview(enabled: boolean, at: number | null, duration: number): LocalPreview | null {
  const cache = useRef(new Map<number, string | null>());
  const inflight = useRef(new Set<number>());
  const mounted = useRef(true);
  const wanted = useRef<number | null>(null);
  // The ref serves the request pump; the state copy is what render reads.
  const [frames, setFrames] = useState<ReadonlyMap<number, string | null>>(() => new Map());
  const [near, setNear] = useState<{ spot: number; at: number; url: string } | null>(null);
  const step = previewInterval(duration);
  const spot = enabled && at !== null && Number.isFinite(at) && at >= 0 ? Math.floor(at / step) * step : null;

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (enabled && duration > 0) void player.previewWarm().catch(() => undefined);
  }, [enabled, duration]);

  useEffect(() => {
    wanted.current = spot;
    if (spot === null || cache.current.has(spot)) return;
    // Stand-in first: answered from the host's cache, so it is there before the exact frame could be.
    void player
      .previewNear(spot)
      .catch(() => null)
      .then((n) => {
        if (mounted.current && n && wanted.current === spot) setNear({ spot, ...n });
      });
    const pump = () => {
      const s = wanted.current;
      const busy = inflight.current;
      if (!mounted.current || s === null || cache.current.has(s) || busy.has(s) || busy.size >= MAX_INFLIGHT) return;
      busy.add(s);
      void player
        .preview(s)
        .catch(() => null)
        .then((url) => {
          cache.current.set(s, url);
          busy.delete(s);
          if (!mounted.current) return;
          setFrames(new Map(cache.current));
          pump();
        });
    };
    const id = setTimeout(pump, SETTLE_MS);
    return () => {
      clearTimeout(id);
    };
  }, [spot]);

  if (spot === null) return null;
  const exact = frames.get(spot);
  if (exact) return { url: exact, approx: false };
  // The last stand-in stays up while the next one is asked for, so the tile does not flicker between spots.
  if (near && Math.abs(near.at - spot) <= Math.max(2 * step, duration * NEAR_SHARE)) {
    return { url: near.url, approx: true };
  }
  return null;
}
