import { CalendarDays, Check, ChevronLeft, ChevronRight } from "lucide-react";
import { useEffect, useState } from "react";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams, useSearchParams } from "react-router";
import { Row } from "../components/Row";
import { Alert } from "../components/ui";
import { artwork, catalog, type CalendarDay, type CalendarEvent, type CalendarFilter } from "../lib/catalog";
import { addDays, isDay, isoDate, parseDay, weekDays, weekRange, weekStart } from "../lib/calendarWeek";
import { errorMessage } from "../lib/errors";
import { retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, itemPath, needsReauth } from "../lib/nav";
import { useLoad } from "../lib/useLoad";

const FILTERS: CalendarFilter[] = ["following", "trending", "everything"];
const STORED_FILTER = "granary.calendar.filter";
const BADGES = ["series_premiere", "season_premiere", "finale", "digital", "physical"];

function storedFilter(): CalendarFilter {
  try {
    const v = localStorage.getItem(STORED_FILTER);
    return FILTERS.find((f) => f === v) ?? "following";
  } catch {
    return "following";
  }
}

/** "S2 · E1 · Premiere" for an episode, "Season 2" for a season, "Film" for a movie. */
function eventLine(t: TFunction, e: CalendarEvent): string {
  if (e.type === "movie") return t("calendar.movie");
  if (e.season_number == null) return e.episode_title ?? "";
  const code =
    e.episode_number == null
      ? t("calendar.season", { season: e.season_number })
      : t("calendar.episode", { season: e.season_number, episode: e.episode_number });
  return e.episode_title ? `${code} · ${e.episode_title}` : code;
}

/** The airing time on the viewer's clock when the server knows the instant, else the network's wall-clock time. */
function eventTime(e: CalendarEvent, locale: string): string {
  if (e.air_at) {
    const d = new Date(e.air_at);
    if (!Number.isNaN(d.getTime()))
      return d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
  }
  return e.air_time ?? "";
}

function EventCard({ serverId, event }: { serverId: string; event: CalendarEvent }) {
  const { t, i18n } = useTranslation();
  const [failed, setFailed] = useState(false);
  const src = failed ? undefined : artwork(serverId, event.poster_url);
  // An episode opens its series: an episode that has not aired has no page of its own.
  const to = itemPath(serverId, event.type === "movie" ? event.content_id : (event.series_id ?? event.content_id));
  const line = eventLine(t, event);
  const time = eventTime(event, i18n.language);
  return (
    <div className="media-card group/card flex w-40 shrink-0 flex-col gap-2">
      <Link
        to={to}
        draggable={false}
        aria-label={[event.title, line, time].filter(Boolean).join(", ")}
        className="media-card-image relative block overflow-hidden rounded-xl bg-surface-raised ring-ring outline outline-1 -outline-offset-1 outline-white/[0.07] focus-visible:ring-2 focus-visible:outline-none"
      >
        <div className={`aspect-[2/3] ${event.watched ? "opacity-50 grayscale" : ""}`}>
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
              {event.title}
            </div>
          )}
        </div>
        {event.badges.length > 0 ? (
          <div className="absolute top-2 left-2 flex flex-wrap gap-1">
            {event.badges.map((b) => (
              <span
                key={b}
                className="rounded-full bg-black/75 px-2 py-0.5 text-[0.625rem] leading-none font-semibold tracking-wide text-white uppercase"
              >
                {BADGES.includes(b) ? t(`calendar.badge.${b}`) : b}
              </span>
            ))}
          </div>
        ) : null}
        {event.watched ? (
          <span
            className="absolute top-2 right-2 flex size-6 items-center justify-center rounded-full bg-black/75 text-white"
            aria-label={t("calendar.watched")}
          >
            <Check className="size-3.5" />
          </span>
        ) : null}
      </Link>
      <div className="flex flex-col gap-0.5 px-0.5">
        <p className={`truncate text-sm font-semibold ${event.watched ? "text-muted-foreground" : ""}`}>
          {event.title}
        </p>
        {line ? <p className="truncate text-xs text-muted-foreground">{line}</p> : null}
        {time ? <p className="text-xs text-muted-foreground tabular-nums">{time}</p> : null}
      </div>
    </div>
  );
}

