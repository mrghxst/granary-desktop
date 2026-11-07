import { useEffect, useRef, useState } from "react";
import { useRetryTick } from "./events";

/**
 * Load data for a key; a new key discards the previous result (no flash of stale data, no setState inside the effect
 * body). `key === null` means "nothing to load". A changed `refresh` reloads the same key in place, keeping the shown
 * data until the new result arrives (live updates from the events socket, progress after playback). The global retry
 * tick (network back, Retry button) reloads too: in place when data is shown, replacing the error otherwise.
 */
export function useLoad<T>(key: string | null, load: () => Promise<T>, refresh = 0) {
  const loader = useRef(load);
  useEffect(() => {
    loader.current = load;
  });
  const [state, setState] = useState<{ key: string | null; data?: T; error?: unknown }>({ key: null });
  const retry = useRetryTick();
  const lastRefresh = useRef(refresh);
  const lastRetry = useRef(retry);

  useEffect(() => {
    if (key === null) return;
    let alive = true;
    const inPlace = lastRefresh.current !== refresh || lastRetry.current !== retry;
    lastRefresh.current = refresh;
    lastRetry.current = retry;
    loader.current().then(
      (data) => {
        if (alive) setState({ key, data });
      },
      (error: unknown) => {
        // A failed background refresh keeps what is shown.
        if (alive) setState((s) => (inPlace && s.key === key && s.data !== undefined ? s : { key, error }));
      },
    );
    return () => {
      alive = false;
    };
  }, [key, refresh, retry]);

  const current = key !== null && state.key === key;
  return {
    data: current ? state.data : undefined,
    error: current ? state.error : undefined,
    loading: key !== null && !current,
  };
}
