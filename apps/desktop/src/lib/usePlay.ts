import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { catalog, type WatchDetail } from "./catalog";
import { errorMessage } from "./errors";
import { goReauth, needsReauth } from "./nav";
import { defaultChoice, playRequest, type PlayChoice } from "./play";

/**
 * The single path from any Play/Resume control to the native player: resolve what to play (`getWatchState`), apply the
 * defaults or the user's picks, hand the request to the host (`player_play`). There is no browser video-element path.
 */
export function usePlay(serverId: string) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const play = useCallback(
    async (itemId: string, opts: { fromStart?: boolean; choice?: PlayChoice; watch?: WatchDetail } = {}) => {
      setBusy(true);
      setError(null);
      try {
        const w = opts.watch ?? (await catalog.watch(serverId, itemId));
        const choice = opts.choice ?? defaultChoice(w);
        if (!choice) {
          setError(t("item.noVersions"));
          return;
        }
        await catalog.play(playRequest(serverId, w, choice, opts.fromStart ?? false));
      } catch (e) {
        if (needsReauth(e)) goReauth(navigate, serverId, e);
        else setError(errorMessage(t, e));
      } finally {
        setBusy(false);
      }
    },
    [serverId, t, navigate],
  );

  return {
    play,
    busy,
    error,
    clearError: () => {
      setError(null);
    },
  };
}
