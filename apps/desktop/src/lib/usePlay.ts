import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { usePlayer, type StartOptions } from "../player/context";
import { errorMessage } from "./errors";
import { goReauth, needsReauth } from "./nav";

/**
 * Play/Resume controls go through the player provider (`getWatchState`, defaults or the user's picks, the season's
 * episodes, then `player_play`). There is no browser video-element path.
 */
export function usePlay(serverId: string) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { start } = usePlayer();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const play = useCallback(
    async (itemId: string, opts: StartOptions = {}) => {
      setBusy(true);
      setError(null);
      try {
        if (!(await start(serverId, itemId, opts))) setError(t("item.noVersions"));
      } catch (e) {
        if (needsReauth(e)) goReauth(navigate, serverId, e);
        else setError(errorMessage(t, e));
      } finally {
        setBusy(false);
      }
    },
    [serverId, t, navigate, start],
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
