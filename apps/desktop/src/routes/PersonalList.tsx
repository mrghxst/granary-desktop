import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Navigate, useParams } from "react-router";
import type { CatalogQuery } from "../lib/catalog";
import { useRefreshTick } from "../lib/events";
import { ItemGrid } from "./Library";

const SOURCES = ["watchlist", "favorites", "history"] as const;
type Source = (typeof SOURCES)[number];

const TABS = [
  { key: "all", type: undefined },
  { key: "movies", type: "movie" },
  { key: "series", type: "series" },
] as const;

/**
 * The profile's watchlist, favorites or watch history: the catalog read with `source` set, in the list's own order
 * (newest entry first), as the web app's catalog page does (`silo-server/web/src/pages/Catalog.tsx@749db96a`).
 */
export function PersonalList() {
  const { t } = useTranslation();
  const { serverId = "", source = "" } = useParams();
  const [tab, setTab] = useState<(typeof TABS)[number]["key"]>("all");
  // A flag changed elsewhere (this app or another client): the list is read again.
  const refresh = useRefreshTick();
  if (!SOURCES.includes(source as Source)) return <Navigate to="../home" replace />;
  const list = source as Source;
  const type = TABS.find((x) => x.key === tab)?.type;
  const query: CatalogQuery = { source: list, ...(type ? { type } : {}) };

  return (
    <section className="flex flex-col gap-6" aria-label={t(`lists.${list}.title`)}>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-bold tracking-tight">{t(`lists.${list}.title`)}</h1>
          <p className="text-sm text-muted-foreground">{t(`lists.${list}.subtitle`)}</p>
        </div>
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
              {t(`lists.tabs.${x.key}`)}
            </button>
          ))}
        </div>
      </div>
      <ItemGrid serverId={serverId} query={query} emptyText={t(`lists.${list}.empty`)} nonce={refresh} />
    </section>
  );
}
