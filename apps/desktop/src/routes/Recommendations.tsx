import { Sparkles } from "lucide-react";
import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { PosterCard } from "../components/PosterCard";
import { Row } from "../components/Row";
import { Alert } from "../components/ui";
import { catalog, type RecommendationRow } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, needsReauth, serverBase } from "../lib/nav";
import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6";
/** Rows built from the profile's own history; the others are the same for everyone. */
const PERSONAL = ["cluster", "similar_users_liked"];

function sectionPath(serverId: string, row: RecommendationRow): string | undefined {
  if (!row.kind) return undefined;
  const base = `${serverBase(serverId)}/recommendations/${encodeURIComponent(row.kind)}`;
  return row.key ? `${base}/${encodeURIComponent(row.key)}` : base;
}

function Empty({ title, body }: { title: string; body: string }) {
  return (
    <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-2xl bg-card px-6 py-12 text-center">
      <Sparkles className="size-10 text-muted-foreground" strokeWidth={1.5} />
      <p className="text-sm font-medium">{title}</p>
      <p className="max-w-sm text-sm text-muted-foreground">{body}</p>
    </div>
  );
}

function Chips({ label, values }: { label: string; values: string[] }) {
  if (values.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <span className="mr-1 text-[0.6875rem] font-semibold tracking-[0.14em] text-muted-foreground uppercase">
        {label}
      </span>
      {values.map((v) => (
        <span key={v} className="rounded-full border border-border px-2.5 py-0.5 text-xs font-medium">
          {v}
        </span>
      ))}
    </div>
  );
}

/**
 * "For You": the profile's recommendation rows (`getDiscover`) with its taste summary (`getTasteProfile`), as on the
 * web app's page (`silo-server/web/src/pages/Recommendations.tsx@749db96a`). A row's heading opens the row in full
 * when the server has a page for it.
 */
export function Recommendations() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(`${serverId}/recommendations`, () => catalog.discover(serverId), refresh);
  // The taste summary is an extra: the page stands without it.
  const { data: taste } = useLoad(`${serverId}/taste`, () => catalog.tasteProfile(serverId));
  const { play } = usePlay(serverId);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const personal = (data ?? []).some((r) => PERSONAL.includes(r.type));
  return (
    <div className="flex flex-col gap-10">
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-bold tracking-tight">{t("recommendations.title")}</h1>
          {data && data.length > 0 ? (
            <p className="text-sm text-muted-foreground">
              {t(personal ? "recommendations.subtitlePersonal" : "recommendations.subtitleDefault")}
            </p>
          ) : null}
        </div>
        {taste && (taste.top_genres.length > 0 || taste.favorite_directors.length > 0) ? (
          <section className="flex flex-col gap-2" aria-label={t("recommendations.taste")}>
            <Chips label={t("recommendations.genres")} values={taste.top_genres} />
            <Chips label={t("recommendations.directors")} values={taste.favorite_directors} />
          </section>
        ) : null}
      </div>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {!data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {data && data.length === 0 ? (
        <Empty title={t("recommendations.emptyTitle")} body={t("recommendations.emptyBody")} />
      ) : null}
      {(data ?? []).map((row, i) => (
        <Row key={`${row.type}-${row.key ?? ""}-${String(i)}`} title={row.title} to={sectionPath(serverId, row)}>
          {row.items.map((x) => (
            <PosterCard key={x.content_id} serverId={serverId} item={x} onPlay={(c, o) => void play(c.content_id, o)} />
          ))}
        </Row>
      ))}
    </div>
  );
}

/** One recommendation row in full (`getRecommendationSection`). */
export function RecommendationSection() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "", kind = "", key } = useParams();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(
    `${serverId}/recommendations/${kind}/${key ?? ""}`,
    () => catalog.recommendationSection(serverId, kind, key ?? null),
    refresh,
  );
  const { play } = usePlay(serverId);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const title = data?.title || t("recommendations.title");
  return (
    <section className="flex flex-col gap-6" aria-label={title}>
      <div className="flex flex-col gap-1">
        <h1 className="min-h-9 text-3xl font-bold tracking-tight">{data ? title : ""}</h1>
        {data && data.items.length > 0 ? (
          <p className="text-sm text-muted-foreground">{t("recommendations.count", { count: data.items.length })}</p>
        ) : null}
      </div>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {!data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {data && data.items.length === 0 ? (
        <Empty title={t("recommendations.sectionEmptyTitle")} body={t("recommendations.sectionEmptyBody")} />
      ) : null}
      {data && data.items.length > 0 ? (
        <div className={GRID}>
          {data.items.map((x) => (
            <PosterCard key={x.content_id} serverId={serverId} item={x} onPlay={(c, o) => void play(c.content_id, o)} />
          ))}
        </div>
      ) : null}
    </section>
  );
}
