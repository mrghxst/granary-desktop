import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { appInfo, type AppInfo } from "../lib/host";

export function Home() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    let alive = true;
    appInfo()
      .then((i) => {
        if (alive) setInfo(i);
      })
      .catch((e: unknown) => {
        if (alive) setInfo({ version: "?", libmpv: { ok: false, error: String(e) } });
      });
    return () => {
      alive = false;
    };
  }, []);

  return (
    <main className="flex h-full flex-col items-center justify-center gap-6 p-8">
      <h1 className="text-4xl font-semibold tracking-tight">{t("home.title")}</h1>
      <p className="text-muted-foreground">{t("app.tagline")}</p>
      <section className="w-full max-w-md rounded-lg border border-border bg-card p-6">
        <p className="mb-4 text-muted-foreground">{t("home.notConnected")}</p>
        <button
          type="button"
          disabled
          className="w-full rounded-lg bg-primary px-4 py-2 font-medium text-primary-foreground disabled:opacity-60"
        >
          {t("home.connect")}
        </button>
      </section>
      <footer className="text-sm text-muted-foreground" data-testid="status">
        {info === null
          ? t("status.loading")
          : [
              t("status.version", { version: info.version }),
              info.libmpv.ok
                ? t("status.playerReady", { version: info.libmpv.apiVersion })
                : t("status.playerMissing", { error: info.libmpv.error }),
            ].join(" · ")}
      </footer>
    </main>
  );
}
