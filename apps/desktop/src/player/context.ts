import { createContext, useContext } from "react";
import type { WatchDetail } from "../lib/catalog";
import type { PlayChoice } from "../lib/play";
import type { PlanInfo, PlaybackContext, PlayerState, WindowState } from "../lib/player";
import type { Settings } from "../lib/settings";

export interface PlayerError {
  code: string;
  message: string;
}

export interface StartOptions {
  fromStart?: boolean;
  choice?: PlayChoice;
  watch?: WatchDetail;
  /** Artwork for the launch screen when the caller already has it (the detail page). */
  art?: LaunchArt;
}

/** What the launch screen shows between Play and the first frame (artwork references, not URLs). */
export interface LaunchArt {
  title?: string | null;
  subtitle?: string | null;
  backdrop?: string | null;
  logo?: string | null;
  poster?: string | null;
}

export interface Launch extends LaunchArt {
  serverId: string;
}

export interface PlayerApi {
  state: PlayerState;
  plan: PlanInfo | null;
  ctx: PlaybackContext | null;
  window: WindowState;
  settings: Settings;
  /** The last fatal error (playback is over); cleared by `dismissError` or the next start. */
  error: PlayerError | null;
  /** Short-lived message (reconnecting, not direct play, a refused action). */
  toast: string | null;
  /** Set from Play until the first frame (or a failure): the launch screen covers the transparent page. */
  launch: Launch | null;
  showToast: (message: string | null) => void;
  /**
   * The single path from any Play control to the native player: resolve what to play, the season's episodes for
   * previous/next, then hand the request to the host. Resolves to false when the item has no playable file.
   */
  start: (serverId: string, itemId: string, opts?: StartOptions) => Promise<boolean>;
  updateCtx: (patch: Partial<PlaybackContext>) => void;
  setWindow: (w: WindowState) => void;
  dismissError: () => void;
  /** The user cancelled up-next: the end of this file does not start the next episode. */
  cancelAutoplay: () => void;
}

export const PlayerContext = createContext<PlayerApi | null>(null);

export function usePlayer(): PlayerApi {
  const api = useContext(PlayerContext);
  if (!api) throw new Error("usePlayer outside PlayerProvider");
  return api;
}
