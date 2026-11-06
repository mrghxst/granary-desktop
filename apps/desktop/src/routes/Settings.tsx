import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import { Alert } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { useSettings, type Settings as SettingsModel } from "../lib/settings";

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

const CODECS = ["ac3", "eac3", "dts", "dts-hd", "truehd"] as const;

export function Settings() {
  const { t } = useTranslation();
  const { settings: s, update, error } = useSettings();
  if (!s) return <p className="text-muted-foreground">{t("status.loading")}</p>;
  const set = (patch: (draft: SettingsModel) => SettingsModel) => void update(patch(s));

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
        <Toggle
          label={t("settings.autoHdr")}
          hint={t("settings.autoHdrHint")}
          checked={s.display.autoHdr}
          onChange={(autoHdr) => {
            set((d) => ({ ...d, display: { ...d.display, autoHdr } }));
          }}
        />
        <Toggle
          label={t("settings.matchRefresh")}
          hint={t("settings.matchRefreshHint")}
          checked={s.display.matchRefreshRate}
          onChange={(matchRefreshRate) => {
            set((d) => ({ ...d, display: { ...d.display, matchRefreshRate } }));
          }}
        />
      </Section>

      <Section id="s-audio" title={t("settings.audio")}>
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
