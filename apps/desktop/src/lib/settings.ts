import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import i18n, { detectLanguage } from "../i18n";

/** Mirrors apps/desktop/src-tauri/src/settings.rs (serde camelCase). Stored in `%APPDATA%\Granary\settings.json`. */
export interface Settings {
  language: "system" | "en" | "de";
  theme: "midnight-cinema" | "true-black";
  player: {
    /** mpv `hwdec`. */
    hwdec: "auto-safe" | "auto-copy-safe" | "no";
    /** Start the next episode automatically after a countdown. */
    autoplayNext: boolean;
    /** Seconds per arrow-key seek. */
    seekStep: number;
  };
  display: {
    /** Switch Windows HDR on for HDR content when it is off, and back afterwards. */
    autoHdr: boolean;
    /** Match the display refresh rate to the video frame rate (skipped on VRR displays). */
    matchRefreshRate: boolean;
  };
  audio: {
    /** Bitstream compressed formats to an AV receiver (opt-in, HTPC setups). */
    passthrough: boolean;
    passthroughCodecs: ("ac3" | "eac3" | "dts" | "dts-hd" | "truehd")[];
    channels: "auto" | "stereo" | "5.1" | "7.1";
    /** WASAPI exclusive mode. */
    exclusive: boolean;
  };
  subtitles: {
    /** mpv `sub-scale`. */
    scale: number;
    /** Prefer SDH/hearing-impaired tracks when choosing automatically. */
    preferSdh: boolean;
  };
}

export const DEFAULT_SETTINGS: Settings = {
  language: "system",
  theme: "midnight-cinema",
  player: { hwdec: "auto-safe", autoplayNext: true, seekStep: 10 },
  display: { autoHdr: true, matchRefreshRate: true },
  audio: { passthrough: false, passthroughCodecs: ["ac3", "eac3", "dts"], channels: "auto", exclusive: false },
  subtitles: { scale: 1, preferSdh: false },
};

const STORAGE_KEY = "granary.settings"; // browser/dev fallback only; the host file is authoritative.

export async function loadSettings(): Promise<Settings> {
  if (isTauri()) return invoke<Settings>("settings_get");
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    return raw ? { ...DEFAULT_SETTINGS, ...(JSON.parse(raw) as Partial<Settings>) } : DEFAULT_SETTINGS;
  } catch {
    return DEFAULT_SETTINGS;
  }
}

export async function saveSettings(s: Settings): Promise<Settings> {
  if (isTauri()) return invoke<Settings>("settings_set", { settings: s });
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  } catch {
    /* storage unavailable: settings last for this session */
  }
  return s;
}

/** Apply the UI-level settings: theme on <html>, language. */
export function applySettings(s: Settings): void {
  document.documentElement.dataset.theme = s.theme;
  const lng = s.language === "system" ? detectLanguage() : s.language;
  if (i18n.language !== lng) void i18n.changeLanguage(lng);
  document.documentElement.lang = lng;
}

export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<unknown>(null);

  useEffect(() => {
    let alive = true;
    loadSettings()
      .then((s) => {
        if (alive) setSettings(s);
      })
      .catch((e: unknown) => {
        if (alive) {
          setError(e);
          setSettings(DEFAULT_SETTINGS);
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  const update = useCallback(async (next: Settings) => {
    setSettings(next);
    applySettings(next);
    try {
      const saved = await saveSettings(next);
      setSettings(saved);
      setError(null);
    } catch (e) {
      setError(e);
    }
  }, []);

  return { settings, update, error };
}
