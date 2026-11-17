import { useEffect, useRef } from "react";
import { usePlayer } from "../player/context";
import { catalog, type CatalogItemDetail } from "./catalog";
import { ThemeMusic } from "./themeMusic";

/**
 * Plays the first theme tune of the title a detail page shows, when the setting is on and nothing is playing.
 * Moving between the pages of one series (series, season, episode share an owner) keeps the tune going.
 */
export function useThemeMusic(serverId: string, themes: CatalogItemDetail["themes"]): void {
  const player = usePlayer();
  const enabled = player.settings.themeMusic;
  const loop = player.settings.themeMusicLoop;
  const playing = player.launch !== null || player.ctx !== null;
  const music = useRef<ThemeMusic | null>(null);
  const ownerId = themes?.owner_id ?? "";
  const themeId = themes?.items[0]?.id ?? "";

  useEffect(() => {
    const m = new ThemeMusic((owner, theme) => catalog.themeAudio(serverId, owner, theme));
    music.current = m;
    return () => {
      m.close();
      if (music.current === m) music.current = null;
    };
  }, [serverId]);

  useEffect(() => {
    const m = music.current;
    if (!m) return;
    if (!enabled || playing) m.stop(playing);
    else if (ownerId && themeId) m.play(ownerId, themeId, loop);
    // A page that is still loading has no themes yet: the tune of the page before carries on until it is known.
  }, [enabled, playing, loop, ownerId, themeId]);
}
