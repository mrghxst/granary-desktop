import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { Badge } from "../components/Popover";
import { Alert } from "../components/ui";
import { artwork, catalog, type CatalogQuery } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads } from "../lib/events";
import { goReauth, needsReauth } from "../lib/nav";
import { useLoad } from "../lib/useLoad";
import { ItemGrid } from "./Library";

const TABS = [
  { key: "all", type: undefined },
  { key: "movies", type: "movie" },
  { key: "series", type: "series" },
] as const;

function age(birth: string, death: string | null | undefined): number | null {
  const b = new Date(birth);
  const end = death ? new Date(death) : new Date();
  if (Number.isNaN(b.getTime()) || Number.isNaN(end.getTime())) return null;
  let years = end.getFullYear() - b.getFullYear();
  const m = end.getMonth() - b.getMonth();
  if (m < 0 || (m === 0 && end.getDate() < b.getDate())) years -= 1;
  return years >= 0 && years < 130 ? years : null;
}

/** Person: photo, life facts, biography and their filmography on this server. */
export function Person() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "", personId = "" } = useParams();
  const { data: person, error } = useLoad(`${serverId}/person/${personId}`, () => catalog.person(serverId, personId));
  const [tab, setTab] = useState<(typeof TABS)[number]["key"]>("all");
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!person) return <div className="h-72 animate-pulse rounded-2xl bg-card" />;

  const photo = artwork(serverId, person.photo_url);
  const fmt = (d: string) => {
    const x = new Date(d);
    return Number.isNaN(x.getTime()) ? d : x.toLocaleDateString(i18n.language, { dateStyle: "long" });
  };
  const years = person.birth_date ? age(person.birth_date, person.death_date) : null;
  const type = TABS.find((x) => x.key === tab)?.type;
  const query: CatalogQuery = { source: "person", personId, sort: "-release_date", ...(type ? { type } : {}) };

  return (
    <article className="flex flex-col gap-10">
      <section className="flex flex-col gap-8 md:flex-row">
        <div className="aspect-[2/3] w-48 shrink-0 overflow-hidden rounded-2xl bg-surface-raised shadow-2xl lg:w-60">
          {photo ? (
            <img src={photo} alt="" className="size-full object-cover" />
          ) : (
            <span className="flex size-full items-center justify-center text-5xl font-semibold text-muted-foreground">
              {person.name
                .split(/\s+/)
                .map((w) => w.charAt(0))
                .slice(0, 2)
                .join("")}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-4">
          <h1 className="text-4xl font-bold tracking-tight lg:text-5xl">{person.name}</h1>
          <div className="flex flex-wrap gap-1.5">
            {person.birth_date ? <Badge strong>{t("person.born", { date: fmt(person.birth_date) })}</Badge> : null}
            {person.death_date ? <Badge strong>{t("person.died", { date: fmt(person.death_date) })}</Badge> : null}
            {years !== null ? <Badge>{t("person.age", { count: years })}</Badge> : null}
            {person.birthplace ? <Badge>{person.birthplace}</Badge> : null}
          </div>
          {person.bio ? (
            <div className="max-w-3xl">
              <p
                className={`text-sm leading-relaxed whitespace-pre-line text-foreground/85 lg:text-base ${
                  expanded ? "" : "line-clamp-6"
                }`}
              >
                {person.bio}
              </p>
              {person.bio.length > 480 ? (
                <button
                  type="button"
                  onClick={() => {
                    setExpanded((x) => !x);
                  }}
                  className="mt-2 text-sm font-medium text-muted-foreground hover:text-foreground"
                >
                  {expanded ? t("person.less") : t("person.more")}
                </button>
              ) : null}
            </div>
          ) : null}
        </div>
      </section>
      <section className="flex flex-col gap-4" aria-label={t("person.filmography")}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <h2 className="text-lg font-semibold tracking-tight">{t("person.filmography")}</h2>
          <div role="tablist" className="flex gap-1 rounded-full border border-border p-1">
            {TABS.map((x) => (
              <button
                key={x.key}
                type="button"
                role="tab"
                aria-selected={tab === x.key}
                onClick={() => {
                  setTab(x.key);
                }}
                className={`rounded-full px-3 py-1 text-sm font-medium transition ${
                  tab === x.key ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {t(`person.tabs.${x.key}`)}
              </button>
            ))}
          </div>
        </div>
        <ItemGrid serverId={serverId} query={query} emptyText={t("person.empty")} />
      </section>
    </article>
  );
}
