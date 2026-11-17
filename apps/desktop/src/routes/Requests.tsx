import { Check, Inbox, Plus, Search } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";
import { Row } from "../components/Row";
import { Alert } from "../components/ui";
import { artwork, catalog, type RequestMediaResult, type TitleRequest } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, itemPath, needsReauth } from "../lib/nav";
import { useLoad } from "../lib/useLoad";

type Tab = "discover" | "search" | "mine";
const TABS: Tab[] = ["discover", "search", "mine"];
const STATES = [
  "pending",
  "approved",
  "processing",
  "partially_available",
  "available",
  "declined",
  "cancelled",
  "failed",
];
/** Request states the requester can still withdraw. */
const OPEN = ["pending", "approved", "processing"];
const GRID = "grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6";

/** The request provider's images are TMDB paths; the host fetches them like any other artwork. */
const poster = (serverId: string, path: string | null | undefined) =>
  artwork(serverId, path ? `https://image.tmdb.org/t/p/w342${path}` : undefined);

function StatePill({ state, inRow = false }: { state: string; inRow?: boolean }) {
  const { t } = useTranslation();
  const tone =
    state === "available" || state === "partially_available"
      ? "bg-emerald-500/15 text-emerald-300"
      : state === "declined" || state === "failed"
        ? "bg-red-500/15 text-red-300"
        : state === "cancelled"
          ? "bg-surface-hover text-muted-foreground"
          : "bg-amber-500/15 text-amber-200";
  return (
    <span className={`${inRow ? "shrink-0" : "self-start"} rounded-full px-2.5 py-1 text-xs font-semibold ${tone}`}>
      {STATES.includes(state) ? t(`requests.state.${state}`) : state}
    </span>
  );
}

/** A title of the request provider: in the library, already requested, or with a button that requests it. */
function ResultCard({
  serverId,
  item,
  onRequested,
}: {
  serverId: string;
  item: RequestMediaResult;
  onRequested: () => void;
}) {
  const { t } = useTranslation();
  const [failed, setFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  // The state the server answered a new request with, until the lists are read again.
  const [made, setMade] = useState<string | null>(null);
  const src = failed ? undefined : poster(serverId, item.poster_path);
  const state = made ?? item.request.state ?? null;
  const inLibrary = item.availability === "available" && item.library_content_id;

  const request = () => {
    setBusy(true);
    setError(null);
    catalog
      .requestCreate(serverId, {
        media_type: item.media_type,
        tmdb_id: item.tmdb_id,
        title: item.title,
        ...(item.year ? { year: item.year } : {}),
        ...(item.overview ? { overview: item.overview } : {}),
        ...(item.poster_path ? { poster_path: item.poster_path } : {}),
        ...(item.backdrop_path ? { backdrop_path: item.backdrop_path } : {}),
      })
      .then((r) => {
        setMade(r.state || "pending");
        onRequested();
      })
      .catch((e: unknown) => {
        setError(e);
      })
      .finally(() => {
        setBusy(false);
      });
  };

  const kind = t(item.media_type === "movie" ? "requests.movie" : "requests.series");
  return (
    <div className="flex w-40 shrink-0 flex-col gap-2">
      <div className="relative overflow-hidden rounded-xl bg-surface-raised outline outline-1 -outline-offset-1 outline-white/[0.07]">
        <div className="aspect-[2/3]">
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
            <div className="flex size-full items-center justify-center p-3 text-center text-sm font-medium text-muted-foreground">
              {item.title}
            </div>
          )}
        </div>
      </div>
      <div className="flex flex-col gap-0.5 px-0.5">
        <p className="truncate text-sm font-semibold" title={item.title}>
          {item.title}
        </p>
        <p className="truncate text-xs text-muted-foreground">
          {[item.year ? String(item.year) : "", kind].filter(Boolean).join(" · ")}
        </p>
      </div>
      {inLibrary ? (
        <Link
          to={itemPath(serverId, item.library_content_id ?? "")}
          draggable={false}
          className="flex items-center gap-1.5 self-start rounded-full bg-emerald-500/15 px-2.5 py-1 text-xs font-semibold text-emerald-300 hover:bg-emerald-500/25"
        >
          <Check className="size-3.5" />
          {t("requests.inLibrary")}
        </Link>
      ) : state ? (
        <StatePill state={state} />
      ) : item.request.requestable ? (
        <button
          type="button"
          disabled={busy}
          onClick={request}
          aria-label={t("requests.requestTitle", { title: item.title })}
          className="flex items-center gap-1.5 self-start rounded-full bg-foreground px-3 py-1 text-xs font-semibold text-background transition hover:opacity-90 disabled:opacity-50"
        >
          <Plus className="size-3.5" />
          {t(busy ? "requests.requesting" : "requests.request")}
        </button>
      ) : (
        <span className="self-start rounded-full bg-surface-hover px-2.5 py-1 text-xs font-medium text-muted-foreground">
          {t("requests.notRequestable")}
        </span>
      )}
      {error ? <p className="text-xs text-red-300">{errorMessage(t, error)}</p> : null}
    </div>
  );
}

