import { Check, Play, RotateCcw } from "lucide-react";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link } from "react-router";
import { Alert } from "../components/ui";
import { bookChapters, bookDuration, bookParts } from "../lib/audiobook";
import { artwork, catalog, type AudiobookRelatedItem, type CatalogItemDetail } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { itemPath, personPath } from "../lib/nav";
import { formatTime, resumePosition } from "../lib/play";
import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";
import type { LaunchArt } from "../player/context";

/** How many chapters show before "Show all". */
const CHAPTERS_COLLAPSED = 8;

/** "11h 32m" / "48m": hours and minutes, the web's short book length. */
function hoursMinutes(seconds: number): string {
  const total = Math.max(0, Math.round(seconds / 60));
  const h = Math.floor(total / 60);
  const m = total % 60;
  if (h === 0) return `${String(m)}m`;
  return m > 0 ? `${String(h)}h ${String(m)}m` : `${String(h)}h`;
}

function Cover({ src, title, className }: { src: string | undefined; title: string; className: string }) {
  const [failed, setFailed] = useState(false);
  return (
    <div className={`aspect-square overflow-hidden bg-surface-raised ${className}`}>
      {src && !failed ? (
        <img
          src={src}
          alt=""
          draggable={false}
          className="size-full object-cover"
          onError={() => {
            setFailed(true);
          }}
        />
      ) : (
        <div className="flex size-full items-center justify-center p-3 text-center text-sm font-semibold text-muted-foreground">
          {title}
        </div>
      )}
    </div>
  );
}

function People({ serverId, people }: { serverId: string; people: { name: string; person_id?: string | null }[] }) {
  return (
    <>
      {people.map((p, i) => (
        <span key={`${p.name}-${String(i)}`}>
          {i > 0 ? ", " : ""}
          {p.person_id ? (
            <Link to={personPath(serverId, p.person_id)} className="font-semibold text-foreground hover:underline">
              {p.name}
            </Link>
          ) : (
            <span className="font-semibold text-foreground">{p.name}</span>
          )}
        </span>
      ))}
    </>
  );
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="truncate text-[15px] font-semibold">{children}</dd>
    </div>
  );
}

/** A rail of square book covers (series, same author, similar), as the web's book page shows them. */
function BookRail({
  serverId,
  heading,
  meta,
  items,
  current,
}: {
  serverId: string;
  heading: string;
  meta?: string | undefined;
  items: AudiobookRelatedItem[];
  current?: string;
}) {
  const { t } = useTranslation();
  if (items.length === 0) return null;
  return (
    <section>
      <div className="mb-4 flex items-baseline justify-between gap-3">
        <h2 className="text-xl font-semibold tracking-tight">{heading}</h2>
        {meta ? <span className="text-sm text-muted-foreground">{meta}</span> : null}
      </div>
      <div className="-mx-2 -mt-1 flex gap-4 overflow-x-auto px-2 pt-1 pb-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        {items.map((b) => {
          const here = b.content_id === current;
          const progress = !b.played && b.progress_ratio ? Math.min(1, b.progress_ratio) : 0;
          return (
            <Link
              key={b.content_id}
              to={itemPath(serverId, b.content_id)}
              aria-current={here ? "page" : undefined}
              className="group/related block w-40 shrink-0 focus-visible:outline-none"
            >
              <div
                className={`relative overflow-hidden rounded-lg transition-[filter] group-hover/related:brightness-110 group-focus-visible/related:ring-2 group-focus-visible/related:ring-ring ${
                  here ? "ring-2 ring-primary ring-offset-2 ring-offset-background" : ""
                }`}
              >
                <Cover src={artwork(serverId, b.poster_url)} title={b.title} className="rounded-lg" />
                {b.played ? (
                  <span className="absolute top-2 right-2 flex size-6 items-center justify-center rounded-full bg-black/60 text-white">
                    <Check className="size-3.5" aria-label={t("browse.watched")} />
                  </span>
                ) : null}
                {progress > 0 ? (
                  <span className="absolute inset-x-2 bottom-2 h-1 overflow-hidden rounded-full bg-white/25">
                    <span className="block h-full bg-primary" style={{ width: `${String(progress * 100)}%` }} />
                  </span>
                ) : null}
              </div>
              {typeof b.series_index === "number" ? (
                <div className="mt-2 truncate text-xs text-muted-foreground">
                  {t("book.bookN", { n: b.series_index })}
                </div>
              ) : null}
              <div
                className={`line-clamp-2 text-sm leading-snug font-semibold ${typeof b.series_index === "number" ? "mt-0.5" : "mt-2"}`}
              >
                {b.title}
              </div>
              {b.duration_seconds || b.year ? (
                <div className="mt-0.5 truncate text-xs text-muted-foreground">
                  {[b.duration_seconds ? hoursMinutes(b.duration_seconds) : null, b.year ? String(b.year) : null]
                    .filter(Boolean)
                    .join(" · ")}
                </div>
              ) : null}
            </Link>
          );
        })}
      </div>
    </section>
  );
}

