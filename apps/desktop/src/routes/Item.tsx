import { Check, Play, RotateCcw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link, useNavigate, useParams } from "react-router";
import { Alert, Button } from "../components/ui";
import {
  artwork,
  catalog,
  type AudioTrack,
  type CatalogItemDetail,
  type Episode,
  type Season,
  type SubtitleTrack,
} from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { goReauth, itemPath, needsReauth } from "../lib/nav";
import { defaultAudio, defaultChoice, defaultSubtitle, formatTime, resumePosition, type PlayChoice } from "../lib/play";

import { useLoad } from "../lib/useLoad";
import { usePlay } from "../lib/usePlay";

const PLAYABLE = new Set(["movie", "episode"]);

const langName = (code: string | null | undefined, locale: string) => {
  if (!code || code === "und") return "";
  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
};

function audioLabel(a: AudioTrack, i: number, locale: string): string {
  const parts = [
    a.title ?? a.embedded_title ?? langName(a.language, locale),
    a.codec?.toUpperCase(),
    a.layout ?? (a.channels ? `${String(a.channels)}ch` : null),
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : `#${String(i + 1)}`;
}

function subtitleLabel(s: SubtitleTrack, locale: string, forced: string): string {
  return [s.title ?? s.embedded_title ?? langName(s.language, locale), s.forced ? forced : null, s.codec?.toUpperCase()]
    .filter(Boolean)
    .join(" · ");
}

function Select<T extends string>({
  label,
  value,
  options,
  onChange,
}: {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <label className="flex min-w-48 flex-col gap-1 text-xs">
      <span className="font-medium text-muted-foreground">{label}</span>
      <select
        value={value}
        onChange={(e) => {
          onChange(e.target.value as T);
        }}
        className="rounded-lg border border-border bg-background px-3 py-2 text-sm focus:border-ring focus:outline-none"
      >
        {options.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </label>
  );
}

/** Version/audio/subtitle pickers plus Play/Resume, for movies and episodes. */
function PlayPanel({ serverId, itemId }: { serverId: string; itemId: string }) {
  const { t, i18n } = useTranslation();
  const { data: watch, error: loadError } = useLoad(`${serverId}/${itemId}`, () => catalog.watch(serverId, itemId));
  const [picked, setPicked] = useState<{ itemId: string; choice: PlayChoice } | null>(null);
  const { play, busy, error: playError } = usePlay(serverId);

  if (loadError) return <Alert>{errorMessage(t, loadError)}</Alert>;
  if (!watch) return <p className="text-sm text-muted-foreground">{t("status.loading")}</p>;
  const choice = picked?.itemId === itemId ? picked.choice : defaultChoice(watch);
  if (!choice) return <Alert tone="warning">{t("item.noVersions")}</Alert>;
  const setChoice = (c: PlayChoice) => {
    setPicked({ itemId, choice: c });
  };

  const resume = resumePosition(watch);
  const v = choice.version;
  const pickVersion = (fileId: string) => {
    const version = watch.versions.find((x) => x.file_id === fileId);
    if (!version) return;
    const audioIndex = defaultAudio(version);
    setChoice({ version, audioIndex, subtitleIndex: defaultSubtitle(watch, version, audioIndex) });
  };

  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap gap-2">
        {resume > 0 ? (
          <>
            <Button
              disabled={busy}
              className="inline-flex items-center gap-2"
              onClick={() => void play(itemId, { watch, choice })}
            >
              <Play className="size-4" />
              {t("item.resume", { time: formatTime(resume) })}
            </Button>
            <Button
              variant="secondary"
              disabled={busy}
              className="inline-flex items-center gap-2"
              onClick={() => void play(itemId, { watch, choice, fromStart: true })}
            >
              <RotateCcw className="size-4" />
              {t("item.fromStart")}
            </Button>
          </>
        ) : (
          <Button
            disabled={busy}
            className="inline-flex items-center gap-2"
            onClick={() => void play(itemId, { watch, choice, fromStart: true })}
          >
            <Play className="size-4" />
            {t("item.play")}
          </Button>
        )}
      </div>
      <div className="flex flex-wrap gap-3">
        {watch.versions.length > 1 ? (
          <Select
            label={t("item.version")}
            value={v.file_id}
            onChange={pickVersion}
            options={watch.versions.map((x) => ({
              value: x.file_id,
              label: [x.edition_raw ?? x.edition_key, x.resolution, x.hdr ? "HDR" : null, x.codec_video.toUpperCase()]
                .filter(Boolean)
                .join(" · "),
            }))}
          />
        ) : null}
        {v.audio_tracks.length > 0 ? (
          <Select
            label={t("item.audio")}
            value={String(choice.audioIndex ?? 0)}
            onChange={(x) => {
              setChoice({ ...choice, audioIndex: Number(x) });
            }}
            options={v.audio_tracks.map((a, i) => ({ value: String(i), label: audioLabel(a, i, i18n.language) }))}
          />
        ) : null}
        <Select
          label={t("item.subtitles")}
          value={choice.subtitleIndex === null ? "off" : String(choice.subtitleIndex)}
          onChange={(x) => {
            setChoice({ ...choice, subtitleIndex: x === "off" ? null : Number(x) });
          }}
          options={[
            { value: "off", label: t("item.subtitlesOff") },
            ...v.subtitle_tracks
              .filter((s): s is SubtitleTrack & { index: number } => typeof s.index === "number")
              .map((s) => ({ value: String(s.index), label: subtitleLabel(s, i18n.language, t("item.forced")) })),
          ]}
        />
      </div>
      <p className="text-xs text-muted-foreground">
        {[
          v.container.toUpperCase(),
          v.resolution,
          v.hdr ? "HDR" : "SDR",
          v.codec_video.toUpperCase(),
          v.codec_audio.toUpperCase(),
        ]
          .filter(Boolean)
          .join(" · ")}
      </p>
      {playError ? <Alert>{playError}</Alert> : null}
    </div>
  );
}

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
  const episodesLoad = useLoad(season === null ? null : `${serverId}/${seriesId}/${String(season)}`, () =>
    season === null ? Promise.resolve([] as Episode[]) : catalog.episodes(serverId, seriesId, season),
  );
  const episodes: Episode[] | undefined = episodesLoad.data;
  const { play, busy, error: playError } = usePlay(serverId);

  const loadError = seasonsLoad.error ?? episodesLoad.error;
  if (loadError) return <Alert>{errorMessage(t, loadError)}</Alert>;
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
                  {[e.runtime ? t("item.minutes", { count: e.runtime }) : null, e.air_date].filter(Boolean).join(" · ")}
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

export function Item() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { serverId = "", itemId = "" } = useParams();
  const { data: item, error } = useLoad(`${serverId}/${itemId}`, () => catalog.item(serverId, itemId));

  useEffect(() => {
    if (needsReauth(error)) goReauth(navigate, serverId, error);
  }, [error, navigate, serverId]);

  const meta = useMemo(() => {
    if (!item) return "";
    return [
      item.year,
      item.content_rating,
      item.runtime ? t("item.minutes", { count: item.runtime }) : null,
      item.genres.slice(0, 3).join(", "),
    ]
      .filter(Boolean)
      .join(" · ");
  }, [item, t]);

  if (error) return <Alert>{errorMessage(t, error)}</Alert>;
  if (!item) return <p className="text-muted-foreground">{t("status.loading")}</p>;

  const backdrop = artwork(serverId, item.backdrop_url);
  const poster = artwork(serverId, item.poster_url);
  const seriesId = item.type === "series" ? item.content_id : item.type === "season" ? item.series_id : null;

  return (
    <article className="flex flex-col gap-8">
      <section className="relative isolate overflow-hidden rounded-2xl border border-border/60 bg-card">
        {backdrop ? (
          <img src={backdrop} alt="" className="absolute inset-0 -z-10 size-full object-cover opacity-40" />
        ) : null}
        <div className="absolute inset-0 -z-10 bg-gradient-to-t from-card via-card/80 to-transparent" />
        <div className="flex flex-col gap-6 p-6 md:flex-row md:items-end lg:p-10">
          {poster ? (
            <img src={poster} alt="" className="hidden w-44 shrink-0 rounded-xl shadow-2xl md:block lg:w-56" />
          ) : null}
          <div className="flex min-w-0 flex-col gap-3">
            {item.series_title && item.series_id ? (
              <Link
                to={itemPath(serverId, item.series_id)}
                className="text-sm font-medium text-muted-foreground hover:underline"
              >
                {item.series_title}
                {item.season_number != null ? ` · ${t("item.seasonN", { n: item.season_number })}` : ""}
                {item.episode_number != null ? ` · ${t("item.episodeN", { n: item.episode_number })}` : ""}
              </Link>
            ) : null}
            <h1 className="text-3xl font-semibold tracking-tight lg:text-5xl">{item.title}</h1>
            {meta ? <p className="text-sm text-muted-foreground">{meta}</p> : null}
            {item.tagline ? <p className="italic text-muted-foreground">{item.tagline}</p> : null}
            {item.overview ? <p className="max-w-3xl text-sm leading-relaxed lg:text-base">{item.overview}</p> : null}
            {PLAYABLE.has(item.type) ? <PlayPanel serverId={serverId} itemId={item.content_id} /> : null}
            {!PLAYABLE.has(item.type) && item.play_content_id ? <SeriesPlay serverId={serverId} item={item} /> : null}
          </div>
        </div>
      </section>
      {seriesId ? (
        <EpisodeList
          serverId={serverId}
          seriesId={seriesId}
          initialSeason={item.type === "season" ? item.season_number : item.play_season_number}
        />
      ) : null}
      {item.cast.length > 0 ? (
        <section className="flex flex-col gap-3" aria-label={t("item.cast")}>
          <h2 className="text-lg font-semibold">{t("item.cast")}</h2>
          <ul className="grid grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-4">
            {[...item.cast]
              .sort((a, b) => a.order - b.order)
              .slice(0, 18)
              .map((c) => {
                const photo = artwork(serverId, c.photo_url);
                return (
                  <li key={`${c.name}-${c.character}`} className="flex items-center gap-3">
                    <span className="size-12 shrink-0 overflow-hidden rounded-full bg-surface-raised">
                      {photo ? <img src={photo} alt="" loading="lazy" className="size-full object-cover" /> : null}
                    </span>
                    <span className="min-w-0">
                      <span className="block truncate text-sm font-medium">{c.name}</span>
                      <span className="block truncate text-xs text-muted-foreground">{c.character}</span>
                    </span>
                  </li>
                );
              })}
          </ul>
        </section>
      ) : null}
    </article>
  );
}

/** Series/season: "Play" / "Resume" the episode the server names in `play_content_id`. */
function SeriesPlay({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const { play, busy, error } = usePlay(serverId);
  const target = item.play_content_id;
  if (!target) return null;
  const inProgress = (item.user_data?.in_progress_count ?? 0) > 0;
  return (
    <div className="flex flex-col gap-2">
      <div>
        <Button disabled={busy} className="inline-flex items-center gap-2" onClick={() => void play(target)}>
          <Play className="size-4" />
          {inProgress ? t("item.continue") : t("item.play")}
        </Button>
      </div>
      {error ? <Alert>{error}</Alert> : null}
    </div>
  );
}
