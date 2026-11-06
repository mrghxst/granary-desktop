import { invoke, isTauri } from "@tauri-apps/api/core";

/** Mirrors `AppInfo` in apps/desktop/src-tauri/src/commands.rs (serde camelCase). */
export interface AppInfo {
  version: string;
  libmpv: { ok: true; apiVersion: string } | { ok: false; error: string };
}

/** Host information; outside Tauri (plain browser, tests) a stub is returned. */
export async function appInfo(): Promise<AppInfo> {
  if (!isTauri()) return { version: "dev", libmpv: { ok: false, error: "not running inside Granary" } };
  return invoke<AppInfo>("app_info");
}
