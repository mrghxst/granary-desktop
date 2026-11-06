import { useEffect, useRef, useState } from "react";

/**
 * Load data for a key; a new key discards the previous result (no flash of stale data, no setState inside the effect
 * body). `key === null` means "nothing to load".
 */
export function useLoad<T>(key: string | null, load: () => Promise<T>) {
  const loader = useRef(load);
  useEffect(() => {
    loader.current = load;
  });
  const [state, setState] = useState<{ key: string | null; data?: T; error?: unknown }>({ key: null });

  useEffect(() => {
    if (key === null) return;
    let alive = true;
    loader.current().then(
      (data) => {
        if (alive) setState({ key, data });
      },
      (error: unknown) => {
        if (alive) setState({ key, error });
      },
    );
    return () => {
      alive = false;
    };
  }, [key]);

  const current = key !== null && state.key === key;
  return {
    data: current ? state.data : undefined,
    error: current ? state.error : undefined,
    loading: key !== null && !current,
  };
}
