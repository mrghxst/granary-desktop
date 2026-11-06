import { Check, Info, Play } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate } from "react-router";
import { artwork, type CatalogItem } from "../lib/catalog";
import { itemPath } from "../lib/nav";
import { ContextMenu } from "./ContextMenu";

export interface CardProps {
  serverId: string;
  item: CatalogItem;
  /** Wide (16:9) card for continue-watching and episodes; default is a 2:3 poster. */
  wide?: boolean;
  onPlay?: (item: CatalogItem) => void;
}

function subtitle(item: CatalogItem): string {
  if (item.type === "episode" && item.season_number != null && item.episode_number != null) {
    return `${item.series_title ?? ""} · S${String(item.season_number)}:E${String(item.episode_number)}`;
  }
  return item.year != null ? String(item.year) : "";
}

/** Poster/landscape card in the web app's style: rounded artwork, hover lift, caption below. */
export function PosterCard({ serverId, item, wide = false, onPlay }: CardProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [failed, setFailed] = useState(false);
  const art = wide
    ? (item.backdrop_url ?? item.series_poster_url ?? item.poster_url)
    : (item.poster_url ?? item.series_poster_url);
  const src = failed ? undefined : artwork(serverId, art);
  const duration = item.duration_seconds ?? (item.runtime ? item.runtime * 60 : 0);
  const progress = item.position_seconds && duration > 0 ? Math.min(1, item.position_seconds / duration) : 0;
  const to = itemPath(serverId, item.content_id);
  const sub = subtitle(item);

  return (
    <div
      className={`group/card flex shrink-0 flex-col gap-2 ${wide ? "w-72" : "w-40"}`}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ x: e.clientX, y: e.clientY });
      }}
    >
      <Link
        to={to}
        className="relative block overflow-hidden rounded-xl bg-surface-raised ring-ring transition duration-200 group-hover/card:-translate-y-0.5 group-hover/card:shadow-[0_18px_40px_-20px_rgba(0,0,0,0.8)] focus-visible:ring-2 focus-visible:outline-none motion-reduce:transition-none motion-reduce:group-hover/card:translate-y-0"
        aria-label={sub ? `${item.title}, ${sub}` : item.title}
      >
        <div className={wide ? "aspect-video" : "aspect-[2/3]"}>
          {src ? (
            <img
              src={src}
              alt=""
              loading="lazy"
              decoding="async"
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
        {item.user_state?.played ? (
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
          actions={[
            ...(onPlay
              ? [
                  {
                    label: t("item.play"),
                    icon: <Play className="size-4" />,
                    onSelect: () => {
                      onPlay(item);
                    },
                  },
                ]
              : []),
            {
              label: t("browse.details"),
              icon: <Info className="size-4" />,
              onSelect: () => {
                void navigate(to);
              },
            },
          ]}
        />
      ) : null}
    </div>
  );
}
