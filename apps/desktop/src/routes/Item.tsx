import { Check, Clapperboard, Heart, MoreHorizontal, Play, RotateCcw, Star } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";
import { Badge, PickerPill, Popover, TrackOption } from "../components/Popover";
import { PosterCard } from "../components/PosterCard";
import { Row } from "../components/Row";
import { Alert, BackButton } from "../components/ui";
import {
  artwork,
  catalog,
  trailerThumb,
  type CatalogItemDetail,
  type CatalogItemUserState,
  type Episode,
  type ItemExtra,
  type ItemVideo,
  type Season,
  type UserFlag,
} from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { qualityTags, runtimeLabel } from "../lib/format";
import { retryLoads, useRefreshTick } from "../lib/events";
import { openTrailer } from "../lib/host";
import { goReauth, itemPath, needsReauth, personPath } from "../lib/nav";
import { defaultAudio, defaultChoice, defaultSubtitle, formatTime, resumePosition, type PlayChoice } from "../lib/play";
import { applyTrackPicks, rememberTracks } from "../lib/trackMemory";
import { audioInfo, subtitleInfo, trackSummary } from "../lib/tracks";
import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";
import type { LaunchArt } from "../player/context";

const PLAYABLE = new Set(["movie", "episode"]);
const WRITER_JOBS = new Set(["Screenplay", "Writer", "Story", "Novel", "Teleplay", "Author", "Characters"]);

/* ---------------------------------------------------------------- play + tracks */

function useTrackLabels() {
  const { t } = useTranslation();
  return {
    audio: { fallback: t("item.unknownTrack"), default: t("item.default") },
    subtitle: {
      fallback: t("item.unknownTrack"),
      forced: t("item.forced"),
      sdh: "SDH",
      default: t("item.default"),
      external: t("item.external"),
    },
  };
}

