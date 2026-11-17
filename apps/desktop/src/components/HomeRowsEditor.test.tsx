import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type HomeRowSetting } from "../lib/catalog";
import { HomeRowsEditor } from "./HomeRowsEditor";

const row = (id: string, title: string, position: number): HomeRowSetting => ({
  id,
  section_type: "recently_added",
  title,
  default_title: title,
  featured: false,
  item_limit: 20,
  hidden: false,
  is_custom: false,
  customized: false,
  position,
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("HomeRowsEditor", () => {
  it("moves and hides rows, then saves the profile's overrides", async () => {
    vi.spyOn(catalog, "homeRows").mockResolvedValue({
      rows: [row("a", "Continue watching", 0), row("b", "Recently added", 1)],
      overrides: [],
    });
    const save = vi.spyOn(catalog, "homeRowsSave").mockResolvedValue(null);
    const onClose = vi.fn();
    render(<HomeRowsEditor serverId="srv" onClose={onClose} />);

    fireEvent.click(await screen.findByRole("button", { name: "Move Recently added up" }));
    fireEvent.click(screen.getByRole("switch", { name: "Show Continue watching" }));
    expect(screen.getByRole("switch", { name: "Show Continue watching" }).getAttribute("aria-checked")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(onClose).toHaveBeenCalledWith(true);
    });
    const [, overrides] = save.mock.calls[0] ?? [];
    expect(overrides?.map((o) => [o.section_id, o.position, o.hidden ?? false])).toEqual([
      ["b", 0, false],
      ["a", 1, true],
    ]);
  });

  it("resets to the server's layout", async () => {
    vi.spyOn(catalog, "homeRows").mockResolvedValue({ rows: [row("a", "Featured", 0)], overrides: [] });
    const reset = vi.spyOn(catalog, "homeRowsReset").mockResolvedValue(null);
    const onClose = vi.fn();
    render(<HomeRowsEditor serverId="srv" onClose={onClose} />);
    fireEvent.click(await screen.findByRole("button", { name: "Use the server's layout" }));
    await waitFor(() => {
      expect(onClose).toHaveBeenCalledWith(true);
    });
    expect(reset).toHaveBeenCalledWith("srv");
  });
});
