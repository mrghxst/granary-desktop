import type { NavigateFunction } from "react-router";
import { isCommandError, needsSignIn } from "./silo";

/** Router state of the sign-in screen. */
export interface ProfilesState {
  /** Reached without the user asking for the picker (app start, lost profile): pick the last profile again. */
  auto?: boolean;
}

/** The app opens the last server once per run; afterwards the server list is a place the user chose to go to. */
export const startup = { opened: false };

/** Where the app starts for a signed-in server: its profile picker, which re-selects the last profile. */
export function goResume(navigate: NavigateFunction, serverId: string): void {
  const state: ProfilesState = { auto: true };
  void navigate(`${serverBase(serverId)}/profiles`, { replace: true, state });
}

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

/** The current profile is missing (after a restart) or its PIN unlock expired: the profile picker fixes it. */
export function needsProfile(e: unknown): boolean {
  return isCommandError(e) && (e.code === "profile_required" || e.code === "profile_verification_required");
}

/** An error that sends the user to sign-in or to the profile picker instead of being shown inline. */
export function needsReauth(e: unknown): boolean {
  return needsSignIn(e) || needsProfile(e);
}

/** Navigate for an error accepted by {@link needsReauth}. */
export function goReauth(navigate: NavigateFunction, serverId: string, err: unknown): void {
  if (needsProfile(err)) goResume(navigate, serverId);
  else goSignIn(navigate, serverId, err);
}

/** Details page of a catalog item. */
export const itemPath = (serverId: string, id: string) => `${serverBase(serverId)}/item/${encodeURIComponent(id)}`;

/** A person (actor, director, writer) and their filmography. */
export const personPath = (serverId: string, id: string) => `${serverBase(serverId)}/person/${encodeURIComponent(id)}`;