/** Play/Resume (with progress fill) plus version/audio/subtitle pickers, for movies and episodes. */
function PlayPanel({
  serverId,
  itemId,
  art,
  extra,
}: {
  serverId: string;
  itemId: string;
  art: LaunchArt;
  extra?: ReactNode;
}) {
  const { t, i18n } = useTranslation();
  const labels = useTrackLabels();
  const refresh = useRefreshTick();
  const { data: watch, error: loadError } = useLoad(
    `${serverId}/${itemId}`,
    () => catalog.watch(serverId, itemId),
    refresh,
  );
  const [picked, setPicked] = useState<{ itemId: string; choice: PlayChoice } | null>(null);
  const { play, busy, error: playError } = usePlay(serverId);

  if (loadError) return <Alert onRetry={retryLoads}>{errorMessage(t, loadError)}</Alert>;
  if (!watch) return <div className="h-12 w-40 animate-pulse rounded-full bg-white/10" />;
  const base = defaultChoice(watch);
  const choice = picked?.itemId === itemId ? picked.choice : base && applyTrackPicks(serverId, watch, base);
  if (!choice) return <Alert tone="warning">{t("item.noVersions")}</Alert>;
  const setChoice = (c: PlayChoice) => {
    setPicked({ itemId, choice: c });
    rememberTracks(serverId, watch, c.version, {
      audioIndex: c.audioIndex !== choice.audioIndex ? c.audioIndex : undefined,
      subtitleIndex: c.subtitleIndex !== choice.subtitleIndex ? c.subtitleIndex : undefined,
    });
  };

  const resume = resumePosition(watch);
  const v = choice.version;
  const duration = v.duration_seconds || watch.user_data?.duration_seconds || 0;
  const progress = resume > 0 && duration > 0 ? Math.min(1, resume / duration) : 0;
  const left = duration > resume ? Math.round((duration - resume) / 60) : 0;
  const subs = v.subtitle_tracks.filter((s): s is typeof s & { index: number } => typeof s.index === "number");
  const audio = choice.audioIndex === null ? undefined : v.audio_tracks[choice.audioIndex];
  const sub = subs.find((s) => s.index === choice.subtitleIndex);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          disabled={busy}
          onClick={() => void play(itemId, { watch, choice, art, fromStart: resume <= 0 })}
          aria-label={resume > 0 ? t("item.resume", { time: formatTime(resume) }) : undefined}
          className="relative isolate inline-flex h-12 items-center gap-2.5 overflow-hidden rounded-full bg-primary pr-6 pl-5 font-semibold text-primary-foreground shadow-lg transition hover:scale-[1.02] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring active:scale-[0.99] disabled:opacity-60 motion-reduce:transition-none"
        >
          {progress > 0 ? (
            <span
              aria-hidden
              className="absolute inset-y-0 left-0 -z-10 bg-black/15"
              style={{ width: `${String(progress * 100)}%` }}
            />
          ) : null}
          <Play className="size-5 fill-current" />
          {resume > 0 ? (
            <span className="flex flex-col items-start leading-tight">
              <span>{t("item.resumeShort")}</span>
              <span className="text-[11px] font-medium opacity-70">
                {left > 0 ? t("item.left", { time: runtimeLabel(left) }) : formatTime(resume)}
              </span>
            </span>
          ) : (
            t("item.play")
          )}
        </button>
        {resume > 0 ? (
          <IconAction
            label={t("item.fromStart")}
            disabled={busy}
            onClick={() => void play(itemId, { watch, choice, art, fromStart: true })}
          >
            <RotateCcw className="size-5" />
          </IconAction>
        ) : null}
        {extra}
      </div>
      <div className="flex flex-wrap gap-2">
        {watch.versions.length > 1 ? (
          <Popover
            label={t("item.version")}
            trigger={(open) => (
              <PickerPill
                caption={t("item.version")}
                value={[v.edition_raw ?? v.edition_key, v.resolution, v.hdr ? "HDR" : null].filter(Boolean).join(" · ")}
                open={open}
              />
            )}
          >
            {(close) =>
              watch.versions.map((x) => (
                <TrackOption
                  key={x.file_id}
                  selected={x.file_id === v.file_id}
                  info={{
                    headline: x.edition_raw ?? x.edition_key ?? x.resolution,
                    detail: null,
                    badges: [x.resolution, x.hdr ? "HDR" : null, x.codec_video.toUpperCase()].filter((b): b is string =>
                      Boolean(b),
                    ),
                  }}
                  onSelect={() => {
                    const audioIndex = defaultAudio(x);
                    setChoice({ version: x, audioIndex, subtitleIndex: defaultSubtitle(watch, x, audioIndex) });
                    close();
                  }}
                />
              ))
            }
          </Popover>
        ) : null}
        {v.audio_tracks.length > 0 ? (
          <Popover
            label={t("item.audio")}
            trigger={(open) => (
              <PickerPill
                caption={t("item.audio")}
                value={audio ? trackSummary(audioInfo(audio, i18n.language, labels.audio)) : "—"}
                open={open}
              />
            )}
          >
            {(close) => (
              <div className="max-h-80 overflow-y-auto">
                {v.audio_tracks.map((a, i) => (
                  <TrackOption
                    key={i}
                    selected={i === choice.audioIndex}
                    info={audioInfo(a, i18n.language, labels.audio)}
                    onSelect={() => {
                      // Forced subtitles follow the audio language; keep a subtitle the user picked by hand.
                      const automatic = choice.subtitleIndex === defaultSubtitle(watch, v, choice.audioIndex);
                      setChoice({
                        ...choice,
                        audioIndex: i,
                        subtitleIndex: automatic ? defaultSubtitle(watch, v, i) : choice.subtitleIndex,
                      });
                      close();
                    }}
                  />
                ))}
              </div>
            )}
          </Popover>
        ) : null}
        <Popover
          label={t("item.subtitles")}
          trigger={(open) => (
            <PickerPill
              caption={t("item.subtitles")}
              value={sub ? trackSummary(subtitleInfo(sub, i18n.language, labels.subtitle), 1) : t("item.subtitlesOff")}
              open={open}
            />
          )}
        >
          {(close) => (
            <div className="max-h-80 overflow-y-auto">
              <TrackOption
                selected={choice.subtitleIndex === null}
                info={{ headline: t("item.subtitlesOff"), detail: null, badges: [] }}
                onSelect={() => {
                  setChoice({ ...choice, subtitleIndex: null });
                  close();
                }}
              />
              {subs.map((s) => (
                <TrackOption
                  key={s.index}
                  selected={s.index === choice.subtitleIndex}
                  info={subtitleInfo(s, i18n.language, labels.subtitle)}
                  onSelect={() => {
                    setChoice({ ...choice, subtitleIndex: s.index });
                    close();
                  }}
                />
              ))}
            </div>
          )}
        </Popover>
      </div>
      {playError ? <Alert>{playError}</Alert> : null}
    </div>
  );
}

