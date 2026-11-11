import { useEffect, useState, type SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { useLocation, useNavigate, useParams } from "react-router";
import { Alert, Button, Field, Page } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { goSignIn, type ProfilesState } from "../lib/nav";
import { needsSignIn, silo, type ProfileView } from "../lib/silo";

export function Profiles() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  // Set when the app came here by itself (start, lost profile): the profile used last is picked again.
  const auto = (useLocation().state as ProfilesState | null)?.auto === true;
  const [profiles, setProfiles] = useState<ProfileView[] | null>(null);
  const [pinFor, setPinFor] = useState<ProfileView | null>(null);
  const [pin, setPin] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    const live = () => alive;
    const last = auto
      ? silo
          .serversList()
          .then((list) => list.find((s) => s.serverId === serverId)?.lastProfileId)
          .catch(() => undefined)
      : Promise.resolve(undefined);
    Promise.all([silo.profilesList(serverId), last])
      .then(async ([p, lastId]) => {
        if (!alive) return;
        const again = p.find((x) => x.id === lastId);
        if (again && !again.hasPin) {
          // No picker in between: the list shows only if selecting fails.
          try {
            await silo.profilesSelect(serverId, again.id);
            if (live()) void navigate(`/servers/${encodeURIComponent(serverId)}/home`, { replace: true });
            return;
          } catch (err) {
            if (!live()) return;
            if (needsSignIn(err)) {
              goSignIn(navigate, serverId, err);
              return;
            }
            setError(errorMessage(t, err));
          }
        }
        setProfiles(p);
        // A locked profile only needs its PIN.
        if (again?.hasPin) setPinFor(again);
      })
      .catch((err: unknown) => {
        if (!alive) return;
        if (needsSignIn(err)) {
          goSignIn(navigate, serverId, err);
        } else {
          setError(errorMessage(t, err));
        }
      });
    return () => {
      alive = false;
    };
  }, [serverId, navigate, t, auto]);

  async function choose(p: ProfileView, withPin?: string) {
    setBusy(true);
    setError(null);
    try {
      await silo.profilesSelect(serverId, p.id, withPin);
      setPin("");
      void navigate(`/servers/${encodeURIComponent(serverId)}/home`);
    } catch (err) {
      setBusy(false);
      if (needsSignIn(err)) {
        goSignIn(navigate, serverId, err);
        return;
      }
      setPin("");
      setError(errorMessage(t, err));
    }
  }

  function onPick(p: ProfileView) {
    setError(null);
    if (p.hasPin) {
      setPinFor(p);
      setPin("");
    } else {
      void choose(p);
    }
  }

  function submitPin(e: SyntheticEvent) {
    e.preventDefault();
    if (pinFor) void choose(pinFor, pin);
  }

  return (
    <Page title={t("profiles.title")}>
      <div>
        <Button
          variant="ghost"
          onClick={() => {
            void navigate("/");
          }}
        >
          <span aria-hidden="true">← </span>
          {t("libraries.back")}
        </Button>
      </div>
      {profiles === null && !error ? (
        <p className="text-muted-foreground" role="status">
          {t("status.loading")}
        </p>
      ) : null}
      {profiles ? (
        <ul className="grid grid-cols-2 gap-4 sm:grid-cols-3">
          {profiles.map((p) => (
            <li key={p.id}>
              <button
                type="button"
                disabled={busy}
                aria-pressed={pinFor?.id === p.id}
                onClick={() => {
                  onPick(p);
                }}
                className={`flex w-full flex-col items-center gap-3 rounded-lg border bg-card p-5 transition hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-60 ${pinFor?.id === p.id ? "border-ring" : "border-border"}`}
              >
                <span
                  aria-hidden="true"
                  className="flex size-16 items-center justify-center rounded-full bg-surface-raised text-2xl font-semibold"
                >
                  {p.name.slice(0, 1).toUpperCase()}
                </span>
                <span className="font-medium">{p.name}</span>
                <span className="flex gap-1 text-xs text-muted-foreground">
                  {p.isChild ? <span>{t("profiles.kids")}</span> : null}
                  {p.hasPin ? <span>{t("profiles.locked")}</span> : null}
                </span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {pinFor ? (
        <form className="flex items-end gap-2" onSubmit={submitPin}>
          <div className="flex-1">
            <Field
              label={t("profiles.pin", { name: pinFor.name })}
              type="password"
              inputMode="numeric"
              autoComplete="off"
              value={pin}
              onChange={(e) => {
                setPin(e.target.value);
              }}
              autoFocus
            />
          </div>
          <Button type="submit" disabled={busy || pin === ""}>
            {t("profiles.unlock")}
          </Button>
        </form>
      ) : null}
      {error ? <Alert>{error}</Alert> : null}
    </Page>
  );
}
