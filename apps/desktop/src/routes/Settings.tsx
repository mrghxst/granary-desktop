import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import { Alert } from "../components/ui";
import { errorMessage } from "../lib/errors";
import {
  DEFAULT_MONITOR,
  listAudioDevices,
  listMonitors,
  useSettings,
  type AudioDevice,
  type MonitorSettings,
  type MonitorView,
  type Settings as SettingsModel,
} from "../lib/settings";

function Section({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={id} className="flex flex-col gap-1 rounded-xl border border-border bg-card p-5">
      <h2 id={id} className="mb-2 text-lg font-semibold">
        {title}
      </h2>
      {children}
    </section>
  );
}

function RowLabel({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border/60 py-3 first-of-type:border-t-0">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{label}</p>
        {hint ? <p className="text-xs text-muted-foreground">{hint}</p> : null}
      </div>
      {children}
    </div>
  );
}

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <RowLabel label={label} {...(hint ? { hint } : {})}>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        onClick={() => {
          onChange(!checked);
        }}
        className={`relative h-6 w-11 shrink-0 rounded-full transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none ${
          checked ? "bg-primary" : "bg-surface-raised"
        }`}
      >
        <span
          className={`absolute top-0.5 size-5 rounded-full transition motion-reduce:transition-none ${
            checked ? "left-[1.375rem] bg-primary-foreground" : "left-0.5 bg-muted-foreground"
          }`}
        />
      </button>
    </RowLabel>
  );
}

function Choice<T extends string>({
  label,
  hint,
  value,
  options,
  onChange,
}: {
  label: string;
  hint?: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <RowLabel label={label} {...(hint ? { hint } : {})}>
      <select
        aria-label={label}
        value={value}
        onChange={(e) => {
          onChange(e.target.value as T);
        }}
        className="rounded-lg border border-border bg-background px-3 py-2 text-sm focus:border-ring focus:outline-none"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </RowLabel>
  );
}

/** Optional number with an automatic default; committed on blur/Enter, empty = automatic (null). */
function NumberField({
  label,
  hint,
  value,
  min,
  max,
  step,
  onCommit,
}: {
  label: string;
  hint: string;
  value: number | null;
  min: number;
  max: number;
  step: number;
  onCommit: (v: number | null) => void;
}) {
  const commit = (raw: string) => {
    const text = raw.trim();
    if (text === "") {
      if (value !== null) onCommit(null);
      return;
    }
    const n = Number(text.replace(",", "."));
    if (Number.isFinite(n) && n >= min && n <= max && n !== value) onCommit(n);
  };
  return (
    <RowLabel label={label} hint={hint}>
      <input
        key={String(value)}
        type="number"
        inputMode="decimal"
        min={min}
        max={max}
        step={step}
        aria-label={label}
        defaultValue={value ?? ""}
        onBlur={(e) => {
          commit(e.target.value);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit(e.currentTarget.value);
        }}
        className="w-28 rounded-lg border border-border bg-background px-3 py-2 text-right text-sm tabular-nums focus:border-ring focus:outline-none"
      />
    </RowLabel>
  );
}

/** Monitors and audio devices from the host (empty outside the app). */
function useOutputs() {
  const [monitors, setMonitors] = useState<MonitorView[] | null>(null);
  const [devices, setDevices] = useState<AudioDevice[]>([]);
  useEffect(() => {
    let alive = true;
    listMonitors()
      .then((m) => {
        if (alive) setMonitors(m);
      })
      .catch(() => {
        if (alive) setMonitors([]);
      });
    listAudioDevices()
      .then((d) => {
        if (alive) setDevices(d);
      })
      .catch(() => {
        /* no devices: only "Windows default" is offered */
      });
    return () => {
      alive = false;
    };
  }, []);
  return { monitors, devices };
}

const CODECS = ["ac3", "eac3", "dts", "dts-hd", "truehd"] as const;

