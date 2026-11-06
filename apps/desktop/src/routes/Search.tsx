import { Search as SearchIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams, useSearchParams } from "react-router";
import type { CatalogQuery } from "../lib/catalog";
import { ItemGrid } from "./Library";

const DEBOUNCE_MS = 300;

export function Search() {
  const { t } = useTranslation();
  const { serverId = "" } = useParams();
  const [params, setParams] = useSearchParams();
  const [text, setText] = useState(params.get("q") ?? "");
  const [q, setQ] = useState(text.trim());

  useEffect(() => {
    const id = window.setTimeout(() => {
      const v = text.trim();
      setQ(v);
      setParams(v ? { q: v } : {}, { replace: true });
    }, DEBOUNCE_MS);
    return () => {
      window.clearTimeout(id);
    };
  }, [text, setParams]);

  // `video_with_episodes` searches movies, series and episodes together (OAS listCatalogItems `type`).
  const query = useMemo<CatalogQuery | null>(() => (q ? { q, type: "video_with_episodes" } : null), [q]);

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
      {query ? (
        <ItemGrid serverId={serverId} query={query} emptyText={t("browse.noResults", { q })} />
      ) : (
        <p className="text-muted-foreground">{t("browse.searchHint")}</p>
      )}
    </div>
  );
}
