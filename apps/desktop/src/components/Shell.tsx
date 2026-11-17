import {
  Bell,
  Bookmark,
  CalendarDays,
  Clapperboard,
  Folder,
  Heart,
  History,
  Home,
  Inbox,
  Info,
  Layers,
  Search,
  Server,
  Settings,
  Sparkles,
  Tv,
  UserRound,
  Users,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { NavigationType, NavLink, Outlet, useLocation, useNavigate, useNavigationType, useParams } from "react-router";
import { goReauth, needsReauth } from "../lib/nav";
import { BackButton } from "./ui";
import { catalog } from "../lib/catalog";
import { bumpRefresh, serverEvents, useRefreshTick } from "../lib/events";
import { useLoad } from "../lib/useLoad";
import { silo, type LibraryView } from "../lib/silo";

function libraryIcon(kind: string) {
  if (kind === "movie" || kind === "movies") return <Clapperboard className="size-[18px] shrink-0" />;
  if (kind === "show" || kind === "shows" || kind === "series" || kind === "tv")
    return <Tv className="size-[18px] shrink-0" />;
  return <Folder className="size-[18px] shrink-0" />;
}

function Item({
  to,
  icon,
  label,
  end = false,
  badge = 0,
}: {
  to: string;
  icon: ReactNode;
  label: string;
  end?: boolean;
  /** A count shown at the item (unread notifications); nothing when zero. */
  badge?: number;
}) {
  return (
    <li>
      <NavLink
        to={to}
        end={end}
        title={label}
        className={({ isActive }) =>
          `relative flex items-center gap-3 rounded-lg px-3 py-2.5 text-[0.8125rem] font-medium transition-colors focus-visible:outline-2 focus-visible:outline-ring ${
            isActive
              ? "bg-surface-hover text-foreground"
              : "text-muted-foreground hover:bg-surface-hover/60 hover:text-foreground"
          }`
        }
      >
        {({ isActive }) => (
          <>
            {isActive ? (
              <span className="absolute top-1/2 left-0 h-[18px] w-[3px] -translate-y-1/2 rounded-r-sm bg-primary" />
            ) : null}
            {icon}
            <span className="hidden truncate lg:inline">{label}</span>
            {badge > 0 ? (
              <span className="absolute top-1.5 left-6 min-w-4 rounded-full bg-primary px-1 text-center text-[0.625rem] leading-4 font-bold text-primary-foreground lg:static lg:ml-auto">
                {badge > 99 ? "99+" : badge}
              </span>
            ) : null}
          </>
        )}
      </NavLink>
    </li>
  );
}

/**
 * Signed-in layout in the web app's arrangement: fixed sidebar (icon rail below `lg`, 260 px with labels above),
 * scrolling main stage with the page gutter. Native window chrome is kept (snap layouts work unchanged).
 */
export function Shell() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "" } = useParams();
  const base = `/servers/${encodeURIComponent(serverId)}`;
  const [libraries, setLibraries] = useState<LibraryView[]>([]);
  // Unread notifications, read again when the server reports a change or one is marked read here.
  const refreshTick = useRefreshTick();
  const { data: unread } = useLoad(`${serverId}/unread`, () => catalog.notificationsUnread(serverId), refreshTick);
  const mainRef = useRef<HTMLElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const location = useLocation();
  const navType = useNavigationType();
  const scrolls = useRef(new Map<string, number>());
  // Detail pages are reached from many places and have no sidebar entry: they get Back, in one fixed spot that is
  // there before the page has loaded. Top-level destinations (sidebar) have none.
  const detail = /\/(item|person|collection|box-set|recommendations)\//.test(location.pathname);

  // Each history entry keeps its scroll position: Back returns to the same spot, a new page starts at the top.
  // Content loads after the first paint, so the saved position is re-applied while the page grows, until it is
  // reached, the user scrolls themselves, or 2 s pass (one jump to the right spot instead of top-then-jump).
  useLayoutEffect(() => {
    const main = mainRef.current;
    const content = contentRef.current;
    if (!main) return;
    const key = location.key;
    const saved = navType === NavigationType.Pop ? scrolls.current.get(key) : undefined;
    let restoring = saved !== undefined && saved > 0;
    main.scrollTop = saved ?? 0;
    const stop = () => {
      restoring = false;
    };
    const deadline = window.setTimeout(stop, 2000);
    const grow =
      restoring && content && typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(() => {
            if (!restoring || saved === undefined) return;
            main.scrollTop = saved;
            if (Math.abs(main.scrollTop - saved) < 2) stop();
          })
        : null;
    if (content) grow?.observe(content);
    const onScroll = () => {
      if (!restoring) scrolls.current.set(key, main.scrollTop);
    };
    main.addEventListener("scroll", onScroll, { passive: true });
    main.addEventListener("wheel", stop, { passive: true });
    main.addEventListener("pointerdown", stop, { passive: true });
    main.addEventListener("keydown", stop);
    return () => {
      window.clearTimeout(deadline);
      grow?.disconnect();
      main.removeEventListener("scroll", onScroll);
      main.removeEventListener("wheel", stop);
      main.removeEventListener("pointerdown", stop);
      main.removeEventListener("keydown", stop);
    };
  }, [location.key, navType]);

  useEffect(() => {
    let alive = true;
    silo
      .librariesList(serverId)
      .then((l) => {
        if (alive) setLibraries(l);
      })
      .catch((err: unknown) => {
        if (!alive) return;
        if (needsReauth(err)) goReauth(navigate, serverId, err);
      });
    return () => {
      alive = false;
    };
  }, [serverId, navigate]);

  // Live updates (events socket, protocol silo.events.v2): user state and catalog changes reload screens in place;
  // access_changed (profile or session access revoked) sends the user back to profile selection.
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void serverEvents
      .on((e) => {
        if (e.serverId !== serverId) return;
        if (e.event === "access_changed") void navigate(`${base}/profiles`, { replace: true });
        else bumpRefresh();
      })
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      });
    void serverEvents.connect(serverId).catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
      void serverEvents.disconnect().catch(() => undefined);
    };
  }, [serverId, base, navigate]);

  return (
    <div className="flex h-full">
      <a
        href="#main"
        className="sr-only focus:not-sr-only focus:fixed focus:top-4 focus:left-4 focus:z-50 focus:rounded-lg focus:bg-background focus:px-4 focus:py-2"
        onClick={(e) => {
          e.preventDefault();
          document.getElementById("main")?.focus();
        }}
      >
        {t("shell.skip")}
      </a>
      <nav
        aria-label={t("shell.nav")}
        className="flex w-16 shrink-0 flex-col gap-6 overflow-y-auto border-r border-border/70 bg-sidebar px-2 py-5 lg:w-[260px] lg:px-3"
      >
        <div className="hidden px-3 text-xl font-semibold tracking-tight lg:block">{t("app.name")}</div>
        <ul className="flex flex-col gap-0.5">
          <Item to={`${base}/home`} icon={<Home className="size-[18px] shrink-0" />} label={t("shell.home")} end />
          <Item to={`${base}/search`} icon={<Search className="size-[18px] shrink-0" />} label={t("shell.search")} />
        </ul>
        <div className="flex flex-col gap-1">
          <p className="hidden px-3 text-[0.6875rem] font-semibold tracking-[0.14em] text-muted-foreground uppercase lg:block">
            {t("shell.yours")}
          </p>
          <ul className="flex flex-col gap-0.5">
            <Item
              to={`${base}/recommendations`}
              icon={<Sparkles className="size-[18px] shrink-0" />}
              label={t("recommendations.title")}
            />
            <Item
              to={`${base}/list/watchlist`}
              icon={<Bookmark className="size-[18px] shrink-0" />}
              label={t("lists.watchlist.title")}
            />
            <Item
              to={`${base}/list/favorites`}
              icon={<Heart className="size-[18px] shrink-0" />}
              label={t("lists.favorites.title")}
            />
            <Item
              to={`${base}/list/history`}
              icon={<History className="size-[18px] shrink-0" />}
              label={t("lists.history.title")}
            />
            <Item
              to={`${base}/collections`}
              icon={<Layers className="size-[18px] shrink-0" />}
              label={t("collections.title")}
            />
            <Item
              to={`${base}/calendar`}
              icon={<CalendarDays className="size-[18px] shrink-0" />}
              label={t("calendar.title")}
            />
            <Item
              to={`${base}/watch-party`}
              icon={<Users className="size-[18px] shrink-0" />}
              label={t("together.title")}
            />
            <Item
              to={`${base}/requests`}
              icon={<Inbox className="size-[18px] shrink-0" />}
              label={t("requests.title")}
            />
            <Item
              to={`${base}/notifications`}
              icon={<Bell className="size-[18px] shrink-0" />}
              label={t("notifications.title")}
              badge={unread ?? 0}
            />
          </ul>
        </div>
        {libraries.length > 0 ? (
          <div className="flex flex-col gap-1">
            <p className="hidden px-3 text-[0.6875rem] font-semibold tracking-[0.14em] text-muted-foreground uppercase lg:block">
              {t("shell.libraries")}
            </p>
            <ul className="flex flex-col gap-0.5">
              {libraries.map((l) => (
                <Item
                  key={l.id}
                  to={`${base}/library/${encodeURIComponent(l.id)}`}
                  icon={libraryIcon(l.kind)}
                  label={l.name}
                />
              ))}
            </ul>
          </div>
        ) : null}
        <ul className="mt-auto flex flex-col gap-0.5">
          <Item
            to={`${base}/profiles`}
            icon={<UserRound className="size-[18px] shrink-0" />}
            label={t("shell.profile")}
          />
          <Item to="/" icon={<Server className="size-[18px] shrink-0" />} label={t("shell.servers")} end />
          <Item
            to={`${base}/settings`}
            icon={<Settings className="size-[18px] shrink-0" />}
            label={t("shell.settings")}
          />
          <Item to={`${base}/about`} icon={<Info className="size-[18px] shrink-0" />} label={t("shell.about")} />
        </ul>
      </nav>
      <main ref={mainRef} id="main" tabIndex={-1} className="min-w-0 flex-1 overflow-y-auto focus:outline-none">
        <div ref={contentRef} className="mx-auto w-full max-w-[2400px] px-4 py-6 sm:px-6 lg:px-10 lg:py-8 xl:px-12">
          {detail ? (
            <div className="sticky top-3 z-30 mb-5 h-9">
              <BackButton fallback={`${base}/home`} />
            </div>
          ) : null}
          <Outlet />
        </div>
      </main>
    </div>
  );
}
