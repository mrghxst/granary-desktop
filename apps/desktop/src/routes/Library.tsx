import { ArrowLeft, Check, ChevronDown, Layers, Shuffle, SlidersHorizontal } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { Popover } from "../components/Popover";
import { PosterCard } from "../components/PosterCard";
import { Row } from "../components/Row";
import { Alert, Button } from "../components/ui";
import {
  artwork,
  catalog,
  type CatalogItem,
  type CatalogQuery,
  type CollectionCard,
  type PageInfo,
} from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, needsReauth } from "../lib/nav";
import { silo } from "../lib/silo";
import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";

/** Sort fields from the catalog filters document (docs/research/silo-api.md §4.2). */
const SORTS = ["title", "-added_at", "-release_date", "-rating", "random"] as const;
type Sort = (typeof SORTS)[number];
const PAGE = 60;
const LETTERS = ["#", ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("")];
const DECADES = [2020, 2010, 2000, 1990, 1980, 1970, 1960, 1950] as const;

interface Page {
  items: CatalogItem[];
  page?: PageInfo | null;
  total?: number;
}

/**
 * A poster grid that loads the next page when its end scrolls into view (a button where IntersectionObserver is
 * missing). `loadKey` identifies the list; a new key starts over.
 */
export function PagedGrid({
  serverId,
  loadKey,
  fetchPage,
  emptyText,
  showCount = true,
}: {
  serverId: string;
  loadKey: string | null;
  fetchPage: (cursor: string | null) => Promise<Page>;
  emptyText: string;
  showCount?: boolean;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const first = useLoad(loadKey, () => fetchPage(null));
  // Further pages for the current key; a new key ignores pages loaded for an older one.
  const [more, setMore] = useState<{ key: string | null; items: CatalogItem[]; cursor: string | null }>({
    key: null,
    items: [],
    cursor: null,
  });
  const [moreError, setMoreError] = useState<unknown>(null);
  const [loading, setLoading] = useState(false);
  const busy = useRef(false);
  const sentinel = useRef<HTMLDivElement>(null);
  const { play, error: playError } = usePlay(serverId);

  useEffect(() => {
    if (needsReauth(first.error)) goReauth(navigate, serverId, first.error);
  }, [first.error, navigate, serverId]);

  const extra = more.key === loadKey ? more : null;
  const items = first.data ? [...first.data.items, ...(extra?.items ?? [])] : undefined;
  const firstCursor = first.data?.page?.has_more ? (first.data.page.next_cursor ?? null) : null;
  const cursor = extra ? extra.cursor : firstCursor;
  const total = first.data?.total ?? items?.length ?? 0;
  const failure = first.error ?? moreError;
  const error = failure && !needsReauth(failure) ? errorMessage(t, failure) : null;

  const loadMore = useCallback(async () => {
    if (!cursor || busy.current) return;
    busy.current = true;
    setLoading(true);
    try {
      const page = await fetchPage(cursor);
      setMore({
        key: loadKey,
        items: [...(extra?.items ?? []), ...page.items],
        cursor: page.page?.has_more ? (page.page.next_cursor ?? null) : null,
      });
      setMoreError(null);
    } catch (e) {
      if (needsReauth(e)) goReauth(navigate, serverId, e);
      else setMoreError(e);
    } finally {
      busy.current = false;
      setLoading(false);
    }
  }, [cursor, extra, fetchPage, loadKey, navigate, serverId]);

  const observe = typeof IntersectionObserver !== "undefined";
  useEffect(() => {
    const el = sentinel.current;
    if (!observe || !el || !cursor || moreError) return;
    const io = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) void loadMore();
      },
      { rootMargin: "1200px 0px" },
    );
    io.observe(el);
    return () => {
      io.disconnect();
    };
  }, [observe, cursor, loadMore, moreError]);

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
      {items === undefined && loadKey && !error ? <GridSkeleton /> : null}
      {items?.length === 0 ? <p className="text-muted-foreground">{emptyText}</p> : null}
      {items && items.length > 0 ? (
        <>
          {showCount ? <p className="text-sm text-muted-foreground">{t("browse.count", { count: total })}</p> : null}
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
      {cursor ? (
        <div ref={sentinel} className="flex justify-center py-4">
          {observe ? (
            loading ? (
              <span className="size-5 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-foreground" />
            ) : null
          ) : (
            <Button variant="secondary" disabled={loading} onClick={() => void loadMore()}>
              {loading ? t("status.loading") : t("browse.more")}
            </Button>
          )}
        </div>
      ) : null}
    </div>
  );
}

