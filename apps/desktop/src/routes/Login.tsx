import { useEffect, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate, useParams } from "react-router";
import { Alert, Button, Card, Field, Page } from "../components/ui";
import { errorMessage } from "../lib/errors";
import type { LoginState } from "../lib/nav";
import { silo } from "../lib/silo";

export function Login() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const location = useLocation();
  const revoked = (location.state as LoginState | null)?.revoked === true;
  const [origin, setOrigin] = useState(serverId);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [passwordChange, setPasswordChange] = useState(false);

  useEffect(() => {
    let alive = true;
    void silo.serversList().then((servers) => {
      const s = servers.find((x) => x.serverId === serverId);
      if (alive && s) setOrigin(s.origin);
    });
    return () => {
      alive = false;
    };
  }, [serverId]);

  async function submit(e: SyntheticEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setPasswordChange(false);
    try {
      const r = await silo.authLogin(serverId, username, password);
      setPassword("");
      if (r.passwordChangeRequired) {
        // The server only allows a password change in this state; Granary has no such screen (the web app does).
        await silo.authLogout(serverId).catch(() => undefined);
        setPasswordChange(true);
        setBusy(false);
        return;
      }
      void navigate(`/servers/${encodeURIComponent(serverId)}/profiles`);
    } catch (err) {
      setError(errorMessage(t, err));
      setBusy(false);
    }
  }

  return (
    <Page title={t("login.title", { server: origin })}>
      <Card>
        <form className="flex flex-col gap-4" onSubmit={(e) => void submit(e)}>
          {revoked && !error ? <Alert tone="info">{t("login.revoked")}</Alert> : null}
          <Field
            label={t("login.username")}
            value={username}
            onChange={(e) => {
              setUsername(e.target.value);
            }}
            autoComplete="username"
            autoCapitalize="off"
            spellCheck={false}
            autoFocus
          />
          <Field
            label={t("login.password")}
            type="password"
            value={password}
            onChange={(e) => {
              setPassword(e.target.value);
            }}
            autoComplete="current-password"
          />
          {error ? <Alert>{error}</Alert> : null}
          {passwordChange ? <Alert tone="warning">{t("login.passwordChange")}</Alert> : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => void navigate("/")}>
              {t("addServer.cancel")}
            </Button>
            <Button type="submit" disabled={busy || username.trim() === "" || password === ""}>
              {busy ? t("login.submitting") : t("login.submit")}
            </Button>
          </div>
        </form>
      </Card>
    </Page>
  );
}
