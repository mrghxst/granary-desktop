import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router";
import { Alert, Button, Card } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { appInfo, type AppInfo } from "../lib/host";
import { silo, type ServerView } from "../lib/silo";

function serverPath(s: ServerView): string {
  const base = `/servers/${encodeURIComponent(s.serverId)}`;
  return s.account ? `${base}/home` : `${base}/login`;
}

export function Home() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [servers, setServers] = useState<ServerView[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const reload = useCallback(() => {
    silo
      .serversList()
      .then(setServers)
      .catch((e: unknown) => {
        setServers([]);
        setError(errorMessage(t, e));
      });
  }, [t]);

  useEffect(() => {
    let alive = true;
    appInfo()
      .then((i) => {
        if (alive) setInfo(i);
      })
      .catch((e: unknown) => {
        if (alive) setInfo({ version: "?", libmpv: { ok: false, error: String(e) } });
      });
    reload();
    return () => {
      alive = false;
    };
  }, [reload]);

  async function act(f: () => Promise<unknown>) {
    setError(null);
    try {
      await f();
    } catch (e) {
      setError(errorMessage(t, e));
    }
    reload();
  }

  return (
    <main className="flex h-full flex-col items-center justify-center gap-6 overflow-y-auto p-8">
      <h1 className="text-4xl font-semibold tracking-tight">{t("home.title")}</h1>
      <Card className="flex w-full max-w-xl flex-col gap-4">
        {servers && servers.length > 0 ? (
          <>
            <h2 className="text-lg font-semibold">{t("home.servers")}</h2>
            <ul className="flex flex-col gap-3">
              {servers.map((s) => (
                <li
                  key={s.serverId}
                  className="flex flex-wrap items-center gap-3 rounded-lg border border-border bg-background p-3"
                >
                  <div className="min-w-0 flex-1">
                    <p className="truncate font-medium">{s.origin}</p>
                    <p className="text-xs text-muted-foreground">
                      {s.account ? t("home.signedInAs", { username: s.account.username }) : t("home.signedOut")}
                      {s.untested ? ` · ${t("home.untested")}` : ""}
                      {s.insecure ? ` · ${t("home.insecure")}` : ""}
                    </p>
                  </div>
                  <Button onClick={() => void navigate(serverPath(s))}>
                    {s.account ? t("home.open") : t("home.signIn")}
                  </Button>
                  {s.account ? (
                    <Button variant="ghost" onClick={() => void act(() => silo.authLogout(s.serverId))}>
                      {t("home.signOut")}
                    </Button>
                  ) : null}
                  <Button variant="danger" onClick={() => void act(() => silo.serversRemove(s.serverId))}>
                    {t("home.remove")}
                  </Button>
                </li>
              ))}
            </ul>
            <Button variant="secondary" onClick={() => void navigate("/servers/add")}>
              {t("home.addServer")}
            </Button>
          </>
        ) : (
          <>
            <p className="text-muted-foreground">{t("home.notConnected")}</p>
            <Button className="w-full" disabled={servers === null} onClick={() => void navigate("/servers/add")}>
              {t("home.connect")}
            </Button>
          </>
        )}
        {error ? <Alert>{error}</Alert> : null}
      </Card>
      <Link
        to="/settings"
        className="text-sm font-medium text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
      >
        {t("shell.settings")}
      </Link>
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