function GridSkeleton() {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6" aria-hidden>
      {Array.from({ length: 12 }, (_, i) => (
        <div key={i} className="aspect-[2/3] animate-pulse rounded-xl bg-surface-raised/60" />
      ))}
    </div>
  );
}

/** A grid of catalog items, used by the library, search and person pages. */
export function ItemGrid({
  serverId,
  query,
  emptyText,
  nonce = 0,
}: {
  serverId: string;
  query: CatalogQuery | null;
  emptyText: string;
  /** A new value reloads the same query ("random" sorts again server-side). */
  nonce?: number;
}) {
  const key = query ? `${serverId}|${String(nonce)}|${JSON.stringify(query)}` : null;
  const fetchPage = useCallback(
    (cursor: string | null) =>
      query
        ? catalog.browse(serverId, { ...query, limit: PAGE, ...(cursor ? { cursor } : {}) })
        : Promise.reject(new Error("no query")),
    [serverId, query],
  );
  return <PagedGrid serverId={serverId} loadKey={key} fetchPage={fetchPage} emptyText={emptyText} />;
}

/* ---------------------------------------------------------------- tabs */

function Recommended({ serverId, libraryId }: { serverId: string; libraryId: string }) {
  const { t } = useTranslation();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(
    `${serverId}/${libraryId}/sections`,
    () => catalog.librarySections(serverId, libraryId),
    refresh,
  );
  const { play } = usePlay(serverId);
  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!data) return <GridSkeleton />;
  const rows = data.filter((s) => s.items.length > 0);
  if (rows.length === 0) return <p className="text-muted-foreground">{t("browse.emptyLibrary")}</p>;
  return (
    <div className="flex flex-col gap-10">
      {rows.map((s) => {
        const wide = s.section_type === "continue_watching" || s.items.every((i) => i.type === "episode");
        return (
          <Row key={s.id} title={s.title}>
            {s.items.map((i) => (
              <PosterCard
                key={i.content_id}
                serverId={serverId}
                item={i}
                wide={wide}
                onPlay={(x) => void play(x.play_content_id ?? x.content_id)}
              />
            ))}
          </Row>
        );
      })}
    </div>
  );
}

function Collections({ serverId, libraryId }: { serverId: string; libraryId: string }) {
  const { t } = useTranslation();
  const { data, error } = useLoad(`${serverId}/${libraryId}/collections`, () =>
    catalog.libraryCollections(serverId, libraryId),
  );
  const [open, setOpen] = useState<CollectionCard | null>(null);
  const fetchPage = useCallback(
    (cursor: string | null) =>
      open ? catalog.collectionItems(serverId, libraryId, open.id, cursor) : Promise.reject(new Error("none")),
    [serverId, libraryId, open],
  );

  if (open) {
    return (
      <div className="flex flex-col gap-5">
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => {
              setOpen(null);
            }}
            aria-label={t("browse.backToCollections")}
            className="flex size-9 items-center justify-center rounded-full border border-border hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
          >
            <ArrowLeft className="size-4" />
          </button>
          <h2 className="text-xl font-semibold tracking-tight">{open.title}</h2>
          <span className="text-sm text-muted-foreground">{t("browse.count", { count: open.item_count })}</span>
        </div>
        <PagedGrid
          serverId={serverId}
          loadKey={`${serverId}/${libraryId}/collection/${open.id}`}
          fetchPage={fetchPage}
          emptyText={t("browse.emptyCollection")}
          showCount={false}
        />
      </div>
    );
  }

  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!data) return <GridSkeleton />;
  if (data.length === 0) return <p className="text-muted-foreground">{t("browse.noCollections")}</p>;
  return (
    <ul className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
      {data.map((c) => (
        <li key={c.id}>
          <CollectionTile
            serverId={serverId}
            card={c}
            onOpen={() => {
              setOpen(c);
            }}
          />
        </li>
      ))}
    </ul>
  );
}

