import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router";
import { Alert, Button, Card } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { appInfo, signalReady, type AppInfo } from "../lib/host";
import { goResume, startup } from "../lib/nav";
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
  // Removing a server also deletes its stored session, so it takes a second, explicit click.
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  const reload = useCallback(() => {
    silo
      .serversList()
      .then((list) => {
        // Once per run: straight back into the server and profile used last.
        const last = startup.opened ? undefined : list.find((s) => s.active && s.account);
        startup.opened = true;
        if (last) goResume(navigate, last.serverId);
        setServers(list);
      })
      .catch((e: unknown) => {
        setServers([]);
        setError(errorMessage(t, e));
      });
  }, [t, navigate]);

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

  // The first interactive screen once the server list is known (cold-start measurement, docs/perf.md).
  useEffect(() => {
    if (servers !== null) signalReady();
  }, [servers]);

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
                  <Button
                    onClick={() => {
                      // The server opened last is the one the next start returns to.
                      if (s.account) void silo.serversActivate(s.serverId).catch(() => undefined);
                      void navigate(serverPath(s));
                    }}
                  >
                    {s.account ? t("home.open") : t("home.signIn")}
                  </Button>
                  {s.account ? (
                    <Button variant="ghost" onClick={() => void act(() => silo.authLogout(s.serverId))}>
                      {t("home.signOut")}
                    </Button>
                  ) : null}
                  {confirmRemove === s.serverId ? (
                    <>
                      <Button
                        variant="danger"
                        onClick={() => {
                          setConfirmRemove(null);
                          void act(() => silo.serversRemove(s.serverId));
                        }}
                      >
                        {t("home.confirmRemove")}
                      </Button>
                      <Button
                        variant="ghost"
                        // The confirmation replaces the focused Remove button: keep keyboard focus on the safe choice.
                        autoFocus
                        onClick={() => {
                          setConfirmRemove(null);
                          // Back on the Remove button that comes back in its place (phase-8 review L1).
                          requestAnimationFrame(() => {
                            document.querySelector<HTMLButtonElement>(`[data-remove="${s.serverId}"]`)?.focus();
                          });
                        }}
                      >
                        {t("home.cancel")}
                      </Button>
                    </>
                  ) : (
                    <Button
                      variant="danger"
                      data-remove={s.serverId}
                      onClick={() => {
                        setConfirmRemove(s.serverId);
                      }}
                    >
                      {t("home.remove")}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
            <Button variant="secondary" onClick={() => void navigate("/servers/add")}>
              {t("home.addServer")}
            </Button>
          </>
        ) : (
          <>
            {servers === null ? (
              <p className="text-muted-foreground" role="status">
                {t("status.loading")}
              </p>
            ) : (
              <section aria-labelledby="onboarding-title" className="flex flex-col gap-3" data-testid="onboarding">
                <h2 id="onboarding-title" className="text-lg font-semibold">
                  {t("home.onboarding.title")}
                </h2>
                <ol className="flex list-decimal flex-col gap-2 pl-5 text-sm text-muted-foreground">
                  <li>{t("home.onboarding.step1")}</li>
                  <li>{t("home.onboarding.step2")}</li>
                  <li>{t("home.onboarding.step3")}</li>
                </ol>
                <p className="text-xs text-muted-foreground">{t("home.onboarding.privacy")}</p>
              </section>
            )}
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
      <Link
        to="/about"
        className="text-sm font-medium text-muted-foreground hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
      >
        {t("shell.about")}
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
