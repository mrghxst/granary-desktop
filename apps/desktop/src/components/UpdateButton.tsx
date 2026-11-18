import { ArrowDownToLine, RotateCw, Sparkles, X } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { appInfo, openLink } from "../lib/host";
import { downloadShare, takeUpdatedFrom, updates, useUpdateStatus } from "../lib/update";

/**
 * The sidebar's update entry (ADR-0015): a quiet progress line while a new version downloads in the background, then
 * one prominent "Restart to update" button. Nothing at all while there is no update.
 */
export function UpdateButton() {
  const { t } = useTranslation();
  const status = useUpdateStatus();
  const [error, setError] = useState(false);

  if (status.phase === "downloading") {
    const share = downloadShare(status);
    return (
      <div
        className="flex flex-col gap-1.5 rounded-lg px-3 py-2 text-[0.75rem] text-muted-foreground"
        role="status"
        title={t("update.downloading", { version: status.version })}
      >
        <span className="flex items-center gap-3">
          <ArrowDownToLine className="size-[18px] shrink-0" />
          <span className="hidden truncate lg:inline">
            {t("update.downloading", { version: status.version })}
            {share !== null ? ` · ${String(Math.round(share * 100))}%` : ""}
          </span>
        </span>
        <span className="hidden h-[3px] overflow-hidden rounded-full bg-surface-hover lg:block">
          <span
            className="block h-full rounded-full bg-primary transition-[width]"
            style={{ width: `${String(Math.round((share ?? 0.05) * 100))}%` }}
          />
        </span>
      </div>
    );
  }

  if (status.phase !== "ready" && status.phase !== "installing") return null;
  const installing = status.phase === "installing";
  return (
    <button
      type="button"
      disabled={installing}
      title={t("update.restartHint", { version: status.version })}
      aria-label={t("update.restart")}
      onClick={() => {
        setError(false);
        updates.install().catch(() => {
          setError(true);
        });
      }}
      className="group relative flex items-center gap-3 rounded-lg bg-primary px-3 py-2.5 text-left text-[0.8125rem] font-semibold text-primary-foreground shadow-[0_8px_24px_-12px_var(--color-primary)] transition hover:brightness-110 focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-70"
    >
      <RotateCw className={`size-[18px] shrink-0 ${installing ? "animate-spin" : ""}`} />
      <span className="hidden min-w-0 flex-col lg:flex">
        <span className="truncate">{installing ? t("update.restarting") : t("update.restart")}</span>
        <span className="truncate text-[0.6875rem] font-medium opacity-80">
          {error ? t("update.failedShort") : t("update.version", { version: status.version })}
        </span>
      </span>
      <span className="absolute top-1.5 left-7 size-2 rounded-full bg-primary-foreground lg:hidden" aria-hidden />
    </button>
  );
}

/** "Granary was updated", once after an update, with a link to the release notes. */
export function UpdatedNotice() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void appInfo()
      .then((a) => {
        if (alive && takeUpdatedFrom(a.version)) setVersion(a.version);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  if (!version) return null;
  return (
    <div
      role="status"
      className="fixed right-6 bottom-6 z-50 flex max-w-sm items-start gap-3 rounded-2xl border border-border bg-card p-4 shadow-2xl"
    >
      <Sparkles className="mt-0.5 size-5 shrink-0 text-primary" />
      <div className="flex min-w-0 flex-col gap-1">
        <p className="text-sm font-semibold">{t("update.updated", { version })}</p>
        <button
          type="button"
          className="self-start text-sm text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
          onClick={() => {
            void openLink("releases").catch(() => undefined);
          }}
        >
          {t("update.whatsNew")}
        </button>
      </div>
      <button
        type="button"
        aria-label={t("update.dismiss")}
        className="rounded-full p-1 text-muted-foreground hover:bg-surface-hover hover:text-foreground"
        onClick={() => {
          setVersion(null);
        }}
      >
        <X className="size-4" />
      </button>
    </div>
  );
}
