import { Info, Play } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";
import { PosterCard } from "../components/PosterCard";
import { Row } from "../components/Row";
import { Alert, Button } from "../components/ui";
import { artwork, catalog, type CatalogItem, type Section } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { signalReady } from "../lib/host";
import { goReauth, itemPath, needsReauth } from "../lib/nav";

import { useRefreshTick } from "../lib/events";
import { usePlay } from "../lib/usePlay";

const WIDE_SECTIONS = new Set(["continue_watching", "next_up", "in_progress"]);

function Hero({ serverId, item, onPlay }: { serverId: string; item: CatalogItem; onPlay: () => void }) {
  const { t } = useTranslation();
  const backdrop = artwork(serverId, item.backdrop_url);
  return (
    <section className="relative isolate overflow-hidden rounded-2xl border border-border/60 bg-card">
      {backdrop ? (
        <img src={backdrop} alt="" className="absolute inset-0 -z-10 size-full object-cover opacity-60" />
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
            className="inline-flex items-center gap-2 rounded-lg bg-secondary px-4 py-2 text-sm font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
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
  const [sections, setSections] = useState<Section[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Started straight into a server: interactive once the home rows (or their error) are in.
  useEffect(() => {
    if (sections !== null || error !== null) signalReady();
  }, [sections, error]);
  const { play, error: playError } = usePlay(serverId);
  const refresh = useRefreshTick();

  useEffect(() => {
    let alive = true;
    catalog
      .homeSections(serverId)
      .then((s) => {
        if (alive) setSections(s.filter((x) => x.items.length > 0));
      })
      .catch((e: unknown) => {
        if (!alive) return;
        if (needsReauth(e)) goReauth(navigate, serverId, e);
        else setError(errorMessage(t, e));
      });
    return () => {
      alive = false;
    };
  }, [serverId, navigate, t, refresh]);

  const onPlay = (item: CatalogItem) => void play(item.play_content_id ?? item.content_id);
  const featured = sections?.find((s) => s.featured)?.items[0];

  return (
    <div className="flex flex-col gap-10">
      {error ? <Alert>{error}</Alert> : null}
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
              onPlay={onPlay}
            />
          ))}
        </Row>
      ))}
    </div>
  );
}
