/**
 * The home rows last loaded per server, in memory only: coming back to Home draws them at once and reloads them
 * behind (stale-while-revalidate). Forgotten when the session or profile changes, so one profile's rows never show
 * under another; nothing is written to disk.
 */
const rows = new Map<string, unknown>();

export const homeCache = {
  get: <T>(serverId: string): T | undefined => rows.get(serverId) as T | undefined,
  set: (serverId: string, value: unknown): void => {
    rows.set(serverId, value);
  },
  forget: (serverId: string): void => {
    rows.delete(serverId);
  },
};