function CollectionTile({ serverId, card, onOpen }: { serverId: string; card: CollectionCard; onOpen: () => void }) {
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  const src = failed ? undefined : artwork(serverId, card.poster_url);
  return (
    <button type="button" onClick={onOpen} className="group/c w-full text-left focus-visible:outline-none">
      <span className="relative block aspect-[2/3] overflow-visible">
        {/* Two offset sheets read as a stack: a collection, not a single title. */}
        <span className="absolute inset-x-3 -top-1.5 h-full rounded-xl bg-surface-raised/50" />
        <span className="absolute inset-x-1.5 -top-0.5 h-full rounded-xl bg-surface-raised/80" />
        <span className="relative block size-full overflow-hidden rounded-xl bg-surface-raised ring-ring transition duration-200 group-hover/c:-translate-y-0.5 group-focus-visible/c:ring-2">
          {src ? (
            <img
              src={src}
              alt=""
              loading="lazy"
              className="size-full object-cover"
              onError={() => {
                setFailed(true);
              }}
            />
          ) : (
            <span className="flex size-full items-center justify-center text-muted-foreground">
              <Layers className="size-8" />
            </span>
          )}
        </span>
      </span>
      <span className="mt-2 block truncate px-0.5 text-sm font-semibold">{card.title}</span>
      <span className="block px-0.5 text-xs font-medium text-muted-foreground">
        {t("browse.count", { count: card.item_count })}
      </span>
    </button>
  );
}

/* ---------------------------------------------------------------- library tab controls */

function MenuButton({ label, value, open }: { label: string; value: string; open: boolean }) {
  return (
    <span className="flex h-9 items-center gap-2 rounded-full border border-border px-4 text-sm transition hover:border-foreground/40">
      <span className="text-muted-foreground">{label}</span>
      <span className="font-medium">{value}</span>
      <ChevronDown className={`size-3.5 text-muted-foreground transition ${open ? "rotate-180" : ""}`} />
    </span>
  );
}

function Option({ selected, onSelect, children }: { selected: boolean; onSelect: () => void; children: string }) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none"
    >
      <Check className={`size-4 ${selected ? "opacity-100" : "opacity-0"}`} />
      {children}
    </button>
  );
}

/** A–Z rail: filters the grid to titles starting with a letter (`name_prefix`); the active letter clears it. */
function LetterRail({ value, onChange }: { value: string | null; onChange: (v: string | null) => void }) {
  const { t } = useTranslation();
  return (
    <nav
      aria-label={t("browse.jumpToLetter")}
      className="sticky top-4 hidden max-h-[calc(100vh-6rem)] w-6 shrink-0 flex-col items-center justify-between self-start py-1 md:flex"
    >
      {LETTERS.map((l) => (
        <button
          key={l}
          type="button"
          aria-pressed={value === l}
          onClick={() => {
            onChange(value === l ? null : l);
          }}
          className={`flex size-6 items-center justify-center rounded-full text-[11px] leading-none font-semibold transition focus-visible:outline-2 focus-visible:outline-ring ${
            value === l
              ? "bg-foreground text-background"
              : "text-muted-foreground hover:scale-125 hover:text-foreground motion-reduce:hover:scale-100"
          }`}
        >
          {l}
        </button>
      ))}
    </nav>
  );
}

