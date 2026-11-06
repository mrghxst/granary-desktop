import { useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { Alert, Button, Card, Field, Page } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { silo, type ProbeView } from "../lib/silo";

export function AddServer() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [url, setUrl] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [probe, setProbe] = useState<ProbeView | null>(null);

  async function check(e: SyntheticEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setProbe(null);
    try {
      setProbe(await silo.serversProbe(url));
    } catch (err) {
      setError(errorMessage(t, err));
    } finally {
      setBusy(false);
    }
  }

  async function add() {
    if (!probe) return;
    setBusy(true);
    setError(null);
    try {
      const server = await silo.serversAdd(probe.origin);
      void navigate(`/servers/${encodeURIComponent(server.serverId)}/login`);
    } catch (err) {
      setError(errorMessage(t, err));
      setBusy(false);
    }
  }

  return (
    <Page title={t("addServer.title")}>
      <Card>
        <form className="flex flex-col gap-4" onSubmit={(e) => void check(e)}>
          <Field
            label={t("addServer.label")}
            hint={t("addServer.hint")}
            placeholder={t("addServer.placeholder")}
            value={url}
            onChange={(e) => {
              setUrl(e.target.value);
              setProbe(null);
            }}
            autoFocus
            spellCheck={false}
            autoCapitalize="off"
            inputMode="url"
          />
          {error ? <Alert>{error}</Alert> : null}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={() => void navigate("/")}>
              {t("addServer.cancel")}
            </Button>
            <Button type="submit" variant={probe ? "secondary" : "primary"} disabled={busy || url.trim() === ""}>
              {busy && !probe ? t("addServer.checking") : t("addServer.check")}
            </Button>
          </div>
        </form>
      </Card>
      {probe ? (
        <Card className="flex flex-col gap-3">
          <div>
            <p className="font-medium">{t("addServer.found")}</p>
            <p className="text-sm text-muted-foreground">{probe.origin}</p>
            <p className="text-sm text-muted-foreground">
              {t("addServer.version", { version: probe.serverVersion, api: probe.apiMajor })}
            </p>
          </div>
          {probe.insecure ? <Alert tone="warning">{t("addServer.warnInsecure")}</Alert> : null}
          {probe.untested ? <Alert tone="warning">{t("addServer.warnUntested")}</Alert> : null}
          {probe.reportedOrigin ? (
            <Alert tone="info">{t("addServer.warnOrigin", { origin: probe.reportedOrigin })}</Alert>
          ) : null}
          {probe.known ? <Alert tone="info">{t("addServer.known")}</Alert> : null}
          <div className="flex justify-end">
            <Button onClick={() => void add()} disabled={busy}>
              {probe.known ? t("addServer.update") : t("addServer.add")}
            </Button>
          </div>
        </Card>
      ) : null}
    </Page>
  );
}