/** Series/season: "Play" / "Continue" the episode the server names in `play_content_id`. */
function SeriesPlay({
  serverId,
  item,
  art,
  extra,
}: {
  serverId: string;
  item: CatalogItemDetail;
  art: LaunchArt;
  extra?: ReactNode;
}) {
  const { t } = useTranslation();
  const { play, busy, error } = usePlay(serverId);
  const target = item.play_content_id;
  const inProgress = (item.user_data?.in_progress_count ?? 0) > 0;
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-3">
        {target ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void play(target, { art })}
            className="inline-flex h-12 items-center gap-2.5 rounded-full bg-primary pr-6 pl-5 font-semibold text-primary-foreground shadow-lg transition hover:scale-[1.02] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-60 motion-reduce:transition-none"
          >
            <Play className="size-5 fill-current" />
            {inProgress ? t("item.continue") : t("item.play")}
          </button>
        ) : null}
        {extra}
      </div>
      {error ? <Alert>{error}</Alert> : null}
    </div>
  );
}

/* ---------------------------------------------------------------- user actions */

function IconAction({
  label,
  active = false,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={`flex size-12 items-center justify-center rounded-full border backdrop-blur transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:opacity-50 ${
        active
          ? "border-foreground bg-foreground text-background"
          : "border-white/15 bg-black/30 text-foreground hover:border-white/40"
      }`}
    >
      {children}
    </button>
  );
}

function Stars({
  value,
  onChange,
  label,
}: {
  value: number | null;
  onChange: (v: number | null) => void;
  label: string;
}) {
  const [hover, setHover] = useState<number | null>(null);
  const shown = hover ?? value ?? 0;
  return (
    <div
      role="group"
      aria-label={label}
      className="flex h-12 items-center rounded-full border border-white/15 bg-black/30 px-2 backdrop-blur"
      onMouseLeave={() => {
        setHover(null);
      }}
    >
      {[1, 2, 3, 4, 5].map((n) => (
        <button
          key={n}
          type="button"
          aria-pressed={value === n}
          aria-label={String(n)}
          onMouseEnter={() => {
            setHover(n);
          }}
          onClick={() => {
            onChange(value === n ? null : n);
          }}
          className="p-1 focus-visible:outline-2 focus-visible:outline-ring"
        >
          <Star className={`size-4 transition ${n <= shown ? "fill-foreground text-foreground" : "text-white/35"}`} />
        </button>
      ))}
    </div>
  );
}

