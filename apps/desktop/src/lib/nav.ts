import type { NavigateFunction } from "react-router";
import { isCommandError } from "./silo";

/** Router state of the sign-in screen. */
export interface LoginState {
  /** The server rejected the stored session (as opposed to "never signed in"). */
  revoked?: boolean;
}

export const serverBase = (serverId: string) => `/servers/${encodeURIComponent(serverId)}`;

/** Go to the sign-in screen of a server, telling it whether the session was revoked. */
export function goSignIn(navigate: NavigateFunction, serverId: string, err?: unknown): void {
  const state: LoginState = { revoked: err === undefined || (isCommandError(err) && err.code === "signed_out") };
  void navigate(`${serverBase(serverId)}/login`, { replace: true, state });
}
