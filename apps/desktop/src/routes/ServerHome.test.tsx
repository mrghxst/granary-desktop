import { act, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type Section } from "../lib/catalog";
import { homeCache } from "../lib/homeCache";
import { PlayerProvider } from "../player/PlayerProvider";
import { ServerHome } from "./ServerHome";

const section = (title: string): Section =>
  ({
    id: "s1",
    title,
    section_type: "recently_added",
    featured: false,
    items: [{ content_id: "c1", type: "movie", title: `${title} film` }],
  }) as unknown as Section;

function mount() {
  return render(
    <MemoryRouter initialEntries={["/servers/srv/home"]}>
      <PlayerProvider>
        <Routes>
          <Route path="/servers/:serverId/home" element={<ServerHome />} />
        </Routes>
      </PlayerProvider>
    </MemoryRouter>,
  );
}

afterEach(() => {
  vi.restoreAllMocks();
  homeCache.forget("srv");
});

describe("ServerHome", () => {
  it("draws the rows loaded before at once and reloads them behind", async () => {
    let answer: (s: Section[]) => void = () => undefined;
    const load = vi.spyOn(catalog, "homeSections").mockImplementation(
      () =>
        new Promise<Section[]>((r) => {
          answer = r;
        }),
    );
    vi.spyOn(catalog, "artworkKeep").mockResolvedValue(null as never);
    // First visit: nothing to show until the server answers.
    const first = mount();
    expect(screen.queryByRole("region", { name: "Fresh" })).toBeNull();
    await act(async () => {
      answer([section("Fresh")]);
      await Promise.resolve();
    });
    expect(screen.getByRole("region", { name: "Fresh" })).toBeTruthy();
    first.unmount();
    // Coming back: the rows are there in the first render, before the reload answers.
    mount();
    expect(screen.getByRole("region", { name: "Fresh" })).toBeTruthy();
    expect(load).toHaveBeenCalledTimes(2);
    await act(async () => {
      answer([section("Newer")]);
      await Promise.resolve();
    });
    expect(screen.getByRole("region", { name: "Newer" })).toBeTruthy();
    // A changed session forgets them.
    homeCache.forget("srv");
    expect(homeCache.get("srv")).toBeUndefined();
  });
});
