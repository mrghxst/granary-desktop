import { ArrowDown, ArrowUp } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { catalog } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { DEFAULT_PREVIEW_SOURCES, seekr, type PreviewSourceId, type PreviewSourceSetting } from "../lib/previews";

const LABELS: Record<PreviewSourceId, { label: string; hint: string }> = {
  server: { label: "settings.previewSourceServer", hint: "settings.previewSourceServerHint" },
  seekr: { label: "settings.previewSourceSeekr", hint: "settings.previewSourceSeekrHint" },
  local: { label: "settings.previewSourceLocal", hint: "settings.previewSourceLocalHint" },
};

const iconButton =
  "grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-surface-hover hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-30 disabled:hover:bg-transparent";

/** Reorder / enable the seek-preview providers (ADR-0011) and manage the Seekr key. */
export function PreviewSources({
  sources,
  disabled,
  onChange,
}: {
  sources: PreviewSourceSetting[];
  disabled: boolean;
  onChange: (next: PreviewSourceSetting[]) => void;
}) {
  const { t } = useTranslation();
  const list = sources.length > 0 ? sources : DEFAULT_PREVIEW_SOURCES;
  const move = (i: number, by: -1 | 1) => {
    const next = [...list];
    const [it] = next.splice(i, 1);
    if (!it) return;
    next.splice(i + by, 0, it);
    onChange(next);
  };
  return (
    <div className={`border-t border-border/60 py-3 ${disabled ? "opacity-50" : ""}`}>
      <p className="mb-2 text-sm font-medium">{t("settings.previewSources")}</p>
      <ol className="flex flex-col gap-2" aria-label={t("settings.previewSources")}>
        {list.map((s, i) => {
          const l = LABELS[s.id];
          return (
            <li key={s.id} className="rounded-lg border border-border/60 bg-surface-raised/40 px-3 py-2">
              <div className="flex items-center gap-3">
                <span className="w-5 text-center text-xs text-muted-foreground tabular-nums">{i + 1}</span>
                <div className="flex min-w-0 flex-1 items-start gap-3">
                  <input
                    id={`preview-source-${s.id}`}
                    type="checkbox"
                    className="mt-1 size-4 accent-[var(--color-primary)]"
                    checked={s.enabled}
                    disabled={disabled}
                    aria-describedby={`preview-source-${s.id}-hint`}
                    onChange={(e) => {
                      onChange(list.map((x) => (x.id === s.id ? { ...x, enabled: e.target.checked } : x)));
                    }}
                  />
                  <span className="min-w-0">
                    <label htmlFor={`preview-source-${s.id}`} className="block cursor-pointer text-sm">
                      {t(l.label)}
                    </label>
                    <span id={`preview-source-${s.id}-hint`} className="block text-xs text-muted-foreground">
                      {t(l.hint)}
                    </span>
                  </span>
                </div>
                <button
                  type="button"
                  className={iconButton}
                  aria-label={`${t(l.label)}: ${t("settings.moveUp")}`}
                  disabled={disabled || i === 0}
                  onClick={() => {
                    move(i, -1);
                  }}
                >
                  <ArrowUp className="size-4" aria-hidden />
                </button>
                <button
                  type="button"
                  className={iconButton}
                  aria-label={`${t(l.label)}: ${t("settings.moveDown")}`}
                  disabled={disabled || i === list.length - 1}
                  onClick={() => {
                    move(i, 1);
                  }}
                >
                  <ArrowDown className="size-4" aria-hidden />
                </button>
              </div>
              {s.id === "seekr" ? <SeekrKey /> : null}
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/** Seekr key field: the key goes to the host once (validated, then stored in Credential Manager) and is never shown. */
function SeekrKey() {
  const { t } = useTranslation();
  const [saved, setSaved] = useState<boolean | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    seekr
      .keyStatus()
      .then(setSaved)
      .catch(() => {
        setSaved(false);
      });
  }, []);
  const submit = async (key: string | null) => {
    setBusy(true);
    setError(null);
    try {
      const ok = await seekr.setKey(key);
      if (key !== null && !ok) setError(t("settings.seekrKeyInvalid"));
      else setDraft("");
      setSaved(key === null ? false : ok || saved === true);
    } catch (e) {
      setError(errorMessage(t, e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mt-2 ml-8 flex flex-col gap-1">
      <form
        className="flex flex-wrap items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (draft.trim()) void submit(draft.trim());
        }}
      >
        <input
          type="password"
          autoComplete="off"
          spellCheck={false}
          aria-label={t("settings.seekrKey")}
          placeholder={t("settings.seekrKeyPlaceholder")}
          value={draft}
          onChange={(e) => {
            setDraft(e.target.value);
          }}
          className="min-w-0 flex-1 rounded-md border border-border bg-background px-2 py-1 text-sm"
        />
        <button
          type="submit"
          disabled={busy || !draft.trim()}
          className="rounded-md bg-secondary px-3 py-1 text-sm font-medium hover:bg-surface-hover disabled:opacity-50"
        >
          {t("settings.save")}
        </button>
        {saved ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void submit(null)}
            className="rounded-md px-3 py-1 text-sm text-muted-foreground hover:bg-surface-hover disabled:opacity-50"
          >
            {t("settings.remove")}
          </button>
        ) : null}
      </form>
      <p className="text-xs text-muted-foreground">
        {saved === null ? "" : saved ? t("settings.seekrKeySaved") : t("settings.seekrKeyNone")} ·{" "}
        {t("settings.seekrKeyHint")}
      </p>
      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function formatBytes(n: number): string {
  if (n < 1024 * 1024) return `${String(Math.round(n / 1024))} KB`;
  return `${String(Math.round(n / (1024 * 1024)))} MB`;
}

/** Artwork cache size, limit and a Clear button (ADR-0010). */
export function ArtworkCache() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<[number, number] | null>(null);
  const refresh = () => {
    catalog
      .artworkCacheInfo()
      .then(setInfo)
      .catch(() => {
        setInfo(null);
      });
  };
  useEffect(refresh, []);
  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border/60 py-3">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium">{t("settings.artworkCache")}</p>
        <p className="text-xs text-muted-foreground">
          {t("settings.artworkCacheHint", { limit: formatBytes(info?.[1] ?? 300 * 1024 * 1024) })}
          {info ? ` ${t("settings.artworkCacheUsed", { used: formatBytes(info[0]) })}` : ""}
        </p>
      </div>
      <button
        type="button"
        onClick={() => {
          void catalog.artworkCacheClear().finally(refresh);
        }}
        className="rounded-lg bg-secondary px-4 py-2 text-sm font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
      >
        {t("settings.clearCache")}
      </button>
    </div>
  );
}
