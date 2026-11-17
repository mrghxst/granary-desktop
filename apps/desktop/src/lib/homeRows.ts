/**
 * The override set Granary saves for the home page (`PUT /api/v2/profile/sections?scope=home`) after the profile
 * hid, showed or moved rows. The set replaces everything the profile saved, so it carries every saved override
 * forward as stored, the way the web app does (`silo-server/web/src/lib/homeRows/profileOverrides.ts` and
 * `homeLayoutTransfer.ts` `overrideToWrite`, @749db96a): renames, sizes and rows built in the web app survive, and
 * administrator rows the profile left alone get no override, so the administrator's later changes still reach them.
 */
import type { HomeRowOverride, HomeRowOverrideWrite, HomeRowSetting } from "./catalog";

/** The write shape of a stored override: every member that is set, nothing that is empty or null. */
export function overrideToWrite(o: HomeRowOverride): HomeRowOverrideWrite {
  const w: HomeRowOverrideWrite = { id: o.id };
  if (o.section_id) w.section_id = o.section_id;
  if (o.position != null) w.position = o.position;
  if (o.hidden) w.hidden = true;
  if (o.removed) w.removed = true;
  if (o.section_type) w.section_type = o.section_type;
  if (o.title) w.title = o.title;
  if (o.featured != null) w.featured = o.featured;
  if (o.item_limit != null) w.item_limit = o.item_limit;
  if (o.config != null) w.config = o.config;
  if (o.is_user_added) w.is_user_added = true;
  if (o.user_section_type) w.user_section_type = o.user_section_type;
  if (o.user_config != null) w.user_config = o.user_config;
  if (o.user_title) w.user_title = o.user_title;
  return w;
}

export interface HomeRowsEdit {
  /** The rows' ids in the order the profile wants them. */
  order: string[];
  /** The ids of the rows the profile hides. */
  hidden: ReadonlySet<string>;
}

/**
 * The complete override set for `edit`. Positions are stored once the profile has ordered the page (now or in an
 * earlier save); until then administrator rows keep the administrator's order.
 */
export function buildHomeOverrides(
  rows: HomeRowSetting[],
  saved: HomeRowOverride[],
  edit: HomeRowsEdit,
  newId: () => string = () => crypto.randomUUID(),
): HomeRowOverrideWrite[] {
  const byId = new Map(rows.map((r) => [r.id, r]));
  const order = edit.order.filter((id) => byId.has(id));
  const forSection = new Map<string, HomeRowOverride>();
  const ownRows = new Map<string, HomeRowOverride>();
  for (const o of saved) {
    if (o.section_id) forSection.set(o.section_id, o);
    else ownRows.set(o.id, o);
  }
  const moved = order.join("\n") !== rows.map((r) => r.id).join("\n");
  const ordered = moved || saved.some((o) => o.section_id && !o.removed && o.position != null);

  const used = new Set<HomeRowOverride>();
  const out: HomeRowOverrideWrite[] = [];
  order.forEach((id, index) => {
    const row = byId.get(id);
    if (!row) return;
    const hidden = edit.hidden.has(id);
    if (row.is_custom) {
      const own = ownRows.get(id);
      if (own) used.add(own);
      const w: HomeRowOverrideWrite = own
        ? overrideToWrite(own)
        : {
            id,
            is_user_added: true,
            user_section_type: row.section_type,
            user_title: row.title,
            ...(row.config != null ? { user_config: row.config } : {}),
          };
      w.hidden = hidden;
      if (ordered) w.position = index;
      out.push(tidy(w));
      return;
    }
    const prior = forSection.get(id);
    if (prior) used.add(prior);
    if (!prior && !hidden && !ordered) return;
    const w: HomeRowOverrideWrite = prior ? overrideToWrite(prior) : { id: newId(), section_id: id };
    if (!w.id) w.id = newId();
    w.hidden = hidden;
    if (ordered) w.position = index;
    out.push(tidy(w));
  });
  // Rows removed from the page, and overrides for rows the server no longer has, stay as they were saved.
  for (const o of saved) {
    if (used.has(o)) continue;
    const w = overrideToWrite(o);
    if (!w.id) w.id = newId();
    out.push(w);
  }
  return out;
}

/** `hidden: false` is the default: leave it out like every other member that is not set. */
function tidy(w: HomeRowOverrideWrite): HomeRowOverrideWrite {
  if (!w.hidden) delete w.hidden;
  return w;
}
