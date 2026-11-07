import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Alert, Button, Card } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { aboutInfo, appInfo, exportLogs, openLink, tailLogs, type AboutInfo, type AppInfo } from "../lib/host";

/** About: version, licence and source offer (AGPL), update check (ADR-0005), third-party notices and the log viewer. */
export function About() {
  const { t } = useTranslation();
  const [about, setAbout] = useState<AboutInfo | null>(null);
  const [app, setApp] = useState<AppInfo | null>(null);
  const [lines, setLines] = useState<string[] | null>(null);
  const [status, setStatus] = useState<{ tone: "info" | "error"; text: string } | null>(null);

  useEffect(() => {
    let alive = true;
    void Promise.all([aboutInfo(), appInfo()])
      .then(([a, i]) => {
        if (!alive) return;
        setAbout(a);
        setApp(i);
      })
      .catch((e: unknown) => {
        if (alive) setStatus({ tone: "error", text: errorMessage(t, e) });
      });
    return () => {
      alive = false;
    };
  }, [t]);

  const refreshLogs = useCallback(() => {
    tailLogs(500)
      .then(setLines)
      .catch((e: unknown) => {
        setStatus({ tone: "error", text: errorMessage(t, e) });
      });
  }, [t]);

  const open = (link: Parameters<typeof openLink>[0]) => {
    openLink(link).catch((e: unknown) => {
      setStatus({ tone: "error", text: errorMessage(t, e) });
    });
  };

  return (
    <div className="flex max-w-3xl flex-col gap-6" data-testid="about">
      <h1 className="text-3xl font-semibold tracking-tight">{t("about.title")}</h1>
      {status ? <Alert tone={status.tone}>{status.text}</Alert> : null}

      <Card className="flex flex-col gap-3">
        <h2 className="text-lg font-semibold">Granary</h2>
        {about === null ? (
          <p className="text-sm text-muted-foreground" role="status">
            {t("status.loading")}
          </p>
        ) : (
          <dl className="grid grid-cols-[max-content_1fr] gap-x-6 gap-y-1 text-sm">
            <dt className="text-muted-foreground">{t("about.version")}</dt>
            <dd data-testid="about-version">
              {about.version}
              {about.commit !== "unknown" ? (
                <span className="ml-2 font-mono text-xs text-muted-foreground">
                  {about.commit.slice(0, 12)}
                  {about.commit.endsWith("-dirty") ? "-dirty" : ""}
                </span>
              ) : null}
            </dd>
            <dt className="text-muted-foreground">{t("about.player")}</dt>
            <dd>
              {app?.libmpv.ok
                ? t("status.playerReady", { version: app.libmpv.apiVersion })
                : t("status.playerMissing", { error: app?.libmpv.ok === false ? app.libmpv.error : "?" })}
            </dd>
            <dt className="text-muted-foreground">{t("about.license")}</dt>
            <dd>{about.license}</dd>
          </dl>
        )}
        <p className="text-sm text-muted-foreground">{t("about.compatible")}</p>
        <p className="text-sm text-muted-foreground">{t("about.sourceOffer")}</p>
        <div className="flex flex-wrap gap-2">
          <Button
            onClick={() => {
              open("releases");
            }}
          >
            {t("about.checkUpdates")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => {
              open("source");
            }}
          >
            {t("about.source")}
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              open("license");
            }}
          >
            {t("about.licenseText")}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">{t("about.updatesHint")}</p>
      </Card>

      <Card className="flex flex-col gap-3">
        <h2 className="text-lg font-semibold">{t("about.logs")}</h2>
        <p className="text-sm text-muted-foreground">{t("about.logsHint")}</p>
        <div className="flex flex-wrap gap-2">
          <Button variant="secondary" onClick={refreshLogs}>
            {lines === null ? t("about.showLogs") : t("about.refreshLogs")}
          </Button>
          <Button
            variant="secondary"
            onClick={() => {
              exportLogs()
                .then((path) => {
                  setStatus({ tone: "info", text: t("about.exported", { path }) });
                })
                .catch((e: unknown) => {
                  setStatus({ tone: "error", text: errorMessage(t, e) });
                });
            }}
          >
            {t("about.exportLogs")}
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              open("log_folder");
            }}
          >
            {t("about.openLogFolder")}
          </Button>
        </div>
        {lines !== null ? (
          lines.length === 0 ? (
            <p className="text-sm text-muted-foreground">{t("about.noLogs")}</p>
          ) : (
            // A scrollable region must be reachable by keyboard to scroll it (WCAG 2.1.1).
            <pre
              className="max-h-96 overflow-auto rounded-lg border border-border bg-background p-3 font-mono text-xs leading-relaxed"
              role="region"
              aria-label={t("about.logs")}
              // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
              tabIndex={0}
              data-testid="log-view"
            >
              {lines.join("\n")}
            </pre>
          )
        ) : null}
      </Card>

      {about ? (
        <Card className="flex flex-col gap-3">
          <details>
            <summary className="cursor-pointer text-lg font-semibold">{t("about.notices")}</summary>
            <pre className="mt-3 max-h-96 overflow-auto font-mono text-xs leading-relaxed whitespace-pre-wrap">
              {about.notices}
            </pre>
          </details>
        </Card>
      ) : null}
    </div>
  );
}
