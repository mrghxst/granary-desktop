import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { PosterCard } from "../components/PosterCard";
import { Alert, Button } from "../components/ui";
import { catalog, type CatalogItem, type CatalogQuery } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { goReauth, needsReauth } from "../lib/nav";
import { silo } from "../lib/silo";
import { retryLoads } from "../lib/events";
import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";

/** Sort fields from the catalog filters document (docs/research/silo-api.md §4.2). */
const SORTS = ["title", "-added_at", "-release_date", "-rating", "random"] as const;
const PAGE = 60;

/** A grid of catalog items, used by the library page and by search. */
export function ItemGrid({
  serverId,
  query,
  emptyText,
}: {
  serverId: string;
  query: CatalogQuery | null;
  emptyText: string;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const key = query ? JSON.stringify(query) : null;
  const first = useLoad(key, () =>
    query ? catalog.browse(serverId, { ...query, limit: PAGE }) : Promise.reject(new Error("no query")),
  );
  // Further pages for the current key; a new key ignores pages loaded for an older one.
  const [more, setMore] = useState<{ key: string | null; items: CatalogItem[]; cursor: string | null }>({
    key: null,
    items: [],
    cursor: null,
  });
  const [moreError, setMoreError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const { play, error: playError } = usePlay(serverId);

  useEffect(() => {
    if (needsReauth(first.error)) goReauth(navigate, serverId, first.error);
  }, [first.error, navigate, serverId]);

  const extra = more.key === key ? more : null;
  const items = first.data ? [...first.data.items, ...(extra?.items ?? [])] : undefined;
  const firstCursor = first.data?.page?.has_more ? (first.data.page.next_cursor ?? null) : null;
  const cursor = extra ? extra.cursor : firstCursor;
  const total = first.data?.total ?? 0;
  const failure = first.error ?? moreError;
  const error = failure && !needsReauth(failure) ? errorMessage(t, failure) : null;

  const loadMore = async () => {
    if (!query || !cursor) return;
    setLoading(true);
    try {
      const page = await catalog.browse(serverId, { ...query, limit: PAGE, cursor });
      setMore({
        key,
        items: [...(extra?.items ?? []), ...page.items],
        cursor: page.page?.has_more ? (page.page.next_cursor ?? null) : null,
      });
      setMoreError(null);
    } catch (e) {
      if (needsReauth(e)) goReauth(navigate, serverId, e);
      else setMoreError(e);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="flex flex-col gap-6">
      {error ? (
        <Alert
          onRetry={
            first.error
              ? retryLoads
              : () => {
                  void loadMore();
                }
          }
        >
          {error}
        </Alert>
      ) : null}
      {playError ? <Alert>{playError}</Alert> : null}
      {items === undefined && query && !error ? <p className="text-muted-foreground">{t("status.loading")}</p> : null}
      {items?.length === 0 ? <p className="text-muted-foreground">{emptyText}</p> : null}
      {items && items.length > 0 ? (
        <>
          <p className="text-sm text-muted-foreground">{t("browse.count", { count: total })}</p>
          <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
            {items.map((item) => (
              <PosterCard
                key={item.content_id}
                serverId={serverId}
                item={item}
                onPlay={(i) => void play(i.play_content_id ?? i.content_id)}
              />
            ))}
          </div>
        </>
      ) : null}
      {cursor && query ? (
        <div className="flex justify-center">
          <Button variant="secondary" disabled={loading} onClick={() => void loadMore()}>
            {loading ? t("status.loading") : t("browse.more")}
          </Button>
        </div>
      ) : null}
    </div>
  );
}

export function Library() {
  const { t } = useTranslation();
  const { serverId = "", libraryId = "" } = useParams();
  const [sort, setSort] = useState<(typeof SORTS)[number]>("title");
  const [name, setName] = useState("");
  const query = useMemo<CatalogQuery>(() => ({ libraryId, sort }), [libraryId, sort]);
  useEffect(() => {
    let alive = true;
    void silo
      .librariesList(serverId)
      .then((libs) => {
        if (alive) setName(libs.find((l) => l.id === libraryId)?.name ?? "");
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [serverId, libraryId]);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <h1 className="text-3xl font-semibold tracking-tight">{name || t("libraries.title")}</h1>
        <label className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground">{t("browse.sort")}</span>
          <select
            value={sort}
            onChange={(e) => {
              setSort(e.target.value as (typeof SORTS)[number]);
            }}
            className="rounded-lg border border-border bg-background px-3 py-2 text-sm focus:border-ring focus:outline-none"
          >
            {SORTS.map((s) => (
              <option key={s} value={s}>
                {t(`browse.sorts.${s.replace("-", "")}`)}
              </option>
            ))}
          </select>
        </label>
      </div>
      <ItemGrid serverId={serverId} query={query} emptyText={t("browse.emptyLibrary")} />
    </div>
  );
}
