import { Info, Play } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";
import { PosterCard } from "../components/PosterCard";
import { Row } from "../components/Row";
import { Alert, Button } from "../components/ui";
import { artwork, catalog, type CatalogItem, type HomeSurface, type Section } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { homeCache } from "../lib/homeCache";
import { signalReady } from "../lib/host";
import { goReauth, itemPath, needsReauth } from "../lib/nav";

import { retryLoads, useRefreshTick, useRetryTick } from "../lib/events";
import { usePlay } from "../lib/usePlay";

const WIDE_SECTIONS = new Set(["continue_watching", "next_up", "in_progress"]);
/** Rows whose cards can be removed from their home surface (dismissHomeItem). */
const SURFACES: Record<string, HomeSurface | undefined> = {
  continue_watching: "continue_watching",
  in_progress: "continue_watching",
  next_up: "next_up",
};

function Hero({ serverId, item, onPlay }: { serverId: string; item: CatalogItem; onPlay: () => void }) {
  const { t } = useTranslation();
  const backdrop = artwork(serverId, item.backdrop_url);
  return (
    <section className="relative isolate overflow-hidden rounded-2xl border border-border/60 bg-card">
      {backdrop ? (
        <img
          src={backdrop}
          alt=""
          decoding="async"
          fetchPriority="high"
          className="absolute inset-0 -z-10 size-full object-cover opacity-60"
        />
      ) : null}
      <div className="absolute inset-0 -z-10 bg-gradient-to-r from-background via-background/70 to-transparent" />
      <div className="flex min-h-[18rem] max-w-2xl flex-col justify-end gap-4 p-6 lg:min-h-[24rem] lg:p-10">
        <h1 className="text-3xl font-semibold tracking-tight lg:text-5xl">{item.title}</h1>
        {item.overview ? (
          <p className="line-clamp-3 text-sm text-muted-foreground lg:text-base">{item.overview}</p>
        ) : null}
        <div className="flex gap-2">
          <Button onClick={onPlay} className="inline-flex items-center gap-2">
            <Play className="size-4" />
            {t("item.play")}
          </Button>
          <Link
            to={itemPath(serverId, item.content_id)}
            className="inline-flex items-center gap-2 rounded-lg border border-border bg-secondary px-4 py-2 text-sm font-medium transition-colors hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
          >
            <Info className="size-4" />
            {t("browse.details")}
          </Link>
        </div>
      </div>
    </section>
  );
}

export function ServerHome() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  // Rows loaded before (this run, same session) are drawn at once and reloaded behind.
  const [loaded, setLoaded] = useState<{ serverId: string; sections: Section[] } | null>(null);
  const sections = loaded?.serverId === serverId ? loaded.sections : (homeCache.get(serverId) ?? null);
  const [error, setError] = useState<string | null>(null);
  // Started straight into a server: interactive once the home rows (or their error) are in.
  useEffect(() => {
    if (sections !== null || error !== null) signalReady();
  }, [sections, error]);
  const { play, error: playError } = usePlay(serverId);
  const refresh = useRefreshTick();
  const retry = useRetryTick();

  useEffect(() => {
    let alive = true;
    catalog
      .homeSections(serverId)
      .then((s) => {
        if (!alive) return;
        const next = s.filter((x) => x.items.length > 0);
        const before = homeCache.get(serverId);
        // Unchanged rows keep their identity: nothing re-renders after a reload that brought nothing new.
        const same = before !== undefined && JSON.stringify(before) === JSON.stringify(next);
        const sections = same ? before : next;
        homeCache.set(serverId, sections);
        setLoaded((l) => (l?.serverId === serverId && l.sections === sections ? l : { serverId, sections }));
        setError(null);
        // Artwork of what is being watched stays in the artwork cache longest (ADR-0010).
        const keep = s
          .filter((x) => SURFACES[x.section_type] !== undefined)
          .flatMap((x) => x.items)
          .flatMap((i) => [i.poster_url, i.backdrop_url, i.logo_url, i.series_poster_url])
          .filter((u): u is string => typeof u === "string" && u.length > 0);
        if (keep.length > 0) void catalog.artworkKeep(serverId, keep).catch(() => undefined);
      })
      .catch((e: unknown) => {
        if (!alive) return;
        if (needsReauth(e)) goReauth(navigate, serverId, e);
        else setError(errorMessage(t, e));
      });
    return () => {
      alive = false;
    };
  }, [serverId, navigate, t, refresh, retry]);

  const onPlay = (item: CatalogItem, opts?: { fromStart?: boolean }) =>
    void play(item.play_content_id ?? item.content_id, opts);
  const featured = sections?.find((s) => s.featured)?.items[0];

  return (
    <div className="flex flex-col gap-10">
      {error ? <Alert onRetry={retryLoads}>{error}</Alert> : null}
      {playError ? <Alert>{playError}</Alert> : null}
      {sections === null && !error ? <p className="text-muted-foreground">{t("status.loading")}</p> : null}
      {featured ? (
        <Hero
          serverId={serverId}
          item={featured}
          onPlay={() => {
            onPlay(featured);
          }}
        />
      ) : null}
      {sections?.length === 0 ? <p className="text-muted-foreground">{t("browse.emptyHome")}</p> : null}
      {sections?.map((s) => (
        <Row key={s.id} title={s.title}>
          {s.items.map((item) => (
            <PosterCard
              key={item.content_id}
              serverId={serverId}
              item={item}
              wide={WIDE_SECTIONS.has(s.section_type)}
              surface={SURFACES[s.section_type]}
              onPlay={onPlay}
            />
          ))}
        </Row>
      ))}
    </div>
  );
}
