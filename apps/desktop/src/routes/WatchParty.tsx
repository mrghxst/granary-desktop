import { Users } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router";
import { Alert, Button, Card } from "../components/ui";
import { errorMessage } from "../lib/errors";
import { together, type TogetherCapabilities } from "../lib/together";
import { useTogether } from "../player/togetherContext";

/**
 * Join a Watch Party by its code, or see the one this profile is in. Parties start from a title's page (More → Watch
 * together); the room then plays here in sync with the others, whichever app they use.
 */
export function WatchParty() {
  const { t } = useTranslation();
  const { serverId = "" } = useParams();
  const party = useTogether();
  const [caps, setCaps] = useState<TogetherCapabilities | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    together
      .capabilities(serverId)
      .then((c) => {
        if (alive) setCaps(c);
      })
      .catch((e: unknown) => {
        if (alive) setError(errorMessage(t, e));
      });
    return () => {
      alive = false;
    };
  }, [serverId, t]);

  const join = () => {
    setBusy(true);
    setError(null);
    party
      .join(serverId, code)
      .catch((e: unknown) => {
        setError(errorMessage(t, e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const room = party.serverId === serverId ? party.room : null;
  const offered = caps !== null && caps.state === "available" && caps.allowed;

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-6 p-6">
      <header className="flex items-center gap-3">
        <Users className="size-6" />
        <h1 className="text-2xl font-semibold">{t("together.title")}</h1>
      </header>
      {error ? <Alert>{error}</Alert> : null}
      {caps !== null && !offered ? <p className="text-muted-foreground">{t("together.unavailable")}</p> : null}
      {room ? (
        <Card className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">{t("together.inRoom")}</p>
          <p className="font-mono text-3xl tracking-[0.2em]" aria-label={t("together.code")}>
            {room.code}
          </p>
          <p className="text-sm">
            {t("together.members", { count: room.member_count })} ·{" "}
            {room.self_role === "host" ? t("together.youHost") : t("together.youGuest")} ·{" "}
            {t(`together.link.${party.link}`)}
          </p>
          {room.members && room.members.length > 0 ? (
            <ul className="flex flex-wrap gap-2 text-sm">
              {room.members.map((m) => (
                <li
                  key={m.profile_id}
                  className={`rounded-full border border-border px-3 py-1 ${m.connected ? "" : "opacity-50"}`}
                >
                  {m.display_name}
                  {m.is_host ? ` · ${t("together.host")}` : ""}
                </li>
              ))}
            </ul>
          ) : null}
          {room.phase === "lobby" ? <p className="text-muted-foreground">{t("together.waitingForHost")}</p> : null}
          <div>
            <Button variant="secondary" onClick={party.leave}>
              {room.self_role === "host" ? t("together.end") : t("together.leave")}
            </Button>
          </div>
        </Card>
      ) : offered ? (
        <Card className="flex flex-col gap-3">
          <p className="text-sm text-muted-foreground">{t("together.joinHint")}</p>
          <form
            className="flex gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              if (code.trim()) join();
            }}
          >
            <input
              aria-label={t("together.code")}
              placeholder={t("together.codePlaceholder")}
              value={code}
              maxLength={64}
              onChange={(e) => {
                setCode(e.target.value.toUpperCase());
              }}
              className="h-10 flex-1 rounded-md border border-border bg-background px-3 font-mono tracking-widest focus-visible:outline-2 focus-visible:outline-ring"
            />
            <Button type="submit" disabled={busy || !code.trim()}>
              {t("together.join")}
            </Button>
          </form>
          <p className="text-xs text-muted-foreground">{t("together.startHint")}</p>
        </Card>
      ) : null}
    </div>
  );
}
