import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type BoxSet, type CatalogItem, type CollectionsOverview } from "../lib/catalog";
import { PlayerProvider } from "../player/PlayerProvider";
import { BoxSetPage, Collections } from "./Collections";

const film = (id: string, title: string) => ({ content_id: id, type: "movie", title }) as unknown as CatalogItem;

function mount(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <PlayerProvider>
        <Routes>
          <Route path="/servers/:serverId/collections" element={<Collections />} />
          <Route path="/servers/:serverId/box-set/:libraryId/:boxSetId" element={<BoxSetPage />} />
        </Routes>
      </PlayerProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Collections", () => {
  it("groups own, shared and server collections", async () => {
    const overview: CollectionsOverview = {
      mine: [{ id: "a", name: "Weekend", description: "", item_count: 3, poster_url: "", creator_profile_id: "1" }],
      shared: [{ id: "b", name: "Kids", description: "", item_count: 1, poster_url: "", creator_profile_id: "2" }],
      server: [
        { library_id: "l1", library_name: "Movies", collections: [{ id: "c", title: "Oscars", item_count: 9 }] },
        { library_id: "l2", library_name: "Shows", collections: [] },
      ],
    };
    vi.spyOn(catalog, "collections").mockResolvedValue(overview);
    mount("/servers/srv/collections");
    expect(await screen.findByRole("region", { name: "Your collections" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Shared with you" })).toBeTruthy();
    expect(screen.getByRole("region", { name: "Movies" })).toBeTruthy();
    // A library without collections gets no heading.
    expect(screen.queryByRole("region", { name: "Shows" })).toBeNull();
    expect(screen.getByText("Weekend")).toBeTruthy();
    expect(screen.getByText("Oscars")).toBeTruthy();
  });

  it("says so when there are none", async () => {
    vi.spyOn(catalog, "collections").mockResolvedValue({ mine: [], shared: [], server: [] });
    mount("/servers/srv/collections");
    expect(await screen.findByText("No collections yet.")).toBeTruthy();
  });

  it("shows a box set with its films", async () => {
    const set: BoxSet = {
      id: "bs",
      library_id: "l1",
      title: "The Trilogy",
      overview: "Three films.",
      first_year: 2001,
      last_year: 2003,
      movie_count: 3,
      played_count: 1,
      items: [film("m1", "Part One"), film("m2", "Part Two")],
    };
    const load = vi.spyOn(catalog, "boxSet").mockResolvedValue(set);
    mount("/servers/srv/box-set/l1/bs");
    expect(await screen.findByRole("heading", { name: "The Trilogy" })).toBeTruthy();
    expect(load).toHaveBeenCalledWith("srv", "l1", "bs");
    expect(screen.getByText("2001 – 2003 · 3 films · 1 of 3 watched")).toBeTruthy();
    expect(screen.getAllByText("Part One").length).toBeGreaterThan(0);
  });
});
