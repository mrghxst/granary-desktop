import type { CatalogItem, HomeDismissal, HomeSurface } from "./catalog";

export type CardAction = "play" | "fromStart" | "watched" | "unwatched" | "dismiss" | "details";

/** The dismissal for a card on a home surface, when the server has what it needs to anchor it. */
export function cardDismissal(item: CatalogItem, surface: HomeSurface | undefined): HomeDismissal | null {
  if (!surface) return null;
  // Next-up cards (also merged into Continue Watching) hold for their series; progress cards for their progress stamp
  // (`internal/apiv2/home.go@6791aac` L72-75).
  if (item.item_source === "next_up" || surface === "next_up") {
    return item.series_id ? { surface: "next_up", item_id: item.content_id, series_id: item.series_id } : null;
  }
  return item.progress_updated_at
    ? { surface: "continue_watching", item_id: item.content_id, progress_updated_at: item.progress_updated_at }
    : null;
}

/**
 * Right-click actions, only where they apply (the web app's card menu, `web/src` MediaCard details button): play;
 * from the beginning when there is progress; watched or unwatched by state (a show part-watched gets both); removing
 * from a home surface; details.
 */
export function cardActions(
  item: CatalogItem,
  opts: { canPlay: boolean; surface?: HomeSurface | undefined; played: boolean },
) {
  const actions: CardAction[] = [];
  const progress = (item.position_seconds ?? 0) > 0;
  const partly = (item.user_state?.watched_count ?? 0) > 0;
  if (opts.canPlay) actions.push("play");
  if (opts.canPlay && progress) actions.push("fromStart");
  if (!opts.played) actions.push("watched");
  if (opts.played || progress || partly) actions.push("unwatched");
  if (cardDismissal(item, opts.surface)) actions.push("dismiss");
  actions.push("details");
  return actions;
}