export function Settings() {
  const { t } = useTranslation();
  const { settings: s, update, error } = useSettings();
  const { monitors, devices } = useOutputs();
  if (!s) return <p className="text-muted-foreground">{t("status.loading")}</p>;
  const set = (patch: (draft: SettingsModel) => SettingsModel) => void update(patch(s));
  const setMonitor = (m: MonitorView, patch: Partial<MonitorSettings>) => {
    set((d) => {
      const prev = d.display.monitors[m.key] ?? DEFAULT_MONITOR;
      return {
        ...d,
        display: { ...d.display, monitors: { ...d.display.monitors, [m.key]: { ...prev, ...patch, name: m.name } } },
      };
    });
  };
  const deviceOptions = [
    { value: "auto", label: t("settings.audioDeviceAuto") },
    ...devices.filter((d) => d.name !== "auto").map((d) => ({ value: d.name, label: d.description || d.name })),
  ];
  if (!deviceOptions.some((o) => o.value === s.audio.device)) {
    deviceOptions.push({ value: s.audio.device, label: s.audio.device });
  }

  return (
    <div className="flex max-w-3xl flex-col gap-6">
      <h1 className="text-3xl font-semibold tracking-tight">{t("settings.title")}</h1>
      {error ? <Alert>{errorMessage(t, error)}</Alert> : null}

      <Section id="s-general" title={t("settings.general")}>
        <Choice
          label={t("settings.language")}
          value={s.language}
          onChange={(language) => {
            set((d) => ({ ...d, language }));
          }}
          options={[
            { value: "system", label: t("settings.languageSystem") },
            { value: "en", label: "English" },
            { value: "de", label: "Deutsch" },
          ]}
        />
        <Choice
          label={t("settings.theme")}
          hint={t("settings.themeHint")}
          value={s.theme}
          onChange={(theme) => {
            set((d) => ({ ...d, theme }));
          }}
          options={[
            { value: "midnight-cinema", label: t("settings.themeDefault") },
            { value: "true-black", label: t("settings.themeBlack") },
          ]}
        />
        <Toggle
          label={t("settings.posterTags")}
          hint={t("settings.posterTagsHint")}
          checked={s.posterTags}
          onChange={(posterTags) => {
            set((d) => ({ ...d, posterTags }));
          }}
        />
      </Section>

      <Section id="s-servers" title={t("settings.servers")}>
        <RowLabel label={t("settings.serversHint")}>
          <Link
            to="/"
            className="rounded-lg bg-secondary px-4 py-2 text-sm font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
          >
            {t("settings.manageServers")}
          </Link>
        </RowLabel>
      </Section>

      <Section id="s-player" title={t("settings.player")}>
        <Choice
          label={t("settings.hwdec")}
          hint={t("settings.hwdecHint")}
          value={s.player.hwdec}
          onChange={(hwdec) => {
            set((d) => ({ ...d, player: { ...d.player, hwdec } }));
          }}
          options={[
            { value: "auto-safe", label: t("settings.hwdecAuto") },
            { value: "auto-copy-safe", label: t("settings.hwdecCopy") },
            { value: "no", label: t("settings.hwdecOff") },
          ]}
        />
        <Toggle
          label={t("settings.autoplayNext")}
          checked={s.player.autoplayNext}
          onChange={(autoplayNext) => {
            set((d) => ({ ...d, player: { ...d.player, autoplayNext } }));
          }}
        />
        <Toggle
          label={t("settings.seekPreviews")}
          hint={t("settings.seekPreviewsHint")}
          checked={s.player.seekPreviews}
          onChange={(seekPreviews) => {
            set((d) => ({ ...d, player: { ...d.player, seekPreviews } }));
          }}
        />
        <Choice
          label={t("settings.seekStep")}
          value={String(s.player.seekStep) as "5" | "10" | "15" | "30"}
          onChange={(v) => {
            set((d) => ({ ...d, player: { ...d.player, seekStep: Number(v) } }));
          }}
          options={(["5", "10", "15", "30"] as const).map((v) => ({
            value: v,
            label: t("settings.seconds", { count: Number(v) }),
          }))}
        />
      </Section>

      <Section id="s-display" title={t("settings.display")}>
        <Choice
          label={t("settings.hdrMode")}
          hint={t("settings.hdrModeHint")}
          value={s.display.hdrMode}
          onChange={(hdrMode) => {
            set((d) => ({ ...d, display: { ...d.display, hdrMode } }));
          }}
          options={[
            { value: "off", label: t("settings.hdrOff") },
            { value: "content", label: t("settings.hdrContent") },
            { value: "always", label: t("settings.hdrAlways") },
          ]}
        />
        <Choice
          label={t("settings.refresh")}
          hint={t("settings.refreshHint")}
          value={s.display.refresh}
          onChange={(refresh) => {
            set((d) => ({ ...d, display: { ...d.display, refresh } }));
          }}
          options={[
            { value: "auto", label: t("settings.refreshAuto") },
            { value: "match", label: t("settings.refreshMatch") },
            { value: "never", label: t("settings.refreshNever") },
          ]}
        />
        <h3 className="mt-4 text-sm font-semibold">{t("settings.monitors")}</h3>
        <p className="text-xs text-muted-foreground">{t("settings.monitorsHint")}</p>
        {monitors === null ? (
          <p className="py-2 text-sm text-muted-foreground">{t("status.loading")}</p>
        ) : monitors.length === 0 ? (
          <p className="py-2 text-sm text-muted-foreground">{t("settings.monitorNone")}</p>
        ) : (
          monitors.map((m) => {
            const prefs = s.display.monitors[m.key] ?? DEFAULT_MONITOR;
            return (
              <fieldset key={m.key} className="mt-3 rounded-lg border border-border/60 px-4 py-2">
                <legend className="px-1 text-sm font-medium">
                  {m.name || m.key}
                  {m.current ? (
                    <span className="ml-2 text-xs text-primary">({t("settings.monitorCurrent")})</span>
                  ) : null}
                </legend>
                <p className="text-xs text-muted-foreground">
                  {t("settings.monitorFacts", {
                    refresh: m.refreshHz.toFixed(m.refreshHz % 1 === 0 ? 0 : 3),
                    peak: Math.round(m.peakNits),
                    white: Math.round(m.sdrWhiteNits),
                    hdr: m.hdrSupported
                      ? m.hdrEnabled
                        ? t("settings.monitorHdrOn")
                        : t("settings.monitorHdrOff")
                      : t("settings.monitorNoHdr"),
                  })}
                </p>
                <Toggle
                  label={t("settings.vrr")}
                  hint={t("settings.vrrHint")}
                  checked={prefs.vrr}
                  onChange={(vrr) => {
                    setMonitor(m, { vrr });
                  }}
                />
                <Choice
                  label={t("settings.refresh")}
                  value={prefs.refresh}
                  onChange={(refresh) => {
                    setMonitor(m, { refresh });
                  }}
                  options={[
                    { value: "default", label: t("settings.refreshDefault") },
                    { value: "auto", label: t("settings.refreshAuto") },
                    { value: "match", label: t("settings.refreshMatch") },
                    { value: "never", label: t("settings.refreshNever") },
                  ]}
                />
                {m.hdrSupported ? (
                  <>
                    <NumberField
                      label={t("settings.peakNits")}
                      hint={t("settings.overrideHint", { value: Math.round(m.peakNits) })}
                      value={prefs.peakNits}
                      min={80}
                      max={10000}
                      step={10}
                      onCommit={(peakNits) => {
                        setMonitor(m, { peakNits });
                      }}
                    />
                    <NumberField
                      label={t("settings.blackNits")}
                      hint={t("settings.overrideHint", { value: m.blackNits.toFixed(4) })}
                      value={prefs.blackNits}
                      min={0}
                      max={5}
                      step={0.0001}
                      onCommit={(blackNits) => {
                        setMonitor(m, { blackNits });
                      }}
                    />
                  </>
                ) : null}
              </fieldset>
            );
          })
        )}
      </Section>

      <Section id="s-audio" title={t("settings.audio")}>
        <p className="text-xs text-muted-foreground">{t("settings.audioLiveHint")}</p>
        <Choice
          label={t("settings.audioDevice")}
          value={s.audio.device}
          onChange={(device) => {
            set((d) => ({ ...d, audio: { ...d.audio, device } }));
          }}
          options={deviceOptions}
        />
        <Choice
          label={t("settings.channels")}
          value={s.audio.channels}
          onChange={(channels) => {
            set((d) => ({ ...d, audio: { ...d.audio, channels } }));
          }}
          options={[
            { value: "auto", label: t("settings.channelsAuto") },
            { value: "stereo", label: t("settings.channelsStereo") },
            { value: "5.1", label: "5.1" },
            { value: "7.1", label: "7.1" },
          ]}
        />
        <Toggle
          label={t("settings.normalizeDownmix")}
          hint={t("settings.normalizeDownmixHint")}
          checked={s.audio.normalizeDownmix}
          onChange={(normalizeDownmix) => {
            set((d) => ({ ...d, audio: { ...d.audio, normalizeDownmix } }));
          }}
        />
        <Toggle
          label={t("settings.passthrough")}
          hint={t("settings.passthroughHint")}
          checked={s.audio.passthrough}
          onChange={(passthrough) => {
            set((d) => ({ ...d, audio: { ...d.audio, passthrough } }));
          }}
        />
        {s.audio.passthrough ? (
          <fieldset className="flex flex-wrap gap-4 py-2">
            <legend className="sr-only">{t("settings.passthroughCodecs")}</legend>
            {CODECS.map((c) => (
              <label key={c} className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={s.audio.passthroughCodecs.includes(c)}
                  onChange={(e) => {
                    const on = e.target.checked;
                    set((d) => ({
                      ...d,
                      audio: {
                        ...d.audio,
                        passthroughCodecs: on
                          ? [...d.audio.passthroughCodecs, c]
                          : d.audio.passthroughCodecs.filter((x) => x !== c),
                      },
                    }));
                  }}
                />
                {c.toUpperCase()}
              </label>
            ))}
          </fieldset>
        ) : null}
        <Toggle
          label={t("settings.exclusive")}
          hint={t("settings.exclusiveHint")}
          checked={s.audio.exclusive}
          onChange={(exclusive) => {
            set((d) => ({ ...d, audio: { ...d.audio, exclusive } }));
          }}
        />
      </Section>

      <Section id="s-subtitles" title={t("settings.subtitles")}>
        <RowLabel label={t("settings.subScale")}>
          <input
            type="range"
            min={0.5}
            max={2}
            step={0.1}
            aria-label={t("settings.subScale")}
            value={s.subtitles.scale}
            onChange={(e) => {
              const scale = Number(e.target.value);
              set((d) => ({ ...d, subtitles: { ...d.subtitles, scale } }));
            }}
            className="w-48 accent-primary"
          />
          <span className="w-12 text-right text-sm tabular-nums">{s.subtitles.scale.toFixed(1)}×</span>
        </RowLabel>
        <Toggle
          label={t("settings.preferSdh")}
          checked={s.subtitles.preferSdh}
          onChange={(preferSdh) => {
            set((d) => ({ ...d, subtitles: { ...d.subtitles, preferSdh } }));
          }}
        />
      </Section>

      <p className="text-xs text-muted-foreground">{t("settings.about")}</p>
    </div>
  );
}
