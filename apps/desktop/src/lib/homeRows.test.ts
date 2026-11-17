import { describe, expect, it } from "vitest";
import type { HomeRowOverride, HomeRowSetting } from "./catalog";
import { buildHomeOverrides } from "./homeRows";

function row(id: string, position: number, extra: Partial<HomeRowSetting> = {}): HomeRowSetting {
  return {
    id,
    section_type: "recently_added",
    title: id,
    default_title: id,
    featured: false,
    item_limit: 20,
    hidden: false,
    is_custom: false,
    customized: false,
    position,
    ...extra,
  };
}

function saved(extra: Partial<HomeRowOverride>): HomeRowOverride {
  return {
    id: "o",
    section_id: "",
    position: null,
    hidden: false,
    removed: false,
    section_type: "",
    title: "",
    featured: null,
    item_limit: null,
    is_user_added: false,
    user_section_type: "",
    user_title: "",
    ...extra,
  };
}

const ids = () => {
  let n = 0;
  return () => `new-${String(++n)}`;
};

describe("buildHomeOverrides", () => {
  const rows = [row("a", 0), row("b", 1), row("c", 2)];

  it("stores only a hidden row while the order is the server's", () => {
    const out = buildHomeOverrides(rows, [], { order: ["a", "b", "c"], hidden: new Set(["b"]) }, ids());
    expect(out).toEqual([{ id: "new-1", section_id: "b", hidden: true }]);
  });

  it("stores every row's position once the profile moves one", () => {
    const out = buildHomeOverrides(rows, [], { order: ["c", "a", "b"], hidden: new Set() }, ids());
    expect(out).toEqual([
      { id: "new-1", section_id: "c", position: 0 },
      { id: "new-2", section_id: "a", position: 1 },
      { id: "new-3", section_id: "b", position: 2 },
    ]);
  });

  it("keeps saved renames, removed rows and the profile's own rows", () => {
    const own = row("u-gems", 1, { is_custom: true, section_type: "hidden_gems", title: "Hidden gems" });
    const page = [row("a", 0, { title: "Mine", customized: true }), own];
    const prior = [
      saved({ id: "o-1", section_id: "a", title: "Mine", item_limit: 10 }),
      saved({
        id: "u-gems",
        is_user_added: true,
        user_section_type: "hidden_gems",
        user_title: "Hidden gems",
        user_config: { library_ids: [3] },
      }),
      saved({ id: "o-9", section_id: "gone", removed: true }),
    ];
    const out = buildHomeOverrides(page, prior, { order: ["a", "u-gems"], hidden: new Set(["u-gems"]) }, ids());
    expect(out).toEqual([
      { id: "o-1", section_id: "a", title: "Mine", item_limit: 10 },
      {
        id: "u-gems",
        is_user_added: true,
        user_section_type: "hidden_gems",
        user_title: "Hidden gems",
        user_config: { library_ids: [3] },
        hidden: true,
      },
      { id: "o-9", section_id: "gone", removed: true },
    ]);
  });

  it("showing a hidden row again keeps its override but clears the flag", () => {
    const prior = [saved({ id: "o-1", section_id: "b", hidden: true })];
    const out = buildHomeOverrides(rows, prior, { order: ["a", "b", "c"], hidden: new Set() }, ids());
    expect(out).toEqual([{ id: "o-1", section_id: "b" }]);
  });
});
