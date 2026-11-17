import type { TFunction } from "i18next";
import { BellOff, CheckCheck } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router";
import { Alert } from "../components/ui";
import { artwork, catalog, type NotificationItem } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { bumpRefresh, retryLoads, useRefreshTick } from "../lib/events";
import { goReauth, itemPath, needsReauth, serverBase } from "../lib/nav";
import { useLoad } from "../lib/useLoad";

const flag = (n: NotificationItem, key: string): string => {
  const v = n.reason_flags?.[key];
  return typeof v === "string" ? v : "";
};

/** "S1 · E3", or nothing when the entry names no episode. */
function episodeCode(t: TFunction, n: NotificationItem): string {
  if (n.season_number == null || n.episode_number == null) return "";
  return t("calendar.episode", { season: n.season_number, episode: n.episode_number });
}

/** The headline: the series or requested title; a type this app does not know gets a general one. */
function notificationTitle(t: TFunction, n: NotificationItem): string {
  switch (n.type) {
    case "episode.available":
      return n.series_title || t("notifications.newEpisode");
    case "request.fulfilled":
      return n.series_title || flag(n, "title") || t("notifications.request");
    case "request.approved":
    case "request.declined":
      return flag(n, "title") || t("notifications.request");
    default:
      return t("notifications.generic");
  }
}

function notificationText(t: TFunction, n: NotificationItem): string {
  switch (n.type) {
    case "episode.available": {
      const code = episodeCode(t, n);
      return [code, n.episode_title].filter(Boolean).join(" · ") || t("notifications.newEpisode");
    }
    case "request.fulfilled":
      return t("notifications.fulfilled");
    case "request.approved":
      return t("notifications.approved");
    case "request.declined": {
      const reason = flag(n, "reason");
      return reason ? t("notifications.declinedReason", { reason }) : t("notifications.declined");
    }
    default:
      return n.type;
  }
}

/** Where an entry leads: the episode or series in the library, else the Requests page for a request's answer. */
function target(serverId: string, n: NotificationItem): string | null {
  if (n.episode_id) return itemPath(serverId, n.episode_id);
  if (n.series_id) return itemPath(serverId, n.series_id);
  if (n.type.startsWith("request.")) return `${serverBase(serverId)}/requests`;
  return null;
}

/** "5 minutes ago", "yesterday", and a date once it is more than a week old. */
function ago(value: string, locale: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const minutes = Math.round((date.getTime() - Date.now()) / 60_000);
  const rel = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (Math.abs(minutes) < 60) return rel.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return rel.format(hours, "hour");
  const days = Math.round(hours / 24);
  if (Math.abs(days) < 7) return rel.format(days, "day");
  return date.toLocaleDateString(locale, { dateStyle: "medium" });
}

/**
 * The profile's notification inbox (`listNotifications`): new episodes of what it follows and answers to its
 * requests, newest first. Opening an entry marks it read (`markNotificationRead`); "Mark all read" takes the list's
 * `read_cutoff` (`markNotificationsRead`). Which notifications are sent, and to where, is set in the web app.
 */
export function Notifications() {
  const { t, i18n } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const refresh = useRefreshTick();
  const { data, error } = useLoad(`${serverId}/notifications`, () => catalog.notifications(serverId), refresh);
  // Entries marked read here, until the list is read again.
  const [read, setRead] = useState<ReadonlySet<string>>(new Set());
  const [allRead, setAllRead] = useState(false);
  const [failure, setFailure] = useState<unknown>(null);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const unread = (n: NotificationItem) => !n.read_at && !allRead && !read.has(n.id);
  const open = (n: NotificationItem) => {
    if (unread(n)) {
      setRead((s) => new Set(s).add(n.id));
      catalog
        .notificationRead(serverId, n.id)
        .then(() => {
          bumpRefresh(0);
        })
        .catch(() => {
          setRead((s) => {
            const next = new Set(s);
            next.delete(n.id);
            return next;
          });
        });
    }
    const to = target(serverId, n);
    if (to) void navigate(to);
  };
  const markAll = () => {
    if (!data?.read_cutoff) return;
    setAllRead(true);
    setFailure(null);
    catalog
      .notificationsReadAll(serverId, data.read_cutoff)
      .then(() => {
        bumpRefresh(0);
      })
      .catch((e: unknown) => {
        setAllRead(false);
        setFailure(e);
      });
  };
  const items = data?.items ?? [];
  const anyUnread = items.some(unread);

  return (
    <section className="flex flex-col gap-6" aria-label={t("notifications.title")}>
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h1 className="text-3xl font-bold tracking-tight">{t("notifications.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("notifications.subtitle")}</p>
        </div>
        {anyUnread ? (
          <button
            type="button"
            onClick={markAll}
            className="flex items-center gap-2 rounded-full border border-border px-3 py-1.5 text-sm font-medium transition hover:bg-surface-hover"
          >
            <CheckCheck className="size-4" />
            {t("notifications.markAll")}
          </button>
        ) : null}
      </div>
      {error ? <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert> : null}
      {failure ? <Alert>{errorMessage(t, failure)}</Alert> : null}
      {!data && !error ? <div className="h-72 animate-pulse rounded-2xl bg-card" /> : null}
      {data && items.length === 0 ? (
        <div className="flex min-h-64 flex-col items-center justify-center gap-3 rounded-2xl bg-card px-6 py-12 text-center">
          <BellOff className="size-10 text-muted-foreground" strokeWidth={1.5} />
          <p className="text-sm text-muted-foreground">{t("notifications.empty")}</p>
        </div>
      ) : null}
      {items.length > 0 ? (
        <ul className="flex flex-col gap-2">
          {items.map((n) => {
            const src = artwork(serverId, n.poster_url);
            const fresh = unread(n);
            const title = notificationTitle(t, n);
            return (
              <li key={n.id}>
                <button
                  type="button"
                  onClick={() => {
                    open(n);
                  }}
                  aria-label={fresh ? t("notifications.unreadLabel", { title }) : title}
                  className={`flex w-full items-center gap-4 rounded-2xl p-3 text-left transition hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring ${
                    fresh ? "bg-card" : "bg-card/50"
                  }`}
                >
                  <span
                    aria-hidden
                    className={`size-2 shrink-0 rounded-full ${fresh ? "bg-primary" : "bg-transparent"}`}
                  />
                  <span className="aspect-[2/3] w-12 shrink-0 overflow-hidden rounded-md bg-surface-raised">
                    {src ? <img src={src} alt="" loading="lazy" className="size-full object-cover" /> : null}
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className={`truncate ${fresh ? "font-semibold" : "font-medium text-muted-foreground"}`}>
                      {title}
                    </span>
                    <span className="truncate text-sm text-muted-foreground">{notificationText(t, n)}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted-foreground">{ago(n.created_at, i18n.language)}</span>
                </button>
              </li>
            );
          })}
        </ul>
      ) : null}
    </section>
  );
}
