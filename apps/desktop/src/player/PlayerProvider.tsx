import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { errorMessage } from "../lib/errors";
import { catalog, type Episode } from "../lib/catalog";
import { bumpRefresh } from "../lib/events";
import { defaultChoice, playRequest } from "../lib/play";
import {
  IDLE_STATE,
  mediaLines,
  episodesAround,
  neighbours,
  player,
  type PlanInfo,
  type PlaybackContext,
  type PlayerState,
  type WindowState,
} from "../lib/player";
import { DEFAULT_SETTINGS, loadSettings, type Settings } from "../lib/settings";
import { PlayerContext, type Launch, type PlayerApi, type PlayerError, type StartOptions } from "./context";

/** The launch screen gives up after this long even without a first frame (the controls' spinner takes over). */
const LAUNCH_TIMEOUT_MS = 30000;

export function PlayerProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<PlayerState>(IDLE_STATE);
  const [plan, setPlan] = useState<PlanInfo | null>(null);
  const [ctx, setCtx] = useState<PlaybackContext | null>(null);
  const [win, setWin] = useState<WindowState>({ fullscreen: false, mini: false });
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [error, setError] = useState<PlayerError | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [launch, setLaunch] = useState<Launch | null>(null);
  // The launch ends at the first frame of the new file, so it must first see this attempt leave the old state.
  const launchArmed = useRef(false);
  const ctxRef = useRef(ctx);
  const autoplayCancelled = useRef(false);
  const settingsRef = useRef(settings);
  useEffect(() => {
    ctxRef.current = ctx;
    settingsRef.current = settings;
  });

  const startInner = useCallback(async (serverId: string, itemId: string, opts: StartOptions) => {
    const w = opts.watch ?? (await catalog.watch(serverId, itemId));
    const choice = opts.choice ?? defaultChoice(w);
    if (!choice) {
      setLaunch(null);
      return false;
    }
    const lines = mediaLines(w);
    setLaunch((l) =>
      l
        ? {
            ...l,
            title: l.title ?? lines.title,
            subtitle: l.subtitle ?? lines.subtitle ?? null,
            backdrop: l.backdrop ?? w.backdrop_url ?? null,
          }
        : l,
    );
    let episodes: Episode[] = [];
    if (w.series_id && w.season_number != null) {
      const seriesId = w.series_id;
      episodes = await episodesAround(
        w.content_id,
        w.season_number,
        async () =>
          (await catalog.seasons(serverId, seriesId))
            .filter((x) => !x.is_specials)
            .map((x) => x.season_number)
            .sort((a, b) => a - b),
        (season) => catalog.episodes(serverId, seriesId, season),
      );
    }
    const next: PlaybackContext = {
      serverId,
      watch: w,
      version: choice.version,
      audioIndex: choice.audioIndex,
      subtitleIndex: choice.subtitleIndex,
      episodes,
    };
    const near = neighbours(next);
    const s = await loadSettings().catch(() => settingsRef.current);
    setSettings(s);
    autoplayCancelled.current = false;
    setCtx(next);
    setPlan(null);
    setError(null);
    await catalog.play({
      ...playRequest(serverId, w, choice, opts.fromStart ?? false),
      ...mediaLines(w),
      hasPrevious: near.previous !== undefined,
      hasNext: near.next !== undefined,
    });
    return true;
  }, []);

  const start = useCallback(
    async (serverId: string, itemId: string, opts: StartOptions = {}) => {
      launchArmed.current = false;
      setLaunch({ serverId, ...opts.art, backdrop: opts.art?.backdrop ?? opts.watch?.backdrop_url ?? null });
      try {
        return await startInner(serverId, itemId, opts);
      } catch (e) {
        setLaunch(null);
        throw e;
      }
    },
    [startInner],
  );

  const startRef = useRef(start);
  const { t } = useTranslation();
  const tRef = useRef(t);
  useEffect(() => {
    startRef.current = start;
    tRef.current = t;
  });

  useEffect(() => {
    const unlisten = [
      player.onState(setState),
      player.onWindow(setWin),
      player.onNotice((n) => {
        switch (n.kind) {
          case "plan":
            setPlan(n);
            break;
          case "error":
            if (n.fatal) {
              setError({ code: n.code, message: n.message });
              setLaunch(null);
            } else setToast(errorMessage(tRef.current, n));
            break;
          case "ended": {
            // Autoplay: the next episode starts right away when the file ends (the countdown card may have run).
            const c = ctxRef.current;
            const nextEp = neighbours(c).next;
            if (c && nextEp && settingsRef.current.player.autoplayNext && !autoplayCancelled.current) {
              void startRef.current(c.serverId, nextEp.content_id).catch(() => undefined);
            }
            bumpRefresh();
            break;
          }
          case "stopped":
            setLaunch(null);
            bumpRefresh();
            break;
          case "progress":
            break;
        }
      }),
    ];
    void player
      .state()
      .then(setState)
      .catch(() => undefined);
    void player
      .window()
      .then(setWin)
      .catch(() => undefined);
    return () => {
      unlisten.forEach((p) => {
        void p.then((u) => {
          u();
        });
      });
    };
  }, []);

  // The page turns transparent while a playback attempt runs, so the native video window below shows through.
  useEffect(() => {
    const root = document.documentElement;
    if (state.active) root.dataset.videoActive = "true";
    else delete root.dataset.videoActive;
  }, [state.active]);

  // First frame of the new attempt: the state went through loading (or idle) and now plays or sits paused.
  useEffect(() => {
    if (!launch) return;
    const { phase } = state;
    if (phase === "loading" || phase === "idle") launchArmed.current = true;
    else if (launchArmed.current && (phase === "playing" || phase === "paused") && !state.buffering) setLaunch(null);
  }, [launch, state]);
  useEffect(() => {
    if (!launch) return;
    const id = setTimeout(() => {
      setLaunch(null);
    }, LAUNCH_TIMEOUT_MS);
    return () => {
      clearTimeout(id);
    };
  }, [launch]);

  const api = useMemo<PlayerApi>(
    () => ({
      state,
      plan,
      ctx,
      window: win,
      settings,
      error,
      toast,
      launch,
      showToast: setToast,
      start,
      updateCtx: (patch) => {
        setCtx((c) => (c ? { ...c, ...patch } : c));
      },
      setWindow: setWin,
      dismissError: () => {
        setError(null);
      },
      cancelAutoplay: () => {
        autoplayCancelled.current = true;
      },
    }),
    [state, plan, ctx, win, settings, error, toast, launch, start],
  );

  return <PlayerContext.Provider value={api}>{children}</PlayerContext.Provider>;
}
