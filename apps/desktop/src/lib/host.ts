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

/** Mirrors `AboutInfo` in apps/desktop/src-tauri/src/about.rs. */
export interface AboutInfo {
  version: string;
  /** Commit the build was made from, or `unknown`. */
  commit: string;
  license: string;
  sourceUrl: string;
  releasesUrl: string;
  notices: string;
  logDir: string;
}

const NO_HOST_ABOUT: AboutInfo = {
  version: "dev",
  commit: "unknown",
  license: "AGPL-3.0-or-later",
  sourceUrl: "",
  releasesUrl: "",
  notices: "",
  logDir: "",
};

export async function aboutInfo(): Promise<AboutInfo> {
  return isTauri() ? invoke<AboutInfo>("about_info") : NO_HOST_ABOUT;
}

/** The fixed set of external places the host opens in the default browser / Explorer. */
export type ExternalLink = "source" | "releases" | "license" | "log_folder";

export async function openLink(link: ExternalLink): Promise<void> {
  if (isTauri()) await invoke("open_link", { link });
}

export async function tailLogs(lines: number): Promise<string[]> {
  return isTauri() ? invoke<string[]>("logs_tail", { lines }) : [];
}

/** Writes the redacted logs to the Downloads folder; returns the file path. */
export async function exportLogs(): Promise<string> {
  return isTauri() ? invoke<string>("logs_export") : "";
}

/** Tell the host the first screen is interactive (it logs `ready` with the cold-start time once). */
export function signalReady(): void {
  if (isTauri()) void invoke("app_ready").catch(() => undefined);
}
