import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/* Mirrors apps/desktop/src-tauri/src/silo.rs (serde camelCase). Tokens never reach the WebView. */

export interface CommandError {
  code: string;
  message: string;
  status?: number;
  retryAfter?: number;
  problem?: string;
}

export interface AccountView {
  userId: string;
  username: string;
}

export interface ServerView {
  serverId: string;
  origin: string;
  serverVersion: string;
  untested: boolean;
  insecure: boolean;
  active: boolean;
  account?: AccountView;
  lastProfileId?: string;
}

export interface ProbeView {
  origin: string;
  serverId: string;
  serverVersion: string;
  apiMajor: number;
  untested: boolean;
  insecure: boolean;
  reportedOrigin?: string;
  known: boolean;
}

export interface LoginView {
  account: AccountView;
  passwordChangeRequired: boolean;
}

export interface ProfileView {
  id: string;
  name: string;
  hasPin: boolean;
  isChild: boolean;
}

export interface LibraryView {
  id: string;
  name: string;
  kind: string;
}

export function isCommandError(e: unknown): e is CommandError {
  return (
    typeof e === "object" &&
    e !== null &&
    typeof (e as CommandError).code === "string" &&
    typeof (e as CommandError).message === "string"
  );
}

// An Error carrying the same `code`/`message` shape the host rejects with.
const noHost = (): Error & CommandError => Object.assign(new Error("not running inside Granary"), { code: "no_host" });

function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(noHost());
  return invoke<T>(cmd, args);
}

export const silo = {
  /** Saved servers; empty outside the desktop host. */
  serversList: (): Promise<ServerView[]> => (isTauri() ? invoke<ServerView[]>("servers_list") : Promise.resolve([])),
  serversProbe: (url: string) => call<ProbeView>("servers_probe", { url }),
  serversAdd: (url: string) => call<ServerView>("servers_add", { url }),
  serversRemove: (serverId: string) => call<null>("servers_remove", { serverId }),
  serversActivate: (serverId: string) => call<null>("servers_activate", { serverId }),
  authLogin: (serverId: string, username: string, password: string) =>
    call<LoginView>("auth_login", { serverId, username, password }),
  authLogout: (serverId: string) => call<null>("auth_logout", { serverId }),
  profilesList: (serverId: string) => call<ProfileView[]>("profiles_list", { serverId }),
  profilesSelect: (serverId: string, profileId: string, pin?: string) =>
    call<null>("profiles_select", { serverId, profileId, pin: pin ?? null }),
  librariesList: (serverId: string) => call<LibraryView[]>("libraries_list", { serverId }),
};

/** `auth://revoked`: the server rejected a stored session; the UI returns to sign-in. */
export async function onRevoked(handler: (serverId: string) => void): Promise<UnlistenFn> {
  if (!isTauri()) return () => undefined;
  return listen<{ serverId: string }>("auth://revoked", (e) => {
    handler(e.payload.serverId);
  });
}

/** Errors that mean "sign in again". */
export function needsSignIn(e: unknown): boolean {
  return isCommandError(e) && (e.code === "signed_out" || e.code === "not_signed_in");
}