/**
 * An audiobook's page in the web's layout (`silo-server/web/src/pages/ItemDetail/AudiobookContent.tsx@749db96a`): the
 * square cover with Listen/Resume and progress in a sticky column, the series, title, author and narrator beside it,
 * then the facts row, the overview, genres, the chapters (each starts the book there) and rails of the series, the
 * author's other books and similar ones. The cover's colours tint the top of the page (there is no backdrop art).
 */
export function AudiobookDetail({
  serverId,
  item,
  actions,
}: {
  serverId: string;
  item: CatalogItemDetail;
  actions: ReactNode;
}) {
  const { t } = useTranslation();
  const refresh = useRefreshTick();
  const { data: watch, error: loadError } = useLoad(
    `${serverId}/${item.content_id}`,
    () => catalog.watch(serverId, item.content_id),
    refresh,
  );
  const { play, busy, error: playError } = usePlay(serverId);
  const [allChapters, setAllChapters] = useState(false);
  const [moreOverview, setMoreOverview] = useState(false);
  const book = item.audiobook;
  const longOverview = (item.overview?.length ?? 0) > 420;
  const cover = artwork(serverId, item.poster_url);
  const art: LaunchArt = { backdrop: item.backdrop_url ?? null, logo: null, poster: item.poster_url ?? null };

  const parts = watch ? bookParts(watch.versions) : [];
  const total = book?.total_duration_seconds || bookDuration(parts);
  const chapters = watch ? bookChapters(watch.versions, parts, (n) => t("book.chapterN", { n })) : [];
  const resume = watch ? resumePosition(watch) : 0;
  const played = item.user_state?.played === true;
  const progress = resume > 0 && total > 0 ? Math.min(1, resume / total) : 0;
  const current = resume > 0 ? chapters.findLastIndex((c) => c.start <= resume) : -1;
  const seriesEntries = book?.series?.entries ?? [];
  const seriesIndex = seriesEntries.find((e) => e.content_id === item.content_id)?.series_index;
  const author = book?.authors[0]?.name;
  const playable = watch !== undefined && watch.versions.length > 0;

  // Collapsed, the list keeps the chapter playing in view.
  let visible = chapters.map((c, i) => ({ c, i }));
  const collapsible = chapters.length > CHAPTERS_COLLAPSED + 2;
  if (collapsible && !allChapters) {
    const start = Math.max(0, Math.min(current - 2, chapters.length - CHAPTERS_COLLAPSED));
    visible = visible.slice(start, start + CHAPTERS_COLLAPSED);
  }
  const chapterEnd = (i: number) => chapters[i + 1]?.start ?? total;

  const listen = (opts: { fromStart?: boolean; startAt?: number }) => {
    if (!watch) return;
    void play(item.content_id, { watch, art, fromStart: opts.fromStart ?? resume <= 0, ...opts });
  };

  return (
    <div className="relative isolate">
      {/* The cover's colours, blurred, tint the top of the page. */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-x-0 top-0 -z-10 h-[min(42rem,95vh)] overflow-hidden [mask-image:linear-gradient(to_bottom,transparent,black_18%,black_45%,transparent)]"
      >
        {cover ? (
          <img
            src={cover}
            alt=""
            className="absolute -top-1/4 left-0 h-[150%] w-[70%] scale-125 object-cover opacity-40 blur-3xl"
          />
        ) : null}
        <div className="absolute inset-0 bg-gradient-to-b from-background/30 via-background/70 to-background" />
      </div>

      <div className="grid grid-cols-[7rem_minmax(0,1fr)] items-end gap-x-4 gap-y-5 pt-6 sm:grid-cols-[11rem_minmax(0,1fr)] sm:gap-x-8 sm:gap-y-6 lg:grid-cols-[16.25rem_minmax(0,1fr)] lg:items-start lg:gap-x-14 lg:gap-y-8 lg:pt-10">
        {/* Side: cover and listening controls (a sticky column from lg). */}
        <div className="contents lg:sticky lg:top-20 lg:row-span-2 lg:grid lg:gap-4 lg:self-start">
          <Cover
            src={cover}
            title={item.title}
            className="rounded-xl shadow-[0_24px_48px_-20px_rgb(0_0_0/0.85)] outline outline-1 -outline-offset-1 outline-white/10 lg:rounded-2xl"
          />
          <div className="col-span-2 row-start-2 grid grid-cols-[minmax(0,1fr)] gap-3 lg:col-span-1 lg:row-start-auto">
            {loadError ? (
              <Alert onRetry={retryLoads}>{errorMessage(t, loadError)}</Alert>
            ) : !watch ? (
              <div className="h-11 w-full animate-pulse rounded-full bg-white/10" />
            ) : !playable ? (
              <Alert tone="warning">{t("item.noVersions")}</Alert>
            ) : (
              <>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => {
                    listen({});
                  }}
                  className="inline-flex h-11 w-full items-center justify-center gap-2 rounded-full bg-primary px-5 font-semibold text-primary-foreground shadow-lg transition hover:brightness-110 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-60"
                >
                  <Play className="size-4 shrink-0 fill-current" />
                  <span className="truncate">
                    {resume > 0 ? `${t("book.resumeListening")} · ${formatTime(resume)}` : t("book.listen")}
                  </span>
                </button>
                {progress > 0 && !played ? (
                  <div>
                    <div className="h-1.5 w-full overflow-hidden rounded-full bg-foreground/15">
                      <div className="h-full rounded-full bg-primary" style={{ width: `${String(progress * 100)}%` }} />
                    </div>
                    <div className="mt-1.5 flex items-baseline justify-between gap-3 text-xs">
                      <span className="truncate text-muted-foreground">
                        {t("book.listened", { percent: Math.round(progress * 100) })}
                      </span>
                      <span className="shrink-0 font-semibold text-foreground">
                        {t("book.timeLeft", { time: hoursMinutes(total - resume) })}
                      </span>
                    </div>
                  </div>
                ) : null}
                <div className="flex flex-wrap items-center gap-2">
                  {resume > 0 ? (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        listen({ fromStart: true });
                      }}
                      className="inline-flex h-10 min-w-0 flex-1 items-center justify-center gap-2 rounded-full border border-border bg-secondary/60 px-4 text-sm font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring disabled:opacity-60"
                    >
                      <RotateCcw className="size-4 shrink-0" />
                      <span className="truncate">{t("book.fromStart")}</span>
                    </button>
                  ) : null}
                  {actions}
                </div>
              </>
            )}
            {playError ? <Alert>{playError}</Alert> : null}
          </div>
        </div>

        {/* Head: series, title, author, narrator. */}
        <header className="grid min-w-0 grid-cols-[minmax(0,1fr)] gap-1.5 lg:gap-2 lg:pt-3">
          {book?.series?.name ? (
            <span className="w-fit text-sm font-semibold text-foreground/80">
              {typeof seriesIndex === "number"
                ? t("book.seriesBook", { series: book.series.name, n: seriesIndex })
                : book.series.name}
            </span>
          ) : null}
          <h1 className="text-2xl leading-[1.08] font-extrabold tracking-[-0.03em] text-balance sm:text-4xl lg:mb-1 lg:text-5xl">
            {item.title}
          </h1>
          {book && book.authors.length > 0 ? (
            <p className="text-sm text-foreground/80 sm:text-base">
              {t("book.by")} <People serverId={serverId} people={book.authors} />
            </p>
          ) : null}
          {book && book.narrators.length > 0 ? (
            <p className="text-sm text-foreground/80 sm:text-base">
              {t("book.narratedByLead")} <People serverId={serverId} people={book.narrators} />
            </p>
          ) : null}
        </header>

        {/* Body. */}
        <div className="col-span-2 grid min-w-0 grid-cols-[minmax(0,1fr)] gap-10 self-start lg:col-span-1 lg:col-start-2 lg:gap-12">
          <div className="grid grid-cols-[minmax(0,1fr)] gap-6">
            <dl className="flex flex-wrap gap-x-9 gap-y-3 border-y border-border/60 py-4">
              {total > 0 ? <Fact label={t("book.length")}>{hoursMinutes(total)}</Fact> : null}
              {chapters.length > 0 ? <Fact label={t("book.chaptersHeading")}>{chapters.length}</Fact> : null}
              {item.year ? <Fact label={t("book.released")}>{item.year}</Fact> : null}
              {book?.publisher ? <Fact label={t("book.publisher")}>{book.publisher}</Fact> : null}
            </dl>
            {item.overview ? (
              <div className="max-w-[68ch]">
                <p
                  className={`text-[15px] leading-relaxed whitespace-pre-line text-foreground/90 ${longOverview && !moreOverview ? "line-clamp-5" : ""}`}
                >
                  {item.overview}
                </p>
                {longOverview ? (
                  <button
                    type="button"
                    aria-expanded={moreOverview}
                    onClick={() => {
                      setMoreOverview(!moreOverview);
                    }}
                    className="mt-2 text-sm font-semibold text-muted-foreground transition-colors hover:text-foreground"
                  >
                    {moreOverview ? t("person.less") : t("person.more")}
                  </button>
                ) : null}
              </div>
            ) : null}
            {item.genres.length > 0 ? (
              <div className="flex flex-wrap gap-2">
                {item.genres.map((g) => (
                  <span
                    key={g}
                    className="rounded-full border border-border/70 bg-secondary/50 px-3 py-1 text-xs font-medium text-foreground/85"
                  >
                    {g}
                  </span>
                ))}
              </div>
            ) : null}
            {book?.other_narrations && book.other_narrations.length > 0 ? (
              <div className="text-sm text-muted-foreground">
                <span className="mr-2">{t("book.otherNarrations")}</span>
                {book.other_narrations.map((n, i) => (
                  <span key={n.content_id}>
                    {i > 0 ? " · " : ""}
                    <Link to={itemPath(serverId, n.content_id)} className="font-medium text-foreground hover:underline">
                      {n.narrators.join(", ") || n.title}
                    </Link>
                    {n.year ? ` (${String(n.year)})` : ""}
                  </span>
                ))}
              </div>
            ) : null}
          </div>

          {chapters.length > 0 && playable ? (
            <section>
              <div className="mb-3 flex items-baseline justify-between gap-3">
                <h2 className="text-xl font-semibold tracking-tight">{t("book.chaptersHeading")}</h2>
                {collapsible ? (
                  <button
                    type="button"
                    aria-expanded={allChapters}
                    onClick={() => {
                      setAllChapters(!allChapters);
                    }}
                    className="text-sm font-semibold text-muted-foreground transition-colors hover:text-foreground"
                  >
                    {allChapters ? t("book.showFewer") : t("book.showAll", { count: chapters.length })}
                  </button>
                ) : null}
              </div>
              <ol className="grid gap-0.5">
                {visible.map(({ c, i }) => {
                  const isCurrent = i === current;
                  const done = current >= 0 && i < current;
                  const length = Math.max(0, chapterEnd(i) - c.start);
                  const left = isCurrent ? Math.max(0, chapterEnd(i) - resume) : 0;
                  return (
                    <li key={`${String(c.part)}-${String(c.start)}`}>
                      <button
                        type="button"
                        disabled={busy}
                        aria-current={isCurrent ? "step" : undefined}
                        onClick={() => {
                          listen({ startAt: c.start });
                        }}
                        className={`grid w-full grid-cols-[2rem_minmax(0,1fr)_auto_auto] items-center gap-4 rounded-lg px-3 py-2.5 text-left transition-colors focus-visible:outline-2 focus-visible:outline-ring ${
                          isCurrent ? "bg-primary/15 hover:bg-primary/20" : "hover:bg-muted/50"
                        }`}
                      >
                        <span
                          className={`text-right text-xs tabular-nums ${isCurrent ? "font-semibold text-foreground" : "text-muted-foreground"}`}
                        >
                          {i + 1}
                        </span>
                        <span className="min-w-0">
                          <span
                            className={`block truncate text-sm ${done ? "text-muted-foreground" : "font-medium"} ${isCurrent ? "font-semibold" : ""}`}
                          >
                            {c.title}
                          </span>
                          {isCurrent && left > 0 ? (
                            <span className="mt-0.5 block text-xs text-muted-foreground">
                              {t("book.leftInChapter", { time: hoursMinutes(left) })}
                            </span>
                          ) : null}
                        </span>
                        <span className="text-xs text-muted-foreground tabular-nums">{formatTime(c.start)}</span>
                        <span className="flex items-center justify-end gap-1.5 text-xs text-muted-foreground tabular-nums">
                          {done ? <Check className="size-3.5 text-emerald-400" aria-hidden="true" /> : null}
                          {length > 0 ? hoursMinutes(length) : ""}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ol>
            </section>
          ) : null}

          <BookRail
            serverId={serverId}
            heading={book?.series?.name ? t("book.inSeries", { series: book.series.name }) : t("book.inThisSeries")}
            meta={seriesEntries.length > 1 ? t("book.booksCount", { count: seriesEntries.length }) : undefined}
            items={seriesEntries}
            current={item.content_id}
          />
          <BookRail
            serverId={serverId}
            heading={author ? t("book.alsoByAuthor", { author }) : t("book.alsoByThisAuthor")}
            items={book?.related?.also_by_author ?? []}
          />
          <BookRail serverId={serverId} heading={t("book.youMightAlsoLike")} items={book?.related?.similar ?? []} />
        </div>
      </div>
    </div>
  );
}