/** Mark watched, favourite, star rating and the overflow menu (watchlist, not interested); optimistic. */
function UserActions({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const server: CatalogItemUserState = item.user_state ?? { played: false, is_favorite: false, in_watchlist: false };
  const [local, setLocal] = useState<{ id: string; state: CatalogItemUserState; rating: number | null } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const cur = local?.id === item.content_id ? local : { state: server, rating: item.user_rating ?? null };
  const flags = cur.state;
  const watched = flags.played || item.user_data?.played === true;

  const toggle = (flag: UserFlag, key: keyof CatalogItemUserState, on: boolean) => {
    const prev = cur;
    setLocal({ id: item.content_id, rating: cur.rating, state: { ...flags, [key]: on } });
    setError(null);
    catalog.setFlag(serverId, item.content_id, flag, on).catch((e: unknown) => {
      setLocal({ id: item.content_id, ...prev });
      setError(errorMessage(t, e));
    });
  };
  const rate = (r: number | null) => {
    const prev = cur;
    setLocal({ id: item.content_id, state: flags, rating: r });
    setError(null);
    catalog.setRating(serverId, item.content_id, r).catch((e: unknown) => {
      setLocal({ id: item.content_id, ...prev });
      setError(errorMessage(t, e));
    });
  };

  return (
    <>
      <IconAction
        label={watched ? t("item.markUnwatched") : t("item.markWatched")}
        active={watched}
        onClick={() => {
          toggle("watched", "played", !watched);
        }}
      >
        <Check className="size-5" />
      </IconAction>
      <IconAction
        label={flags.is_favorite ? t("item.unfavorite") : t("item.favorite")}
        active={flags.is_favorite}
        onClick={() => {
          toggle("favorite", "is_favorite", !flags.is_favorite);
        }}
      >
        <Heart className={`size-5 ${flags.is_favorite ? "fill-current" : ""}`} />
      </IconAction>
      <Stars value={cur.rating} onChange={rate} label={t("item.rate")} />
      <Popover
        label={t("item.more")}
        align="end"
        trigger={() => (
          <span className="flex size-12 items-center justify-center rounded-full border border-white/15 bg-black/30 backdrop-blur transition hover:border-white/40">
            <MoreHorizontal className="size-5" />
          </span>
        )}
      >
        {(close) => (
          <div role="menu" className="flex flex-col">
            <MenuItem
              onClick={() => {
                toggle("watchlist", "in_watchlist", !flags.in_watchlist);
                close();
              }}
            >
              {flags.in_watchlist ? t("item.removeWatchlist") : t("item.addWatchlist")}
            </MenuItem>
            <MenuItem
              onClick={() => {
                toggle("not_interested", "not_interested", !flags.not_interested);
                close();
              }}
            >
              {flags.not_interested ? t("item.undoNotInterested") : t("item.notInterested")}
            </MenuItem>
          </div>
        )}
      </Popover>
      {error ? (
        <p role="alert" className="basis-full text-sm text-destructive">
          {error}
        </p>
      ) : null}
    </>
  );
}

function MenuItem({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className="rounded-lg px-3 py-2 text-left text-sm hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none"
    >
      {children}
    </button>
  );
}

/* ---------------------------------------------------------------- hero */

function Hero({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const backdrop = artwork(serverId, item.backdrop_url);
  const logo = artwork(serverId, item.logo_url);
  const poster = artwork(serverId, item.poster_url);
  const [posterFailed, setPosterFailed] = useState(false);
  const [logoFailed, setLogoFailed] = useState(false);
  const tmdb = item.ratings.find((r) => r.source === "tmdb") ?? item.ratings[0];
  const tags = qualityTags(item.overlay_summary);
  const directors = item.crew.filter((c) => c.job === "Director");
  const writers = dedupe(item.crew.filter((c) => WRITER_JOBS.has(c.job)));
  const creators = item.crew.filter((c) => c.job === "Creator");
  const actions = <UserActions serverId={serverId} item={item} />;
  const art: LaunchArt = {
    backdrop: item.backdrop_url ?? null,
    logo: item.logo_url ?? null,
    poster: item.poster_url ?? null,
  };

  return (
    <section className="relative isolate z-10 rounded-2xl border border-border/60 bg-card">
      {/* Artwork is clipped to the box; the track popovers are not. */}
      <div aria-hidden className="absolute inset-0 -z-10 overflow-hidden rounded-2xl">
        {backdrop ? (
          <img
            src={backdrop}
            alt=""
            className="absolute inset-0 size-full animate-[fade-in_600ms_ease-out] object-cover object-top"
          />
        ) : null}
        <div className="absolute inset-0 bg-gradient-to-r from-card via-card/85 to-card/10" />
        <div className="absolute inset-0 bg-gradient-to-t from-card via-transparent to-transparent" />
      </div>
      <div className="flex min-h-[26rem] items-end gap-8 p-6 lg:gap-10 lg:p-10">
        {poster && !posterFailed ? (
          <img
            src={poster}
            alt=""
            className="hidden w-44 shrink-0 animate-[fade-in_400ms_ease-out] self-end rounded-xl border border-white/10 shadow-2xl md:block lg:w-56"
            onError={() => {
              setPosterFailed(true);
            }}
          />
        ) : null}
        <div className="flex min-w-0 max-w-3xl flex-1 flex-col justify-end gap-4">
          {item.series_title && item.series_id ? (
            <Link
              to={itemPath(serverId, item.series_id)}
              className="text-sm font-medium text-muted-foreground hover:text-foreground"
            >
              {item.series_title}
              {item.season_number != null ? ` · ${t("item.seasonN", { n: item.season_number })}` : ""}
              {item.episode_number != null ? ` · ${t("item.episodeN", { n: item.episode_number })}` : ""}
            </Link>
          ) : null}
          {logo && !logoFailed ? (
            <img
              src={logo}
              alt={item.title}
              className="max-h-28 max-w-sm object-contain object-left drop-shadow-2xl"
              onError={() => {
                setLogoFailed(true);
              }}
            />
          ) : (
            <h1 className="text-4xl font-bold tracking-tight lg:text-5xl">{item.title}</h1>
          )}
          <div className="flex flex-wrap items-center gap-1.5">
            {item.year ? <Badge strong>{item.year}</Badge> : null}
            {item.content_rating ? <Badge strong>{item.content_rating}</Badge> : null}
            {runtimeLabel(item.runtime) ? <Badge strong>{runtimeLabel(item.runtime)}</Badge> : null}
            {item.type === "series" && item.season_count ? (
              <Badge strong>{t("item.seasonCount", { count: item.season_count })}</Badge>
            ) : null}
            {tags.map((x) => (
              <Badge key={x}>{x}</Badge>
            ))}
            {tmdb ? (
              <span className="ml-1 inline-flex items-center gap-1 text-xs font-semibold" title={tmdb.name}>
                <Star className="size-3.5 fill-foreground" />
                {tmdb.display}
                <span className="font-medium text-muted-foreground">{tmdb.name}</span>
              </span>
            ) : null}
          </div>
          {item.genres.length > 0 ? (
            <p className="text-sm text-muted-foreground">{item.genres.slice(0, 4).join(" · ")}</p>
          ) : null}
          {item.tagline ? <p className="text-sm text-foreground/80 italic">{item.tagline}</p> : null}
          {item.overview ? <p className="line-clamp-4 text-sm leading-relaxed lg:text-base">{item.overview}</p> : null}
          <CreditLine serverId={serverId} label={t("item.directedBy")} people={directors} />
          <CreditLine serverId={serverId} label={t("item.createdBy")} people={creators} />
          <CreditLine serverId={serverId} label={t("item.writtenBy")} people={writers} />
          <div className="pt-2">
            {PLAYABLE.has(item.type) ? (
              <PlayPanel serverId={serverId} itemId={item.content_id} art={art} extra={actions} />
            ) : (
              <SeriesPlay serverId={serverId} item={item} art={art} extra={actions} />
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function dedupe<T extends { name: string }>(xs: T[]): T[] {
  const seen = new Set<string>();
  return xs.filter((x) => (seen.has(x.name) ? false : (seen.add(x.name), true)));
}

function CreditLine({
  serverId,
  label,
  people,
}: {
  serverId: string;
  label: string;
  people: { name: string; person_id?: string | null }[];
}) {
  if (people.length === 0) return null;
  return (
    <p className="text-sm">
      <span className="text-muted-foreground">{label} </span>
      {people.slice(0, 3).map((p, i) => (
        <span key={p.name}>
          {i > 0 ? ", " : ""}
          {p.person_id ? (
            <Link to={personPath(serverId, p.person_id)} className="font-medium hover:underline">
              {p.name}
            </Link>
          ) : (
            <span className="font-medium">{p.name}</span>
          )}
        </span>
      ))}
    </p>
  );
}

/* ---------------------------------------------------------------- cast & crew, trailers */

interface Credit {
  key: string;
  name: string;
  role: string;
  person_id?: string | null | undefined;
  photo_url?: string | null | undefined;
}

function CastAndCrew({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const toCredit = (c: { name: string; person_id?: string | null; photo_url?: string | null }, role: string) => ({
    key: `${c.person_id ?? c.name}-${role}`,
    name: c.name,
    role,
    person_id: c.person_id,
    photo_url: c.photo_url,
  });
  const groups: [string, Credit[]][] = [
    [
      t("item.directors"),
      dedupe(item.crew.filter((c) => c.job === "Director" || c.job === "Creator")).map((c) => toCredit(c, c.job)),
    ],
    [t("item.writers"), dedupe(item.crew.filter((c) => WRITER_JOBS.has(c.job))).map((c) => toCredit(c, c.job))],
    [
      t("item.cast"),
      [...item.cast]
        .sort((a, b) => a.order - b.order)
        .slice(0, 30)
        .map((c) => toCredit(c, c.character)),
    ],
  ];
  const shown = groups.filter(([, xs]) => xs.length > 0);
  if (shown.length === 0) return null;
  return (
    <Row title={t("item.castCrew")}>
      {shown.map(([label, credits]) => (
        <div key={label} className="flex shrink-0 gap-4">
          <div className="flex w-5 shrink-0 items-center justify-center border-l border-border">
            <span className="rotate-180 text-[10px] font-semibold tracking-[0.25em] text-muted-foreground uppercase [writing-mode:vertical-rl]">
              {label}
            </span>
          </div>
          {credits.map((c) => (
            <PersonCard key={c.key} serverId={serverId} credit={c} />
          ))}
        </div>
      ))}
    </Row>
  );
}

function PersonCard({ serverId, credit }: { serverId: string; credit: Credit }) {
  const photo = artwork(serverId, credit.photo_url);
  const [failed, setFailed] = useState(false);
  const initials = credit.name
    .split(/\s+/)
    .map((w) => w.charAt(0))
    .slice(0, 2)
    .join("");
  const body = (
    <>
      <span className="media-card-image block aspect-[2/3] overflow-hidden rounded-xl bg-surface-raised">
        {photo && !failed ? (
          <img
            src={photo}
            alt=""
            loading="lazy"
            draggable={false}
            className="size-full object-cover"
            onError={() => {
              setFailed(true);
            }}
          />
        ) : (
          <span className="flex size-full items-center justify-center text-2xl font-semibold text-muted-foreground">
            {initials}
          </span>
        )}
      </span>
      <span className="mt-2 block truncate text-sm font-semibold">{credit.name}</span>
      <span className="block truncate text-xs text-muted-foreground">{credit.role}</span>
    </>
  );
  return credit.person_id ? (
    <Link
      to={personPath(serverId, credit.person_id)}
      className="media-card group/p block w-28 shrink-0 rounded-xl focus-visible:outline-2 focus-visible:outline-ring"
    >
      {body}
    </Link>
  ) : (
    <div className="media-card w-28 shrink-0">{body}</div>
  );
}

function TrailersAndMore({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const { play, busy } = usePlay(serverId);
  const videos = (item.videos ?? []).filter((v) => v.site.toLowerCase() === "youtube" && v.site_key);
  const extras = item.extras ?? [];
  if (videos.length === 0 && extras.length === 0) return null;
  return (
    <Row title={t("item.trailers")}>
      {extras.map((x) => (
        <ExtraCard
          key={x.content_id}
          serverId={serverId}
          extra={x}
          disabled={busy}
          onPlay={() => void play(x.content_id)}
        />
      ))}
      {videos.map((v) => (
        <TrailerCard key={v.site_key} serverId={serverId} video={v} />
      ))}
    </Row>
  );
}

function Thumb({ src, children }: { src: string | undefined; children?: ReactNode }) {
  const [failed, setFailed] = useState(false);
  return (
    <span className="media-card-image relative block aspect-video overflow-hidden rounded-xl bg-surface-raised">
      {src && !failed ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          draggable={false}
          className="size-full object-cover"
          onError={() => {
            setFailed(true);
          }}
        />
      ) : (
        <span className="flex size-full items-center justify-center text-muted-foreground">
          <Clapperboard className="size-8" />
        </span>
      )}
      <span className="absolute inset-0 flex items-center justify-center bg-black/20 opacity-0 transition group-hover/t:opacity-100">
        <span className="flex size-11 items-center justify-center rounded-full bg-white/90 text-black">
          <Play className="size-5 fill-current" />
        </span>
      </span>
      {children}
    </span>
  );
}

function TrailerCard({ serverId, video }: { serverId: string; video: ItemVideo }) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={() => void openTrailer(video.site_key)}
      title={t("item.opensYoutube")}
      className="media-card group/t w-72 shrink-0 text-left focus-visible:outline-2 focus-visible:outline-ring"
    >
      <Thumb src={trailerThumb(serverId, video)}>
        <span className="absolute top-2 left-2">
          <Badge strong>YouTube</Badge>
        </span>
      </Thumb>
      <span className="mt-2 block truncate text-sm font-semibold">{video.name ?? video.kind}</span>
      <span className="block truncate text-xs text-muted-foreground capitalize">{video.kind}</span>
    </button>
  );
}

function ExtraCard({
  serverId,
  extra,
  disabled,
  onPlay,
}: {
  serverId: string;
  extra: ItemExtra;
  disabled: boolean;
  onPlay: () => void;
}) {
  const kind = extra.kind.replace(/_/g, " ");
  return (
    <button
      type="button"
      disabled={disabled}
      onClick={onPlay}
      className="media-card group/t w-72 shrink-0 text-left focus-visible:outline-2 focus-visible:outline-ring"
    >
      <Thumb src={artwork(serverId, extra.still_url)}>
        {extra.duration_seconds ? (
          <span className="absolute right-2 bottom-2 rounded bg-black/70 px-1.5 text-[11px] font-medium text-white">
            {formatTime(extra.duration_seconds)}
          </span>
        ) : null}
      </Thumb>
      <span className="mt-2 block truncate text-sm font-semibold">{extra.title ?? kind}</span>
      <span className="block truncate text-xs text-muted-foreground capitalize">{kind}</span>
    </button>
  );
}

function MoreLikeThis({ serverId, itemId }: { serverId: string; itemId: string }) {
  const { t } = useTranslation();
  const { data } = useLoad(`${serverId}/${itemId}/similar`, () => catalog.similar(serverId, itemId));
  const { play } = usePlay(serverId);
  if (!data || data.length === 0) return null;
  return (
    <Row title={t("item.moreLikeThis")}>
      {data.map((x) => (
        <PosterCard key={x.content_id} serverId={serverId} item={x} onPlay={(i) => void play(i.content_id)} />
      ))}
    </Row>
  );
}

/* ---------------------------------------------------------------- episodes */

function EpisodeList({
  serverId,
  seriesId,
  initialSeason,
}: {
  serverId: string;
  seriesId: string;
  initialSeason?: number | null | undefined;
}) {
  const { t } = useTranslation();
  const seasonsLoad = useLoad(`${serverId}/${seriesId}`, () => catalog.seasons(serverId, seriesId));
  const seasons: Season[] | undefined = seasonsLoad.data;
  const [picked, setPicked] = useState<{ seriesId: string; season: number } | null>(null);
  const season =
    picked?.seriesId === seriesId
      ? picked.season
      : (initialSeason ?? seasons?.find((x) => !x.is_specials)?.season_number ?? seasons?.[0]?.season_number ?? null);
  const setSeason = (n: number) => {
    setPicked({ seriesId, season: n });
  };
  const refresh = useRefreshTick();
  const episodesLoad = useLoad(
    season === null ? null : `${serverId}/${seriesId}/${String(season)}`,
    () => (season === null ? Promise.resolve([] as Episode[]) : catalog.episodes(serverId, seriesId, season)),
    refresh,
  );
  const episodes: Episode[] | undefined = episodesLoad.data;
  const { play, busy, error: playError } = usePlay(serverId);

  const loadError = seasonsLoad.error ?? episodesLoad.error;
  if (loadError) return <Alert onRetry={retryLoads}>{errorMessage(t, loadError)}</Alert>;
  if (!seasons) return <p className="text-sm text-muted-foreground">{t("status.loading")}</p>;

  return (
    <section className="flex flex-col gap-4" aria-label={t("item.episodes")}>
      <div role="tablist" aria-label={t("item.seasons")} className="flex flex-wrap gap-2">
        {seasons.map((s) => (
          <button
            key={s.content_id}
            type="button"
            role="tab"
            aria-selected={s.season_number === season}
            onClick={() => {
              setSeason(s.season_number);
            }}
            className={`rounded-full px-4 py-1.5 text-sm font-medium transition focus-visible:outline-2 focus-visible:outline-ring ${
              s.season_number === season ? "bg-primary text-primary-foreground" : "bg-secondary hover:bg-surface-hover"
            }`}
          >
            {s.title}
          </button>
        ))}
      </div>
      {playError ? <Alert>{playError}</Alert> : null}
      {episodes === undefined ? <p className="text-sm text-muted-foreground">{t("status.loading")}</p> : null}
      <ol className="flex flex-col gap-3">
        {episodes?.map((e) => {
          const still = artwork(serverId, e.still_url);
          const progress =
            e.user_data?.position_seconds && e.runtime > 0
              ? Math.min(1, e.user_data.position_seconds / (e.runtime * 60))
              : 0;
          return (
            <li
              key={e.content_id}
              className="group flex gap-4 rounded-xl border border-border bg-card p-3 hover:bg-surface-hover"
            >
              <button
                type="button"
                disabled={busy}
                onClick={() => void play(e.content_id)}
                aria-label={t("item.playEpisode", { title: e.title })}
                className="relative aspect-video w-48 shrink-0 overflow-hidden rounded-lg bg-surface-raised focus-visible:outline-2 focus-visible:outline-ring"
              >
                {still ? <img src={still} alt="" loading="lazy" className="size-full object-cover" /> : null}
                <span className="absolute inset-0 flex items-center justify-center bg-black/30 opacity-0 transition group-hover:opacity-100">
                  <Play className="size-8 text-white" />
                </span>
                {progress > 0 ? (
                  <span className="absolute inset-x-2 bottom-2 h-1 overflow-hidden rounded-full bg-white/25">
                    <span className="block h-full bg-primary" style={{ width: `${String(progress * 100)}%` }} />
                  </span>
                ) : null}
              </button>
              <div className="min-w-0 flex-1">
                <Link to={itemPath(serverId, e.content_id)} className="font-semibold hover:underline">
                  {e.episode_number}. {e.title}
                </Link>
                <p className="text-xs text-muted-foreground">
                  {[runtimeLabel(e.runtime), e.air_date].filter(Boolean).join(" · ")}
                  {e.user_data?.played ? (
                    <Check className="ml-2 inline size-3.5" aria-label={t("browse.watched")} />
                  ) : null}
                </p>
                {e.overview ? <p className="mt-1 line-clamp-2 text-sm text-muted-foreground">{e.overview}</p> : null}
              </div>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

/* ---------------------------------------------------------------- page */

export function Item() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "", itemId = "" } = useParams();
  const refresh = useRefreshTick();
  const { data: item, error } = useLoad(`${serverId}/${itemId}`, () => catalog.item(serverId, itemId), refresh);

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  if (error) return <Alert onRetry={retryLoads}>{errorMessage(t, error)}</Alert>;
  if (!item) return <div className="h-[26rem] animate-pulse rounded-2xl border border-border/60 bg-card" />;

  const seriesId = item.type === "series" ? item.content_id : item.type === "season" ? item.series_id : null;

  return (
    <article className="flex flex-col gap-6">
      <BackButton fallback={`/servers/${encodeURIComponent(serverId)}/home`} />
      <div className="flex flex-col gap-10">
        <Hero serverId={serverId} item={item} />
        {seriesId ? (
          <EpisodeList
            serverId={serverId}
            seriesId={seriesId}
            initialSeason={item.type === "season" ? item.season_number : item.play_season_number}
          />
        ) : null}
        <CastAndCrew serverId={serverId} item={item} />
        <TrailersAndMore serverId={serverId} item={item} />
        {item.type === "movie" || item.type === "series" ? (
          <MoreLikeThis serverId={serverId} itemId={item.content_id} />
        ) : null}
      </div>
    </article>
  );
}
