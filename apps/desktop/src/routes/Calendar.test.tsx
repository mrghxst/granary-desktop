import { fireEvent, render, screen, within } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type CalendarDay } from "../lib/catalog";
import { addDays, weekStart } from "../lib/calendarWeek";
import { Calendar } from "./Calendar";

const week = weekStart(new Date());

function mount() {
  return render(
    <MemoryRouter initialEntries={["/servers/srv/calendar"]}>
      <Routes>
        <Route path="/servers/:serverId/calendar" element={<Calendar />} />
      </Routes>
    </MemoryRouter>,
  );
}

const days: CalendarDay[] = [
  {
    date: addDays(week, 2),
    items: [
      {
        content_id: "e1",
        type: "episode",
        title: "The Long Field",
        episode_title: "Harvest",
        series_id: "s1",
        season_number: 2,
        episode_number: 5,
        air_date: addDays(week, 2),
        air_time: "21:00",
        local_air_date: addDays(week, 2),
        watched: false,
        badges: ["finale"],
      },
      {
        content_id: "m1",
        type: "movie",
        title: "Night Train",
        air_date: addDays(week, 2),
        local_air_date: addDays(week, 2),
        watched: true,
        badges: ["digital"],
      },
    ],
  },
];

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

describe("Calendar", () => {
  it("lists a week's days with their airings", async () => {
    const load = vi.spyOn(catalog, "calendar").mockResolvedValue(days);
    mount();
    const episode = await screen.findByRole("link", { name: "The Long Field, S2 · E5 · Harvest, 21:00" });
    // An episode opens its series, a film its own page.
    expect(episode.getAttribute("href")).toBe("/servers/srv/item/s1");
    expect(screen.getByRole("link", { name: "Night Train, Film" }).getAttribute("href")).toBe("/servers/srv/item/m1");
    expect(screen.getByText("Finale")).toBeTruthy();
    expect(screen.getByText("Digital")).toBeTruthy();
    expect(screen.getByLabelText("Watched")).toBeTruthy();
    expect(load).toHaveBeenCalledWith("srv", week, addDays(week, 6), "following", expect.any(String));
    // Seven day buttons; only the day with airings says so.
    const nav = screen.getByRole("navigation");
    expect(within(nav).getAllByRole("button", { pressed: false })).toHaveLength(7);
    expect(within(nav).getAllByRole("button", { name: /, 2 titles$/ })).toHaveLength(1);
  });

  it("moves between weeks and views", async () => {
    const load = vi.spyOn(catalog, "calendar").mockResolvedValue([]);
    mount();
    expect(await screen.findByText(/Nothing is scheduled this week for the titles you follow/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next week" }));
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledWith("srv", addDays(week, 7), addDays(week, 13), "following", expect.any(String));
    });
    fireEvent.click(screen.getByRole("tab", { name: "Everything" }));
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledWith("srv", addDays(week, 7), addDays(week, 13), "everything", expect.any(String));
    });
    // The view is remembered for the next visit.
    expect(localStorage.getItem("granary.calendar.filter")).toBe("everything");
  });
});
