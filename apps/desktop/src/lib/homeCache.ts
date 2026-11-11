/**
 * The home rows last loaded per server, in memory only: coming back to Home draws them at once and reloads them
 * behind (stale-while-revalidate). Forgotten when the session or profile changes, so one profile's rows never show
 * under another; nothing is written to disk.
 */
import type { Section } from "./catalog";

const rows = new Map<string, Section[]>();

export const homeCache = {
  get: (serverId: string): Section[] | undefined => rows.get(serverId),
  set: (serverId: string, value: Section[]): void => {
    rows.set(serverId, value);
  },
  forget: (serverId: string): void => {
    rows.delete(serverId);
  },
};
