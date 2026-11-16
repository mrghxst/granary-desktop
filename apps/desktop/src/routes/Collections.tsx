import { useEffect, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams, useSearchParams } from "react-router";
import { PosterCard } from "../components/PosterCard";
import { Alert } from "../components/ui";
import { artwork, catalog, type CatalogQuery, type CollectionCard } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, needsReauth } from "../lib/nav";
import { usePlay } from "../lib/usePlay";
import { useLoad } from "../lib/useLoad";
import { CollectionTile, ItemGrid } from "./Library";

const GRID = "grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6";

function Group({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-4" aria-label={title}>
      <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
      <ul className={GRID}>{children}</ul>
    </section>
  );
}

/**
 * Collections: the profile's own, those other profiles of the login share with it, and the server's by library
 * (`listCollections`, `listServerCollections`). Read-only: collections are made and edited in the web app.
 */
export function Collections() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(`${serverId}/collections`, () => catalog.collections(serverId), refresh);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const personal = (c: { id: string; name: string; item_count: number; poster_url: string }) => (
    <li key={c.id}>
      <CollectionTile
        serverId={serverId}
        card={{ id: c.id, title: c.name, item_count: c.item_count, poster_url: c.poster_url }}
        onOpen={() => void navigate(`../collection/user/${encodeURIComponent(c.id)}`)}
      />
    </li>
  );
  const server = (data?.server ?? []).filter((l) => l.collections.length > 0);
  const empty = data && data.mine.length === 0 && data.shared.length === 0 && server.length === 0;

  return (
    <div className="flex flex-col gap-10">
      <div className="flex flex-col gap-1">
        <h1 className="text-3xl font-bold tracking-tight">{t("collections.title")}</h1>
        <p className="text-sm text-muted-foreground">{t("collections.subtitle")}</p>
      </div>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {!data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {empty ? <p className="text-muted-foreground">{t("collections.empty")}</p> : null}
      {data && data.mine.length > 0 ? <Group title={t("collections.mine")}>{data.mine.map(personal)}</Group> : null}
      {data && data.shared.length > 0 ? (
        <Group title={t("collections.shared")}>{data.shared.map(personal)}</Group>
      ) : null}
      {server.map((l) => (
        <Group key={l.library_id} title={l.library_name}>
          {l.collections.map((c: CollectionCard) => (
            <li key={c.id}>
              <CollectionTile
                serverId={serverId}
                card={c}
                onOpen={() =>
                  void navigate(
                    `../collection/library/${encodeURIComponent(l.library_id)}/${encodeURIComponent(c.id)}?title=${encodeURIComponent(c.title)}`,
                  )
                }
              />
            </li>
          ))}
        </Group>
      ))}
    </div>
  );
}

/** One collection: its name and its titles, in the collection's own order. */
export function CollectionPage() {
  const { t } = useTranslation();
  const { serverId = "", libraryId, collectionId = "" } = useParams();
  const [search] = useSearchParams();
  // A personal collection is read for its name and description; a library's arrives with its name in the link.
  const personal = libraryId === undefined;
  const { data } = useLoad(personal ? `${serverId}/collection/${collectionId}` : null, () =>
    catalog.collection(serverId, collectionId),
  );
  const title = personal ? data?.name : search.get("title");
  const query: CatalogQuery = personal
    ? { source: "user_collection", collectionId }
    : { source: "library_collection", libraryId, collectionId };
  return (
    <section className="flex flex-col gap-6" aria-label={title ?? t("collections.title")}>
      <div className="flex flex-col gap-1">
        <h1 className="min-h-9 text-3xl font-bold tracking-tight">{title ?? ""}</h1>
        {data?.description ? <p className="max-w-3xl text-sm text-muted-foreground">{data.description}</p> : null}
      </div>
      <ItemGrid serverId={serverId} query={query} emptyText={t("browse.emptyCollection")} />
    </section>
  );
}

/** A box set: the films of a series of films in one library, in release order as the server keeps them. */
export function BoxSetPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "", libraryId = "", boxSetId = "" } = useParams();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(
    `${serverId}/box-set/${libraryId}/${boxSetId}`,
    () => catalog.boxSet(serverId, libraryId, boxSetId),
    refresh,
  );
  const { play } = usePlay(serverId);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!data) return <div className="h-72 animate-pulse rounded-2xl bg-card" />;
  const poster = artwork(serverId, data.poster_url);
  const years =
    data.first_year && data.last_year && data.first_year !== data.last_year
      ? `${String(data.first_year)} – ${String(data.last_year)}`
      : data.first_year
        ? String(data.first_year)
        : "";
  return (
    <article className="flex flex-col gap-10">
      <section className="flex flex-col gap-8 md:flex-row md:items-end">
        {poster ? (
          <img
            src={poster}
            alt=""
            className="aspect-[2/3] w-44 shrink-0 rounded-xl bg-surface-raised object-cover shadow-2xl lg:w-52"
          />
        ) : null}
        <div className="flex min-w-0 flex-col gap-3">
          <h1 className="text-4xl font-bold tracking-tight lg:text-5xl">{data.title}</h1>
          <p className="text-sm font-medium text-muted-foreground">
            {[
              years,
              t("boxSet.movies", { count: data.movie_count }),
              t("boxSet.watched", { played: data.played_count, count: data.movie_count }),
            ]
              .filter(Boolean)
              .join(" · ")}
          </p>
          {data.overview ? (
            <p className="max-w-3xl text-sm leading-relaxed text-foreground/85 lg:text-base">{data.overview}</p>
          ) : null}
        </div>
      </section>
      <div className={GRID}>
        {data.items.map((x) => (
          <PosterCard key={x.content_id} serverId={serverId} item={x} onPlay={(i, o) => void play(i.content_id, o)} />
        ))}
      </div>
    </article>
  );
}
