import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type CatalogItem, type RecommendationRow } from "../lib/catalog";
import { PlayerProvider } from "../player/PlayerProvider";
import { RecommendationSection, Recommendations } from "./Recommendations";

const film = (id: string, title: string) => ({ content_id: id, type: "movie", title }) as unknown as CatalogItem;

function mount(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <PlayerProvider>
        <Routes>
          <Route path="/servers/:serverId/recommendations" element={<Recommendations />} />
          <Route path="/servers/:serverId/recommendations/:kind" element={<RecommendationSection />} />
          <Route path="/servers/:serverId/recommendations/:kind/:key" element={<RecommendationSection />} />
        </Routes>
      </PlayerProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Recommendations", () => {
  it("shows the rows, the taste summary and a way into each row that has a page", async () => {
    const rows: RecommendationRow[] = [
      { type: "cluster", title: "Because you enjoy Crime", kind: "cluster", key: "0", items: [film("m1", "Heat")] },
      { type: "popular", title: "Popular", items: [film("m2", "Ronin")] },
    ];
    vi.spyOn(catalog, "discover").mockResolvedValue(rows);
    vi.spyOn(catalog, "tasteProfile").mockResolvedValue({ top_genres: ["Crime"], favorite_directors: ["M. Mann"] });
    mount("/servers/srv/recommendations");
    const link = await screen.findByRole("link", { name: "Because you enjoy Crime" });
    expect(link.getAttribute("href")).toBe("/servers/srv/recommendations/cluster/0");
    // A row without a page has a plain heading.
    expect(screen.getByRole("heading", { name: "Popular" })).toBeTruthy();
    expect(screen.queryByRole("link", { name: "Popular" })).toBeNull();
    expect(screen.getByText("Picked from what you watch and like.")).toBeTruthy();
    expect(await screen.findByText("M. Mann")).toBeTruthy();
    expect(screen.getAllByText("Heat").length).toBeGreaterThan(0);
  });

  it("says so when the server has nothing to recommend, and stands without a taste summary", async () => {
    vi.spyOn(catalog, "discover").mockResolvedValue([]);
    vi.spyOn(catalog, "tasteProfile").mockRejectedValue(new Error("no"));
    mount("/servers/srv/recommendations");
    expect(await screen.findByText("No recommendations yet")).toBeTruthy();
  });

  it("shows one row in full", async () => {
    const load = vi.spyOn(catalog, "recommendationSection").mockResolvedValue({
      type: "cluster",
      title: "Because you enjoy Crime",
      kind: "cluster",
      key: "0",
      items: [film("m1", "Heat"), film("m2", "Ronin")],
    });
    mount("/servers/srv/recommendations/cluster/0");
    expect(await screen.findByRole("heading", { name: "Because you enjoy Crime" })).toBeTruthy();
    expect(load).toHaveBeenCalledWith("srv", "cluster", "0");
    expect(screen.getByText("2 titles")).toBeTruthy();
  });
});
