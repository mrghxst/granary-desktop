import { Search as SearchIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useSearchParams } from "react-router";
import type { CatalogQuery } from "../lib/catalog";
import { ItemGrid } from "./Library";

const DEBOUNCE_MS = 300;

/**
 * Result types (OAS listCatalogItems `type`, `docs/research/silo-api.md` §catalog): `video_with_episodes` searches
 * movies, series and episodes together; the others narrow it to one kind.
 */
const TYPES = [
  { key: "all", type: "video_with_episodes", label: "browse.searchTypeAll" },
  { key: "movies", type: "movie", label: "browse.searchTypeMovies" },
  { key: "shows", type: "series", label: "browse.searchTypeShows" },
  { key: "episodes", type: "episode", label: "browse.searchTypeEpisodes" },
] as const;
type TypeKey = (typeof TYPES)[number]["key"];

function typeKey(v: string | null): TypeKey {
  return TYPES.find((x) => x.key === v)?.key ?? "all";
}

export function Search() {
  const { t } = useTranslation();
  const { serverId = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const [text, setText] = useState(params.get("q") ?? "");
  const [q, setQ] = useState(text.trim());
  const [kind, setKind] = useState<TypeKey>(() => typeKey(params.get("type")));

  useEffect(() => {
    const id = window.setTimeout(() => {
      const v = text.trim();
      setQ(v);
      setParams({ ...(v ? { q: v } : {}), ...(kind !== "all" ? { type: kind } : {}) }, { replace: true });
    }, DEBOUNCE_MS);
    return () => {
      window.clearTimeout(id);
    };
  }, [text, kind, setParams]);

  const type = TYPES.find((x) => x.key === kind)?.type ?? "video_with_episodes";
  const query = useMemo<CatalogQuery | null>(() => (q ? { q, type } : null), [q, type]);

  return (
    <div className="flex flex-col gap-6">
      <h1 className="text-3xl font-semibold tracking-tight">{t("shell.search")}</h1>
      <label className="relative block max-w-2xl">
        <span className="sr-only">{t("browse.searchLabel")}</span>
        <SearchIcon className="pointer-events-none absolute top-1/2 left-3 size-5 -translate-y-1/2 text-muted-foreground" />
        <input
          type="search"
          value={text}
          autoFocus
          placeholder={t("browse.searchPlaceholder")}
          onChange={(e) => {
            setText(e.target.value);
          }}
          className="w-full rounded-xl border border-border bg-card py-3 pr-4 pl-11 text-base placeholder:text-muted-foreground focus:border-ring focus:outline-none"
        />
      </label>
      <div role="radiogroup" aria-label={t("browse.searchType")} className="-mt-2 flex flex-wrap gap-2">
        {TYPES.map((x) => (
          <button
            key={x.key}
            type="button"
            role="radio"
            aria-checked={kind === x.key}
            onClick={() => {
              setKind(x.key);
            }}
            className={`rounded-full border px-3.5 py-1.5 text-sm font-medium transition-colors ${
              kind === x.key
                ? "border-transparent bg-primary text-primary-foreground"
                : "border-border bg-card text-muted-foreground hover:text-foreground"
            }`}
          >
            {t(x.label)}
          </button>
        ))}
      </div>
      {query ? (
        <ItemGrid serverId={serverId} query={query} emptyText={t("browse.noResults", { q })} />
      ) : (
        <p className="text-muted-foreground">{t("browse.searchHint")}</p>
      )}
    </div>
  );
}