function Discover({ serverId, onRequested }: { serverId: string; onRequested: () => void }) {
  const { t } = useTranslation();
  const { data, error } = useLoad(`${serverId}/requests/discover`, () => catalog.requestDiscover(serverId));
  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!data) return <div className="h-72 animate-pulse rounded-2xl bg-card" />;
  if (data.length === 0) return <p className="text-muted-foreground">{t("requests.discoverEmpty")}</p>;
  return (
    <div className="flex flex-col gap-10">
      {data.map((section) => (
        <Row key={section.key} title={section.title}>
          {section.results.map((x) => (
            <ResultCard
              key={`${x.media_type}-${String(x.tmdb_id)}`}
              serverId={serverId}
              item={x}
              onRequested={onRequested}
            />
          ))}
        </Row>
      ))}
    </div>
  );
}

function SearchTab({ serverId, onRequested }: { serverId: string; onRequested: () => void }) {
  const { t } = useTranslation();
  const [text, setText] = useState("");
  const [query, setQuery] = useState("");
  // The search runs once typing pauses.
  useEffect(() => {
    const id = window.setTimeout(() => {
      setQuery(text.trim());
    }, 350);
    return () => {
      window.clearTimeout(id);
    };
  }, [text]);
  const { data, error } = useLoad(query ? `${serverId}/requests/search/${query}` : null, () =>
    catalog.requestSearch(serverId, query),
  );
  return (
    <div className="flex flex-col gap-6">
      <label className="flex max-w-xl items-center gap-2 rounded-full border border-border bg-card px-4 py-2 focus-within:ring-2 focus-within:ring-ring">
        <Search className="size-4 shrink-0 text-muted-foreground" />
        <input
          type="search"
          autoFocus
          value={text}
          onChange={(e) => {
            setText(e.target.value);
          }}
          placeholder={t("requests.searchPlaceholder")}
          aria-label={t("requests.searchPlaceholder")}
          className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </label>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {query && !data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {!query ? <p className="text-sm text-muted-foreground">{t("requests.searchHint")}</p> : null}
      {query && data?.results.length === 0 ? (
        <p className="text-muted-foreground">{t("requests.searchEmpty", { query })}</p>
      ) : null}
      {query && data && data.results.length > 0 ? (
        <div className={GRID}>
          {data.results.map((x) => (
            <ResultCard
              key={`${x.media_type}-${String(x.tmdb_id)}`}
              serverId={serverId}
              item={x}
              onRequested={onRequested}
            />
          ))}
        </div>
      ) : null}
    </div>
  );
}

function Mine({ serverId, nonce, onChanged }: { serverId: string; nonce: number; onChanged: () => void }) {
  const { t, i18n } = useTranslation();
  const { data, error } = useLoad(`${serverId}/requests/mine`, () => catalog.requestList(serverId), nonce);
  const [busy, setBusy] = useState<string | null>(null);
  const [failure, setFailure] = useState<unknown>(null);
  const cancel = (r: TitleRequest) => {
    setBusy(r.id);
    setFailure(null);
    catalog
      .requestCancel(serverId, r.id)
      .then(onChanged)
      .catch((e: unknown) => {
        setFailure(e);
      })
      .finally(() => {
        setBusy(null);
      });
  };
  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!data) return <div className="h-72 animate-pulse rounded-2xl bg-card" />;
  if (data.length === 0)
    return (
      <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-2xl bg-card px-6 py-12 text-center">
        <Inbox className="size-10 text-muted-foreground" strokeWidth={1.5} />
        <p className="text-sm text-muted-foreground">{t("requests.mineEmpty")}</p>
      </div>
    );
  return (
    <div className="flex flex-col gap-3">
      {failure ? <Alert>{errorMessage(t, failure)}</Alert> : null}
      <ul className="flex flex-col gap-2">
        {data.map((r) => {
          const src = poster(serverId, r.poster_path);
          const date = new Date(r.created_at);
          const details = [
            r.year ? String(r.year) : "",
            t(r.media_type === "movie" ? "requests.movie" : "requests.series"),
            r.seasons.length > 0 ? t("requests.seasons", { seasons: r.seasons.join(", ") }) : "",
            Number.isNaN(date.getTime())
              ? ""
              : t("requests.requestedOn", { date: date.toLocaleDateString(i18n.language, { dateStyle: "medium" }) }),
          ].filter(Boolean);
          return (
            <li key={r.id} className="flex items-center gap-4 rounded-2xl bg-card p-3">
              <div className="aspect-[2/3] w-12 shrink-0 overflow-hidden rounded-md bg-surface-raised">
                {src ? <img src={src} alt="" loading="lazy" className="size-full object-cover" /> : null}
              </div>
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                {r.library_content_id ? (
                  <Link
                    to={itemPath(serverId, r.library_content_id)}
                    className="truncate font-semibold hover:underline"
                  >
                    {r.title}
                  </Link>
                ) : (
                  <p className="truncate font-semibold">{r.title}</p>
                )}
                <p className="truncate text-xs text-muted-foreground">{details.join(" · ")}</p>
                {r.outcome_reason ? <p className="truncate text-xs text-muted-foreground">{r.outcome_reason}</p> : null}
              </div>
              {r.download?.percent != null && r.state === "processing" ? (
                <span className="text-xs text-muted-foreground tabular-nums">{String(r.download.percent)} %</span>
              ) : null}
              <StatePill state={r.state} inRow />
              {OPEN.includes(r.state) ? (
                <button
                  type="button"
                  disabled={busy === r.id}
                  onClick={() => {
                    cancel(r);
                  }}
                  aria-label={t("requests.cancelTitle", { title: r.title })}
                  className="rounded-full border border-border px-3 py-1 text-xs font-semibold transition hover:bg-surface-hover disabled:opacity-50"
                >
                  {t("requests.cancel")}
                </button>
              ) : null}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/**
 * Requests: ask the server for titles it does not have (`createRequest`), find them in the provider's rows
 * (`listDiscoverSections`) or by search (`searchRequestMedia`), and follow the account's own requests
 * (`listMyRequests`, `cancelRequest`). The web app's pages are the model
 * (`silo-server/web/src/pages/Requests.tsx@749db96a`); its title detail, season choice, follow and browse-by-genre
 * pages are left out. A series request covers every aired season the library does not have in full.
 */
export function Requests() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const [tab, setTab] = useState<Tab>("discover");
  // Bumped when a request is made or withdrawn here, so "My requests" is read again.
  const [changes, setChanges] = useState(0);
  const refresh = useRefreshTick();
  const { data: status, error } = useLoad(`${serverId}/requests/status`, () => catalog.requestStatus(serverId));

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const changed = () => {
    setChanges((n) => n + 1);
  };
  const usable = status?.state === "available" && status.requests_enabled && status.allowed;
  const why = !status
    ? null
    : status.state === "not_configured" || status.state === "unsupported"
      ? "requests.notConfigured"
      : status.state === "disabled" || !status.requests_enabled
        ? "requests.disabled"
        : !status.allowed
          ? "requests.notAllowed"
          : null;

  return (
    <section className="flex flex-col gap-6" aria-label={t("requests.title")}>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-bold tracking-tight">{t("requests.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("requests.subtitle")}</p>
        </div>
        {usable ? (
          <div role="tablist" className="flex gap-1 rounded-full border border-border p-1">
            {TABS.map((x) => (
              <button
                key={x}
                type="button"
                role="tab"
                aria-selected={tab === x}
                onClick={() => {
                  setTab(x);
                }}
                className={`rounded-full px-3 py-1 text-sm font-medium transition ${
                  tab === x ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground"
                }`}
              >
                {t(`requests.tab.${x}`)}
              </button>
            ))}
          </div>
        ) : null}
      </div>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {!status && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {why ? (
        <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-2xl bg-card px-6 py-12 text-center">
          <Inbox className="size-10 text-muted-foreground" strokeWidth={1.5} />
          <p className="max-w-md text-sm text-muted-foreground">{t(why)}</p>
        </div>
      ) : null}
      {usable && tab === "discover" ? <Discover serverId={serverId} onRequested={changed} /> : null}
      {usable && tab === "search" ? <SearchTab serverId={serverId} onRequested={changed} /> : null}
      {usable && tab === "mine" ? <Mine serverId={serverId} nonce={changes + refresh} onChanged={changed} /> : null}
    </section>
  );
}
