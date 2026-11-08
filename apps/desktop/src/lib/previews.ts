import { isTauri, invoke } from "@tauri-apps/api/core";
import type { Settings } from "./settings";
import type { Trickplay } from "./player";
import { trickplayTile } from "./player";

/** Seek-preview providers, tried in the order the user set (ADR-0011). */
export type PreviewSourceId = "server" | "seekr" | "local";

export interface PreviewSourceSetting {
  id: PreviewSourceId;
  enabled: boolean;
}

export const DEFAULT_PREVIEW_SOURCES: PreviewSourceSetting[] = [
  { id: "server", enabled: true },
  { id: "seekr", enabled: true },
  { id: "local", enabled: true },
];

/** Mirrors `seekr::SeekrTrack`: cue times are seconds in Seekr's source; `client = source * scale`. */
export interface SeekrTrack {
  scale: number;
  cues: { start: number; end: number; url: string; x: number; y: number; w: number; h: number }[];
}

/** Image-scheme server id under which the host serves Seekr tile sheets (`seekr::SHEET_SERVER`). */
export const SEEKR_SHEETS = "~seekr";

export type PreviewProvider =
  | { kind: "server"; trickplay: Trickplay }
  | { kind: "seekr"; track: SeekrTrack }
  | { kind: "local" }
  | { kind: "none" };

export interface Tile {
  /** Image-scheme server id the URL is served under. */
  server: "file" | typeof SEEKR_SHEETS;
  url: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** The sprite for a position in a Seekr track (`source = client / scale`). */
export function seekrTile(track: SeekrTrack, seconds: number): Tile | undefined {
  const scale = track.scale > 0 ? track.scale : 1;
  const s = Math.max(0, seconds / scale);
  const cues = track.cues;
  if (cues.length === 0) return undefined;
  // Binary search for the last cue starting at or before s.
  let lo = 0;
  let hi = cues.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if ((cues[mid]?.start ?? 0) <= s) lo = mid;
    else hi = mid - 1;
  }
  const c = cues[lo];
  if (!c) return undefined;
  return { server: SEEKR_SHEETS, url: c.url, x: c.x, y: c.y, width: c.w, height: c.h };
}

/** The tile for a position from whichever provider is active (local frames are not tiles). */
export function providerTile(p: PreviewProvider, seconds: number): Tile | undefined {
  if (p.kind === "server") {
    const t = trickplayTile(p.trickplay, seconds);
    return t ? { server: "file", ...t } : undefined;
  }
  if (p.kind === "seekr") return seekrTile(p.track, seconds);
  return undefined;
}

/** The enabled sources in the user's order; nothing when previews are off. */
export function enabledSources(player: Settings["player"]): PreviewSourceId[] {
  if (!player.seekPreviews) return [];
  // Older browser-fallback settings may lack the list; the host always normalises it.
  const saved = player.previewSources as PreviewSourceSetting[] | undefined;
  const list = saved && saved.length > 0 ? saved : DEFAULT_PREVIEW_SOURCES;
  return list.filter((s) => s.enabled).map((s) => s.id);
}

export interface ResolveDeps {
  /** Server trickplay for the file, or null when it has none / the request failed. */
  server: () => Promise<Trickplay | null>;
  /** Seekr previews, or null (no key, not found, limit reached, offline). */
  seekr: () => Promise<SeekrTrack | null>;
}

/** Walk the chain: the first source that has previews wins; `local` always can. */
export async function resolvePreviewProvider(
  order: PreviewSourceId[],
  deps: ResolveDeps,
  alive: () => boolean = () => true,
): Promise<PreviewProvider> {
  for (const id of order) {
    if (!alive()) break;
    if (id === "local") return { kind: "local" };
    try {
      if (id === "server") {
        const trickplay = await deps.server();
        if (trickplay && trickplay.thumbnail_count > 0) return { kind: "server", trickplay };
      } else {
        const track = await deps.seekr();
        if (track && track.cues.length > 0) return { kind: "seekr", track };
      }
    } catch {
      /* this source failed: try the next one */
    }
  }
  return { kind: "none" };
}

function call<T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!isTauri()) return Promise.reject(Object.assign(new Error("not running inside Granary"), { code: "no_host" }));
  return invoke<T>(cmd, args);
}

/** Seekr API key handling; the key is kept by the host in Windows Credential Manager and never read back. */
export const seekr = {
  keyStatus: () => call<boolean>("seekr_key_status"),
  /** `true` when Seekr accepted (and the host saved) the key; `null` deletes it. */
  setKey: (key: string | null) => call<boolean>("seekr_key_set", { key }),
  track: (serverId: string, itemId: string, durationMs: number) =>
    call<SeekrTrack | null>("preview_seekr", { serverId, itemId, durationMs }),
};
