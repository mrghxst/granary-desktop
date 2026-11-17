import { createContext, useContext } from "react";
import type { LinkState, RoomSnapshot } from "../lib/together";

export interface TogetherApi {
  room: RoomSnapshot | null;
  serverId: string | null;
  link: LinkState;
  /** Why the last room ended for us (`room_closed`, `connection_replaced`, `forbidden`, `not_found`, `left`). */
  ended: string | null;
  start: (serverId: string, contentId: string, fileId: string | null) => Promise<void>;
  join: (serverId: string, code: string) => Promise<void>;
  leave: () => void;
}

export const TogetherContext = createContext<TogetherApi | null>(null);

const INERT: TogetherApi = {
  room: null,
  serverId: null,
  link: "closed",
  ended: null,
  start: () => Promise.reject(new Error("no Watch Together here")),
  join: () => Promise.reject(new Error("no Watch Together here")),
  leave: () => undefined,
};

export function useTogether(): TogetherApi {
  return useContext(TogetherContext) ?? INERT;
}