function LibraryTab({ serverId, libraryId }: { serverId: string; libraryId: string }) {
  const { t } = useTranslation();
  const [sort, setSort] = useState<Sort>("title");
  const [letter, setLetter] = useState<string | null>(null);
  const [genre, setGenre] = useState<string | null>(null);
  const [decade, setDecade] = useState<number | null>(null);
  const [shuffleSeed, setShuffleSeed] = useState(0);
  const { data: filters } = useLoad(`${serverId}/${libraryId}/filters`, () => catalog.filters(serverId, libraryId));
  const query = useMemo<CatalogQuery>(
    () => ({
      libraryId,
      sort,
      ...(letter ? { namePrefix: letter } : {}),
      ...(genre ? { genre } : {}),
      ...(decade ? { yearMin: decade, yearMax: decade + 9 } : {}),
    }),
    [libraryId, sort, letter, genre, decade],
  );
  const filtered = genre !== null || decade !== null;
  const sortLabel = (s: Sort) => t(`browse.sorts.${s.replace("-", "")}`);

  return (
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => {
            setSort("random");
            setShuffleSeed((x) => x + 1);
          }}
          className="flex h-9 items-center gap-2 rounded-full bg-foreground px-4 text-sm font-semibold text-background transition hover:opacity-90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
        >
          <Shuffle className="size-4" />
          {t("browse.shuffle")}
        </button>
        <Popover
          label={t("browse.sort")}
          trigger={(open) => <MenuButton label={t("browse.sort")} value={sortLabel(sort)} open={open} />}
        >
          {(close) =>
            SORTS.map((s) => (
              <Option
                key={s}
                selected={s === sort}
                onSelect={() => {
                  setSort(s);
                  close();
                }}
              >
                {sortLabel(s)}
              </Option>
            ))
          }
        </Popover>
        <Popover
          label={t("browse.filters")}
          trigger={() => (
            <span
              className={`flex h-9 items-center gap-2 rounded-full border px-4 text-sm transition hover:border-foreground/40 ${
                filtered ? "border-foreground text-foreground" : "border-border"
              }`}
            >
              <SlidersHorizontal className="size-4" />
              {t("browse.filters")}
              {filtered ? (
                <span className="flex size-5 items-center justify-center rounded-full bg-foreground text-[10px] font-bold text-background">
                  {Number(genre !== null) + Number(decade !== null)}
                </span>
              ) : null}
            </span>
          )}
        >
          {() => (
            <div className="flex w-80 flex-col gap-4 p-3">
              <FilterChips title={t("browse.genre")} values={filters?.genres ?? []} value={genre} onChange={setGenre} />
              <FilterChips
                title={t("browse.decade")}
                values={DECADES.map(String)}
                format={(d) => `${d}s`}
                value={decade === null ? null : String(decade)}
                onChange={(d) => {
                  setDecade(d === null ? null : Number(d));
                }}
              />
              {filtered ? (
                <button
                  type="button"
                  onClick={() => {
                    setGenre(null);
                    setDecade(null);
                  }}
                  className="self-start text-sm font-medium text-muted-foreground hover:text-foreground"
                >
                  {t("browse.clearFilters")}
                </button>
              ) : null}
            </div>
          )}
        </Popover>
        {letter ? (
          <button
            type="button"
            onClick={() => {
              setLetter(null);
            }}
            className="flex h-9 items-center gap-1.5 rounded-full border border-foreground px-3 text-sm font-medium"
          >
            {t("browse.startsWith", { letter })} ×
          </button>
        ) : null}
      </div>
      <div className="flex gap-4">
        <div className="min-w-0 flex-1">
          <ItemGrid
            serverId={serverId}
            query={query}
            nonce={sort === "random" ? shuffleSeed : 0}
            emptyText={t("browse.emptyLibrary")}
          />
        </div>
        <LetterRail value={letter} onChange={setLetter} />
      </div>
    </div>
  );
}

function FilterChips({
  title,
  values,
  value,
  onChange,
  format = (v) => v,
}: {
  title: string;
  values: string[];
  value: string | null;
  onChange: (v: string | null) => void;
  format?: (v: string) => string;
}) {
  if (values.length === 0) return null;
  return (
    <div className="flex flex-col gap-2">
      <span className="text-xs font-semibold tracking-wider text-muted-foreground uppercase">{title}</span>
      <div className="flex max-h-40 flex-wrap gap-1.5 overflow-y-auto">
        {values.map((v) => (
          <button
            key={v}
            type="button"
            aria-pressed={value === v}
            onClick={() => {
              onChange(value === v ? null : v);
            }}
            className={`rounded-full border px-3 py-1 text-xs font-medium transition ${
              value === v
                ? "border-foreground bg-foreground text-background"
                : "border-border text-muted-foreground hover:text-foreground"
            }`}
          >
            {format(v)}
          </button>
        ))}
      </div>
    </div>
  );
}

/* ---------------------------------------------------------------- page */

const TABS = ["recommended", "library", "collections"] as const;
type Tab = (typeof TABS)[number];

export function Library() {
  const { t } = useTranslation();
  const { serverId = "", libraryId = "" } = useParams();
  const [tabs, setTabs] = useState<{ libraryId: string; tab: Tab } | null>(null);
  const tab: Tab = tabs?.libraryId === libraryId ? tabs.tab : "library";
  const [name, setName] = useState("");
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
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h1 className="text-3xl font-semibold tracking-tight">{name || t("libraries.title")}</h1>
        <div role="tablist" aria-label={t("browse.views")} className="flex gap-1 rounded-full border border-border p-1">
          {TABS.map((x) => (
            <button
              key={x}
              type="button"
              role="tab"
              aria-selected={tab === x}
              onClick={() => {
                setTabs({ libraryId, tab: x });
              }}
              className={`rounded-full px-4 py-1.5 text-sm font-medium transition focus-visible:outline-2 focus-visible:outline-ring ${
                tab === x ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {t(`browse.tabs.${x}`)}
            </button>
          ))}
        </div>
      </div>
      {tab === "recommended" ? <Recommended key={libraryId} serverId={serverId} libraryId={libraryId} /> : null}
      {tab === "library" ? <LibraryTab key={libraryId} serverId={serverId} libraryId={libraryId} /> : null}
      {tab === "collections" ? <Collections key={libraryId} serverId={serverId} libraryId={libraryId} /> : null}
    </div>
  );
}
