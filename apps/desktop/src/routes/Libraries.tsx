import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { Alert, Button, Page } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { goSignIn } from "../lib/nav";
import { isCommandError, needsSignIn, silo, type LibraryView } from "../lib/silo";

export function Libraries() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const [libraries, setLibraries] = useState<LibraryView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const base = `/servers/${encodeURIComponent(serverId)}`;

  useEffect(() => {
    let alive = true;
    silo
      .librariesList(serverId)
      .then((l) => {
        if (alive) setLibraries(l);
      })
      .catch((err: unknown) => {
        if (!alive) return;
        if (needsSignIn(err)) goSignIn(navigate, serverId, err);
        else if (isCommandError(err) && err.code === "profile_verification_required")
          void navigate(`${base}/profiles`, { replace: true });
        else setError(errorMessage(t, err));
      });
    return () => {
      alive = false;
    };
  }, [serverId, base, navigate, t]);

  return (
    <Page title={t("libraries.title")}>
      <div className="flex gap-2">
        <Button variant="secondary" onClick={() => void navigate("/")}>
          {t("libraries.back")}
        </Button>
        <Button variant="secondary" onClick={() => void navigate(`${base}/profiles`)}>
          {t("libraries.switchProfile")}
        </Button>
      </div>
      {error ? <Alert>{error}</Alert> : null}
      {libraries === null && !error ? <p className="text-muted-foreground">{t("status.loading")}</p> : null}
      {libraries?.length === 0 ? <p className="text-muted-foreground">{t("libraries.empty")}</p> : null}
      {libraries && libraries.length > 0 ? (
        <ul className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {libraries.map((l) => (
            <li key={l.id} className="rounded-lg border border-border bg-card p-4">
              <p className="font-medium">{l.name}</p>
              <p className="text-xs text-muted-foreground">{l.kind}</p>
            </li>
          ))}
        </ul>
      ) : null}
    </Page>
  );
}
