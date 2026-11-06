// Generate the Phase 5 contract fixtures (browse, details, watch state) from the OpenAPI subset: every required field
// gets a schema-conforming placeholder, then hand-picked values are merged on top. The contract tests validate the
// result against the same subset, so a fixture can never drift from the upstream schema unnoticed.
//
// Usage (repo root): node scripts/gen-catalog-fixtures.mjs
// Data is synthetic (no real titles, servers or users).
import { readFileSync, writeFileSync } from "node:fs";

const dir = "crates/silo-api/tests/fixtures";
const subset = JSON.parse(readFileSync(`${dir}/openapi-subset.json`, "utf8"));
const schemas = subset.schemas;

function placeholder(schema) {
  if (schema.$ref) return placeholder(schemas[schema.$ref.replace("#/components/schemas/", "")]);
  if (schema.enum) return schema.enum[0];
  const type = Array.isArray(schema.type) ? schema.type.find((t) => t !== "null") : schema.type;
  switch (type) {
    case "object": {
      const out = {};
      for (const key of schema.required ?? []) out[key] = placeholder(schema.properties?.[key] ?? {});
      return out;
    }
    case "array":
      return [];
    case "string":
      if (schema.format === "date-time") return "2026-01-01T00:00:00Z";
      if (schema.format === "date") return "2026-01-01";
      return "x".repeat(Math.max(1, schema.minLength ?? 1));
    case "integer":
    case "number":
      return schema.minimum ?? 0;
    case "boolean":
      return false;
    default:
      return null;
  }
}

/** Placeholder for a named schema with `values` deep-merged on top (arrays replaced). */
function make(name, values = {}) {
  const base = placeholder({ $ref: `#/components/schemas/${name}` });
  const merge = (a, b) => {
    if (b === undefined) return a;
    if (b === null || typeof b !== "object" || Array.isArray(b) || a === null || typeof a !== "object") return b;
    const out = { ...a };
    for (const [k, v] of Object.entries(b)) out[k] = merge(a[k], v);
    return out;
  };
  return merge(base, values);
}

const art = (key) => `/api/v2/artwork/${key}?exp=4102444800&sig=fixture`;

const movie = make("CatalogItem", {
  content_id: "m1",
  type: "movie",
  title: "Northern Lights",
  status: "available",
  year: 2023,
  runtime: 118,
  genres: ["Drama"],
  poster_url: art("poster-m1"),
  backdrop_url: art("backdrop-m1"),
});

const audio = (language, codec, isDefault) =>
  make("AudioTrack", { language, codec, channels: 6, default: isDefault });
const subtitle = (index, language, forced) =>
  make("VersionSubtitleTrack", { index, language, codec: "subrip", forced });

const version = make("FileVersion", {
  file_id: "m1-f1",
  container: "mkv",
  codec_video: "hevc",
  codec_audio: "eac3",
  resolution: "2160p",
  hdr: true,
  duration: 7080,
  file_size: 1,
  audio_tracks: [audio("eng", "eac3", true), audio("deu", "ac3", false)],
  subtitle_tracks: [subtitle(2, "eng", false), subtitle(3, "deu", true)],
});

const fixtures = {
  "home_sections.json": make("SectionCollection", {
    sections: [
      make("Section", {
        id: "featured",
        section_type: "featured",
        title: "Featured",
        featured: true,
        item_limit: 20,
        total_count: 1,
        items: [movie],
      }),
    ],
  }),
  "catalog_browse.json": make("CatalogBrowseCollection", {
    items: [movie],
    page: make("PageInfo", { has_more: true, next_cursor: "cursor-2" }),
    total: 2,
    total_exact: true,
  }),
  "catalog_item.json": make("CatalogItemDetail", {
    content_id: "m1",
    type: "movie",
    title: "Northern Lights",
    status: "available",
    genres: ["Drama"],
    overview: "A night-shift radio host follows a signal across the tundra.",
    cast: [make("CastCredit", { name: "Ada Brook", character: "Lead", order: 0 })],
    versions: [version],
    user_data: make("WatchRollup", { position_seconds: 1520, duration_seconds: 7080 }),
  }),
  "series_seasons.json": make("SeasonCollection", {
    items: [make("Season", { content_id: "s1-1", season_number: 1, title: "Season 1", episode_count: 3 })],
  }),
  "season_episodes.json": make("EpisodeCollection", {
    items: [1, 2, 3].map((n) =>
      make("Episode", {
        content_id: `e${n}`,
        season_number: 1,
        episode_number: n,
        title: `Episode ${n}`,
        runtime: 48,
        still_url: art(`still-e${n}`),
      }),
    ),
  }),
  "watch_detail.json": make("WatchDetail", {
    content_id: "m1",
    type: "movie",
    title: "Northern Lights",
    versions: [
      make("WatchFileVersion", {
        file_id: "m1-f1",
        duration_seconds: 7080,
        audio_tracks: [make("WatchAudioTrack", { language: "eng", codec: "eac3", default: true })],
        subtitle_tracks: [make("WatchSubtitleTrack", { index: 3, language: "deu", forced: true })],
      }),
    ],
    effective_subtitle_mode: "forced_only",
  }),
};

for (const [name, value] of Object.entries(fixtures)) {
  writeFileSync(`${dir}/${name}`, `${JSON.stringify(value, null, 2)}\n`);
}
console.log(`wrote ${Object.keys(fixtures).length} fixtures`);