/**
 * The airing calendar (`getCalendar`): a Monday-first week of the viewer's local days, each day with what airs or is
 * released on it. "Following" is what the profile watches or has saved, "Trending" what is popular right now,
 * "Everything" all of the server's titles. The web app's page is the model
 * (`silo-server/web/src/pages/Calendar.tsx@749db96a`); its per-library filter is left out.
 */
export function Calendar() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const [search, setSearch] = useSearchParams();
  const today = isoDate(new Date());
  const weekParam = search.get("week");
  const week = isDay(weekParam) ? weekParam : weekStart(new Date());
  const [filter, setFilter] = useState<CalendarFilter>(storedFilter);
  const [picked, setPicked] = useState<string | null>(null);
  const refresh = useRefreshTick();
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  const { data, error } = useLoad(
    `${serverId}/calendar/${week}/${filter}/${zone}`,
    () => catalog.calendar(serverId, week, addDays(week, 6), filter, zone),
    refresh,
  );

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const days = weekDays(week);
  const byDay = new Map((data ?? []).map((d: CalendarDay) => [d.date, d]));
  const selected = picked && days.includes(picked) ? picked : null;
  const goWeek = (w: string) => {
    setSearch(w === weekStart(new Date()) ? {} : { week: w }, { replace: true });
  };
  const choose = (f: CalendarFilter) => {
    setFilter(f);
    try {
      localStorage.setItem(STORED_FILTER, f);
    } catch {
      // The choice then lasts for this visit only.
    }
  };
  const pick = (day: string) => {
    setPicked(day);
    requestAnimationFrame(() => {
      const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
      document.getElementById(`day-${day}`)?.scrollIntoView({ behavior: reduce ? "auto" : "smooth", block: "start" });
    });
  };
  const heading = (day: string) => {
    const name = parseDay(day).toLocaleDateString(i18n.language, { weekday: "long", day: "numeric", month: "long" });
    return day === today ? `${t("calendar.today")} · ${name}` : name;
  };
  const range = weekRange(week, i18n.language);
  const round =
    "flex size-8 items-center justify-center rounded-full text-muted-foreground transition hover:bg-surface-hover hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring";

  return (
    <section className="flex flex-col gap-6" aria-label={t("calendar.title")}>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-bold tracking-tight">{t("calendar.title")}</h1>
          <p className="text-sm text-muted-foreground">{t(`calendar.subtitle.${filter}`)}</p>
        </div>
        <div
          role="tablist"
          aria-label={t("calendar.show")}
          className="flex gap-1 rounded-full border border-border p-1"
        >
          {FILTERS.map((f) => (
            <button
              key={f}
              type="button"
              role="tab"
              aria-selected={filter === f}
              onClick={() => {
                choose(f);
              }}
              className={`rounded-full px-3 py-1 text-sm font-medium transition ${
                filter === f ? "bg-foreground text-background" : "text-muted-foreground hover:text-foreground"
              }`}
            >
              {t(`calendar.filter.${f}`)}
            </button>
          ))}
        </div>
      </div>

      {/* Pinned as a full-width band from the window's top edge: nothing scrolls visibly above or beside the week
          card, and it gets the same breathing room at the top as at rest. */}
      <div className="sticky top-0 z-10 -mx-4 bg-background px-4 py-3 sm:-mx-6 sm:px-6 lg:-mx-10 lg:px-10 xl:-mx-12 xl:px-12">
        <div
          aria-hidden
          className="pointer-events-none absolute inset-x-0 top-full h-6 bg-gradient-to-b from-background to-transparent"
        />
        <nav
          aria-label={t("calendar.weekOf", { range })}
          className="flex flex-col gap-2 rounded-2xl border border-border/70 bg-card p-3"
        >
          <div className="flex items-center justify-between gap-2">
            <div className="flex items-center gap-1">
              <button
                type="button"
                className={round}
                aria-label={t("calendar.previousWeek")}
                onClick={() => {
                  goWeek(addDays(week, -7));
                }}
              >
                <ChevronLeft className="size-5" />
              </button>
              <p className="min-w-40 text-center text-sm font-semibold tabular-nums">{range}</p>
              <button
                type="button"
                className={round}
                aria-label={t("calendar.nextWeek")}
                onClick={() => {
                  goWeek(addDays(week, 7));
                }}
              >
                <ChevronRight className="size-5" />
              </button>
            </div>
            <button
              type="button"
              className="rounded-full border border-border px-3 py-1 text-xs font-semibold transition hover:bg-surface-hover"
              onClick={() => {
                goWeek(weekStart(new Date()));
                setPicked(null);
              }}
            >
              {t("calendar.thisWeek")}
            </button>
          </div>
          <div className="grid grid-cols-7 gap-1.5">
            {days.map((day) => {
              const d = parseDay(day);
              const count = byDay.get(day)?.items.length ?? 0;
              const on = selected === day;
              return (
                <button
                  key={day}
                  type="button"
                  aria-pressed={on}
                  aria-current={day === today ? "date" : undefined}
                  aria-label={t("calendar.dayLabel", {
                    count,
                    day: d.toLocaleDateString(i18n.language, { weekday: "long", day: "numeric", month: "long" }),
                  })}
                  onClick={() => {
                    pick(day);
                  }}
                  className={`flex flex-col items-center gap-0.5 rounded-xl px-1 py-2 transition focus-visible:outline-2 focus-visible:outline-ring ${
                    on
                      ? "bg-foreground text-background"
                      : day === today
                        ? "bg-surface-hover text-foreground ring-1 ring-border"
                        : "text-muted-foreground hover:bg-surface-hover hover:text-foreground"
                  }`}
                >
                  <span className="text-[0.6875rem] font-medium tracking-wide uppercase">
                    {d.toLocaleDateString(i18n.language, { weekday: "short" })}
                  </span>
                  <span className={`text-base font-bold tabular-nums ${on ? "" : "text-foreground"}`}>
                    {d.getDate()}
                  </span>
                  <span
                    aria-hidden
                    className={`size-1 rounded-full ${count > 0 ? (on ? "bg-background" : "bg-muted-foreground") : "opacity-0"}`}
                  />
                </button>
              );
            })}
          </div>
        </nav>
      </div>

      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {!data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {data && selected && !byDay.has(selected) && data.length > 0 ? (
        <p role="status" className="rounded-2xl bg-card px-4 py-3 text-sm text-muted-foreground">
          {t("calendar.nothingOn", { day: heading(selected) })}
        </p>
      ) : null}
      {data && data.length === 0 ? (
        <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-2xl bg-card px-6 py-12 text-center">
          <CalendarDays className="size-10 text-muted-foreground" strokeWidth={1.5} />
          <p className="text-sm text-muted-foreground">{t(`calendar.empty.${filter}`)}</p>
          <div className="flex flex-wrap justify-center gap-4">
            {FILTERS.filter((f) => f !== filter).map((f) => (
              <button
                key={f}
                type="button"
                className="text-sm font-medium underline-offset-4 hover:underline"
                onClick={() => {
                  choose(f);
                }}
              >
                {t(`calendar.filter.${f}`)}
              </button>
            ))}
          </div>
        </div>
      ) : null}
      {(data ?? []).map((day) => (
        <div key={day.date} id={`day-${day.date}`} className="scroll-mt-40">
          <Row title={heading(day.date)}>
            {day.items.map((e) => (
              <EventCard key={e.content_id} serverId={serverId} event={e} />
            ))}
          </Row>
        </div>
      ))}
    </section>
  );
}
