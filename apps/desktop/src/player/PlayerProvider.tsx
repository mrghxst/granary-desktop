import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { errorMessage } from "../lib/errors";
import { catalog, type Episode } from "../lib/catalog";
import { bumpRefresh } from "../lib/events";
import { defaultChoice, playRequest } from "../lib/play";
import {
  IDLE_STATE,
  mediaLines,
  neighbours,
  player,
  type PlanInfo,
  type PlaybackContext,
  type PlayerState,
  type WindowState,
} from "../lib/player";
import { DEFAULT_SETTINGS, loadSettings, type Settings } from "../lib/settings";
import { PlayerContext, type PlayerApi, type PlayerError, type StartOptions } from "./context";

export function PlayerProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<PlayerState>(IDLE_STATE);
  const [plan, setPlan] = useState<PlanInfo | null>(null);
  const [ctx, setCtx] = useState<PlaybackContext | null>(null);
  const [win, setWin] = useState<WindowState>({ fullscreen: false, mini: false });
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [error, setError] = useState<PlayerError | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const ctxRef = useRef(ctx);
  const autoplayCancelled = useRef(false);
  const settingsRef = useRef(settings);
  useEffect(() => {
    ctxRef.current = ctx;
    settingsRef.current = settings;
  });

  const start = useCallback(async (serverId: string, itemId: string, opts: StartOptions = {}) => {
    const w = opts.watch ?? (await catalog.watch(serverId, itemId));
    const choice = opts.choice ?? defaultChoice(w);
    if (!choice) return false;
    let episodes: Episode[] = [];
    if (w.series_id && w.season_number != null) {
      episodes = await catalog.episodes(serverId, w.series_id, w.season_number).catch(() => []);
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
            if (n.fatal) setError({ code: n.code, message: n.message });
            else setToast(errorMessage(tRef.current, n));
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

  const api = useMemo<PlayerApi>(
    () => ({
      state,
      plan,
      ctx,
      window: win,
      settings,
      error,
      toast,
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
    [state, plan, ctx, win, settings, error, toast, start],
  );

  return <PlayerContext.Provider value={api}>{children}</PlayerContext.Provider>;
}
