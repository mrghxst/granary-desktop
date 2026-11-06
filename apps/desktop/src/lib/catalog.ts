import { invoke, isTauri } from "@tauri-apps/api/core";
import type { CommandError } from "./silo";

/*
 * Browse/detail data. These mirror crates/silo-api/src/catalog.rs, which are lenient subsets of the Silo OpenAPI
 * schemas (snake_case on the wire, kept as-is here). Artwork URLs are presigned paths on the server; they are only
 * ever loaded through the host's `granary-img` scheme (see `artwork()`), never directly.
 */

export interface CatalogItemUserState {
  played: boolean;
  is_favorite: boolean;
  in_watchlist: boolean;
  watched_count?: number | null;
  unplayed_count?: number | null;
}

export interface WatchRollup {
  played: boolean;
  position_seconds?: number | null;
  duration_seconds?: number | null;
  is_in_progress?: boolean | null;
  last_file_id?: string | null;
  watched_count: number;
  unplayed_count: number;
  in_progress_count: number;
}

export interface CatalogItem {
  content_id: string;
  title: string;
  type: string;
  year?: number | null;
  overview?: string | null;
  runtime?: number | null;
  duration_seconds?: number | null;
  content_rating?: string | null;
  rating_tmdb?: number | null;
  genres: string[];
  poster_url?: string | null;
  backdrop_url?: string | null;
  logo_url?: string | null;
  series_id?: string | null;
  series_title?: string | null;
  series_poster_url?: string | null;
  season_number?: number | null;
  episode_number?: number | null;
  play_content_id?: string | null;
  position_seconds?: number | null;
  item_source?: string | null;
  user_state?: CatalogItemUserState | null;
}

export interface Section {
  id: string;
  title: string;
  section_type: string;
  featured: boolean;
  items: CatalogItem[];
  has_more?: boolean | null;
  total_count: number;
}

export interface PageInfo {
  has_more: boolean;
  next_cursor?: string | null;
}

export interface CatalogPage {
  items: CatalogItem[];
  page?: PageInfo | null;
  total: number;
  total_exact: boolean;
}

export interface AudioTrack {
  codec?: string | null;
  profile?: string | null;
  channels?: number | null;
  layout?: string | null;
  language?: string | null;
  title?: string | null;
  embedded_title?: string | null;
  default: boolean;
}

export interface SubtitleTrack {
  index?: number | null;
  codec?: string | null;
  language?: string | null;
  title?: string | null;
  embedded_title?: string | null;
  forced: boolean;
  hearing_impaired: boolean;
  default: boolean;
  external: boolean;
}

export interface FileVersion {
  file_id: string;
  container: string;
  codec_video: string;
  codec_audio: string;
  resolution: string;
  hdr: boolean;
  duration_seconds: number;
  file_size: number;
  edition_key?: string | null;
  edition_raw?: string | null;
  effective_audio_track_index?: number | null;
  audio_tracks: AudioTrack[];
  subtitle_tracks: SubtitleTrack[];
}

export interface CastCredit {
  name: string;
  character: string;
  order: number;
  person_id?: string | null;
  photo_url?: string | null;
}

export interface CatalogRating {
  source: string;
  name: string;
  display: string;
  score: number;
}

export interface CatalogItemDetail {
  content_id: string;
  title: string;
  type: string;
  original_title?: string | null;
  tagline?: string | null;
  overview?: string | null;
  year?: number | null;
  release_date?: string | null;
  runtime?: number | null;
  content_rating?: string | null;
  genres: string[];
  studios: string[];
  ratings: CatalogRating[];
  cast: CastCredit[];
  crew: { name: string; job: string }[];
  poster_url?: string | null;
  backdrop_url?: string | null;
  logo_url?: string | null;
  series_id?: string | null;
  series_title?: string | null;
  season_number?: number | null;
  episode_number?: number | null;
  season_count?: number | null;
  episode_count?: number | null;
  play_content_id?: string | null;
  play_season_number?: number | null;
  user_data?: WatchRollup | null;
  user_state?: CatalogItemUserState | null;
  versions: FileVersion[];
}

export interface Season {
  content_id: string;
  title: string;
  season_number: number;
  episode_count: number;
  is_specials?: boolean | null;
  overview?: string | null;
  poster_url?: string | null;
  play_content_id?: string | null;
  user_data?: WatchRollup | null;
}

export interface Episode {
  content_id: string;
  title: string;
  season_number: number;
  episode_number: number;
  runtime: number;
  air_date?: string | null;
  overview?: string | null;
  still_url?: string | null;
  user_data?: WatchRollup | null;
}

export interface WatchDetail {
  content_id: string;
  title: string;
  type: string;
  series_title?: string | null;
  season_number?: number | null;
  episode_number?: number | null;
  versions: FileVersion[];
  user_data?: WatchRollup | null;
  effective_subtitle_language?: string | null;
  effective_subtitle_mode?: string | null;
}

export interface CatalogQuery {
  libraryId?: string;
  q?: string;
  type?: string;
  sort?: string;
  cursor?: string;
  limit?: number;
}

/** What the native player needs to start (Phase 6 plays it; the browser video-element path does not exist). */
export interface PlayRequest {
  serverId: string;
  itemId: string;
  fileId: string;
  startSeconds: number;
  /** Position in the version's `audio_tracks`. */
  audioIndex: number | null;
  /** `SubtitleTrack.index` of the chosen subtitle, or null for off. */
  subtitleIndex: number | null;
}

const noHost = (): Error & CommandError => Object.assign(new Error("not running inside Granary"), { code: "no_host" });

function call<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  if (!isTauri()) return Promise.reject(noHost());
  return invoke<T>(cmd, args);
}

export const catalog = {
  homeSections: (serverId: string) => call<Section[]>("home_sections", { serverId }),
  browse: (serverId: string, query: CatalogQuery) => call<CatalogPage>("catalog_browse", { serverId, query }),
  item: (serverId: string, itemId: string) => call<CatalogItemDetail>("catalog_item", { serverId, itemId }),
  seasons: (serverId: string, seriesId: string) => call<Season[]>("series_seasons", { serverId, seriesId }),
  episodes: (serverId: string, seriesId: string, season: number) =>
    call<Episode[]>("season_episodes", { serverId, seriesId, season }),
  watch: (serverId: string, itemId: string) => call<WatchDetail>("watch_state", { serverId, itemId }),
  play: (request: PlayRequest) => call<null>("player_play", { request }),
};

/** Image URL for a server artwork reference, served by the host's `granary-img` scheme (WebView2 form). */
export function artwork(serverId: string, url: string | null | undefined): string | undefined {
  if (!url) return undefined;
  return `http://granary-img.localhost/${encodeURIComponent(serverId)}?u=${encodeURIComponent(url)}`;
}
