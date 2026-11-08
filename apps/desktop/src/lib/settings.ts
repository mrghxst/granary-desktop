import { invoke, isTauri } from "@tauri-apps/api/core";
import { useCallback, useEffect, useState } from "react";
import i18n, { detectLanguage } from "../i18n";

/** Mirrors apps/desktop/src-tauri/src/settings.rs (serde camelCase). Stored in `%APPDATA%\Granary\settings.json`. */
export interface Settings {
  language: "system" | "en" | "de";
  theme: "midnight-cinema" | "true-black";
  /** Quality tags (4K, HDR, audio) on poster cards. */
  posterTags: boolean;
  player: {
    /** mpv `hwdec`. */
    hwdec: "auto-safe" | "auto-copy-safe" | "no";
    /** Start the next episode automatically after a countdown. */
    autoplayNext: boolean;
    /** Seconds per arrow-key seek. */
    seekStep: number;
    /** Seek-bar previews decoded locally when the server has no trickplay sheets. */
    seekPreviews: boolean;
  };
  display: {
    /** Windows HDR switching: never, for HDR content only (default) or for all video. Always restored. */
    hdrMode: "off" | "content" | "always";
    /** Refresh strategy: VRR → fits → exact multiple → resample (default), match content exactly (TVs), never. */
    refresh: RefreshStrategy;
    /** Per-monitor preferences keyed by the monitor device path (`MonitorView.key`). */
    monitors: Record<string, MonitorSettings>;
  };
  audio: {
    /** Bitstream compressed formats to an AV receiver (opt-in, HTPC setups). */
    passthrough: boolean;
    passthroughCodecs: ("ac3" | "eac3" | "dts" | "dts-hd" | "truehd")[];
    channels: "auto" | "stereo" | "5.1" | "7.1";
    /** WASAPI exclusive mode. */
    exclusive: boolean;
    /** mpv audio device name; `auto` follows the Windows default device. */
    device: string;
    /** Normalise downmix levels (5.1/7.1 → stereo). */
    normalizeDownmix: boolean;
  };
  subtitles: {
    /** mpv `sub-scale`. */
    scale: number;
    /** Prefer SDH/hearing-impaired tracks when choosing automatically. */
    preferSdh: boolean;
  };
}

export type RefreshStrategy = "auto" | "match" | "never";

export interface MonitorSettings {
  /** Friendly name when saved (display only). */
  name: string;
  /** The monitor runs VRR for video: never switch its refresh rate. */
  vrr: boolean;
  /** Peak luminance override (nits, 80–10000); null = DXGI value. */
  peakNits: number | null;
  /** Black level override (nits, 0–5); null = automatic. */
  blackNits: number | null;
  refresh: "default" | RefreshStrategy;
}

export const DEFAULT_MONITOR: MonitorSettings = {
  name: "",
  vrr: false,
  peakNits: null,
  blackNits: null,
  refresh: "default",
};

/** One monitor as the host reports it (`display_list`). */
export interface MonitorView {
  key: string;
  name: string;
  hdrSupported: boolean;
  hdrEnabled: boolean;
  refreshHz: number;
  modesHz: number[];
  peakNits: number;
  blackNits: number;
  sdrWhiteNits: number;
  /** The player window is on this monitor. */
  current: boolean;
}

export interface AudioDevice {
  name: string;
  description: string;
}

export async function listMonitors(): Promise<MonitorView[]> {
  return isTauri() ? invoke<MonitorView[]>("display_list") : [];
}

export async function listAudioDevices(): Promise<AudioDevice[]> {
  return isTauri() ? invoke<AudioDevice[]>("player_audio_devices") : [];
}

export const DEFAULT_SETTINGS: Settings = {
  language: "system",
  theme: "midnight-cinema",
  posterTags: false,
  player: { hwdec: "auto-safe", autoplayNext: true, seekStep: 10, seekPreviews: true },
  display: { hdrMode: "content", refresh: "auto", monitors: {} },
  audio: {
    passthrough: false,
    passthroughCodecs: ["ac3", "eac3", "dts"],
    channels: "stereo",
    exclusive: false,
    device: "auto",
    normalizeDownmix: true,
  },
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
  if (s.posterTags) document.documentElement.dataset.posterTags = "true";
  else delete document.documentElement.dataset.posterTags;
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
