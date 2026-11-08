import { Check, Eye, EyeOff, Info, Play, RotateCcw, X } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router";
import { cardActions, cardDismissal, type CardAction } from "../lib/cardActions";
import { artwork, catalog, type CatalogItem, type HomeSurface } from "../lib/catalog";
import { bumpRefresh } from "../lib/events";
import { qualityTags } from "../lib/format";
import { itemPath } from "../lib/nav";
import { ContextMenu } from "./ContextMenu";

export interface CardProps {
  serverId: string;
  item: CatalogItem;
  /** Wide (16:9) card for continue-watching and episodes; default is a 2:3 poster. */
  wide?: boolean;
  onPlay?: (item: CatalogItem, opts?: { fromStart?: boolean }) => void;
  /** The home surface the card sits on (Continue Watching, Next Up): offers removing it from there. */
  surface?: HomeSurface | undefined;
}

function subtitle(item: CatalogItem): string {
  if (item.type === "episode" && item.season_number != null && item.episode_number != null) {
    return `${item.series_title ?? ""} · S${String(item.season_number)}:E${String(item.episode_number)}`;
  }
  return item.year != null ? String(item.year) : "";
}

/** Poster/landscape card in the web app's style: rounded artwork, hover lift, caption below. */
export function PosterCard({ serverId, item, wide = false, onPlay, surface }: CardProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [failed, setFailed] = useState(false);
  // Optimistic watched state until the rows reload.
  const [playedLocal, setPlayedLocal] = useState<boolean | null>(null);
  const [hidden, setHidden] = useState(false);
  const played = playedLocal ?? item.user_state?.played === true;
  const art = wide
    ? (item.backdrop_url ?? item.series_poster_url ?? item.poster_url)
    : (item.poster_url ?? item.series_poster_url);
  const src = failed ? undefined : artwork(serverId, art);
  const duration = item.duration_seconds ?? (item.runtime ? item.runtime * 60 : 0);
  const progress =
    !hidden && playedLocal === null && item.position_seconds && duration > 0
      ? Math.min(1, item.position_seconds / duration)
      : 0;
  const to = itemPath(serverId, item.content_id);
  const sub = subtitle(item);
  const tags = wide ? [] : qualityTags(item.overlay_summary).slice(0, 3);

  const setWatched = (on: boolean) => {
    setPlayedLocal(on);
    catalog
      .setFlag(serverId, item.content_id, "watched", on)
      .then(() => {
        bumpRefresh(0);
      })
      .catch(() => {
        setPlayedLocal(null);
      });
  };
  const dismissal = cardDismissal(item, surface);
  const dismiss = () => {
    if (!dismissal) return;
    setHidden(true);
    catalog
      .dismiss(serverId, dismissal)
      .then(() => {
        bumpRefresh(0);
      })
      .catch(() => {
        setHidden(false);
      });
  };
  if (hidden) return null;
  const labels: Record<CardAction, { label: string; icon: React.ReactNode; run: () => void }> = {
    play: {
      label: t("item.play"),
      icon: <Play className="size-4" />,
      run: () => onPlay?.(item),
    },
    fromStart: {
      label: t("item.fromStart"),
      icon: <RotateCcw className="size-4" />,
      run: () => onPlay?.(item, { fromStart: true }),
    },
    watched: {
      label: t("item.markWatched"),
      icon: <Eye className="size-4" />,
      run: () => {
        setWatched(true);
      },
    },
    unwatched: {
      label: t("item.markUnwatched"),
      icon: <EyeOff className="size-4" />,
      run: () => {
        setWatched(false);
      },
    },
    dismiss: {
      label: t(dismissal?.surface === "next_up" ? "browse.removeFromNextUp" : "browse.removeFromContinue"),
      icon: <X className="size-4" />,
      run: dismiss,
    },
    details: {
      label: t("browse.details"),
      icon: <Info className="size-4" />,
      run: () => void navigate(to),
    },
  };

  return (
    <div
      className={`media-card group/card flex shrink-0 flex-col gap-2 ${wide ? "w-72" : "w-40"}`}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <Link
        to={to}
        className="media-card-image relative block overflow-hidden rounded-xl bg-surface-raised ring-ring shadow-[0_24px_40px_-26px_rgba(0,0,0,0.8)] focus-visible:ring-2 focus-visible:outline-none"
        aria-label={sub ? `${item.title}, ${sub}` : item.title}
      >
        <div className={wide ? "aspect-video" : "aspect-[2/3]"}>
          {src ? (
            <img
              src={src}
              alt=""
              loading="lazy"
              decoding="async"
              draggable={false}
              className="size-full object-cover"
              onError={() => {
                setFailed(true);
              }}
            />
          ) : (
            <div className="flex size-full items-center justify-center p-3 text-center text-sm font-semibold text-muted-foreground">
              {item.title}
            </div>
          )}
        </div>
        {tags.length > 0 ? (
          // Shown only with the "quality tags on posters" setting (html[data-poster-tags]).
          <span className="poster-tags absolute top-2 left-2 hidden max-w-[calc(100%-2.5rem)] flex-wrap gap-1">
            {tags.map((x) => (
              <span
                key={x}
                className="rounded-[4px] bg-black/70 px-1.5 py-px text-[9px] font-bold tracking-wide text-white uppercase backdrop-blur"
              >
                {x}
              </span>
            ))}
          </span>
        ) : null}
        {played ? (
          <span className="absolute top-2 right-2 flex size-6 items-center justify-center rounded-full bg-black/60 text-white">
            <Check className="size-3.5" aria-label={t("browse.watched")} />
          </span>
        ) : null}
        {progress > 0 ? (
          <span className="absolute inset-x-2 bottom-2 h-1 overflow-hidden rounded-full bg-white/25">
            <span className="block h-full bg-primary" style={{ width: `${String(progress * 100)}%` }} />
          </span>
        ) : null}
      </Link>
      <div className="min-w-0 px-0.5">
        <Link to={to} className="block truncate text-sm font-semibold hover:underline" tabIndex={-1}>
          {item.title}
        </Link>
        {sub ? <p className="truncate text-xs font-medium text-muted-foreground">{sub}</p> : null}
      </div>
      {menu ? (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          label={item.title}
          onClose={() => {
            setMenu(null);
          }}
          actions={cardActions(item, { canPlay: onPlay !== undefined, surface, played }).map((a) => ({
            label: labels[a].label,
            icon: labels[a].icon,
            onSelect: labels[a].run,
          }))}
        />
      ) : null}
    </div>
  );
}
