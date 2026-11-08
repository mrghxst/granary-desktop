import { describe, expect, it } from "vitest";
import { cardActions, cardDismissal } from "./cardActions";
import type { CatalogItem } from "./catalog";

const item = (over: Partial<CatalogItem> = {}): CatalogItem =>
  ({ content_id: "m1", title: "Movie", type: "movie", ...over }) as CatalogItem;

describe("card context menu", () => {
  it("offers only what applies", () => {
    expect(cardActions(item(), { canPlay: true, played: false })).toEqual(["play", "watched", "details"]);
    expect(cardActions(item(), { canPlay: true, played: true })).toEqual(["play", "unwatched", "details"]);
    expect(cardActions(item({ position_seconds: 600 }), { canPlay: true, played: false })).toEqual([
      "play",
      "fromStart",
      "watched",
      "unwatched",
      "details",
    ]);
    expect(cardActions(item(), { canPlay: false, played: false })).toEqual(["watched", "details"]);
  });

  it("removes from a home surface only with the anchor the server needs", () => {
    const cw = item({ position_seconds: 60, progress_updated_at: "2026-11-01T20:00:00Z", item_source: "in_progress" });
    expect(cardDismissal(cw, "continue_watching")).toEqual({
      surface: "continue_watching",
      item_id: "m1",
      progress_updated_at: "2026-11-01T20:00:00Z",
    });
    expect(cardActions(cw, { canPlay: true, played: false, surface: "continue_watching" })).toContain("dismiss");
    expect(cardDismissal(cw, undefined)).toBeNull();
    expect(cardDismissal(item(), "continue_watching")).toBeNull();
    const nextUp = item({ content_id: "e2", type: "episode", item_source: "next_up", series_id: "s1" });
    expect(cardDismissal(nextUp, "continue_watching")).toEqual({ surface: "next_up", item_id: "e2", series_id: "s1" });
    expect(cardDismissal(item({ item_source: "next_up" }), "next_up")).toBeNull();
  });
});
