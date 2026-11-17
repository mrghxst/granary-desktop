import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type NotificationItem } from "../lib/catalog";
import { Notifications } from "./Notifications";

function mount() {
  return render(
    <MemoryRouter initialEntries={["/servers/srv/notifications"]}>
      <Routes>
        <Route path="/servers/:serverId/notifications" element={<Notifications />} />
        <Route path="/servers/:serverId/item/:itemId" element={<p>item page</p>} />
        <Route path="/servers/:serverId/requests" element={<p>requests page</p>} />
      </Routes>
    </MemoryRouter>,
  );
}

const item = (over: Partial<NotificationItem>): NotificationItem => ({
  id: "n1",
  type: "episode.available",
  created_at: new Date().toISOString(),
  read_at: null,
  reason_flags: {},
  ...over,
});
const inbox = {
  read_cutoff: "cut",
  items: [
    item({
      series_id: "s1",
      series_title: "The Long Field",
      episode_id: "e3",
      episode_title: "Harvest",
      season_number: 1,
      episode_number: 3,
    }),
    item({ id: "n2", type: "request.declined", reason_flags: { title: "Heat", reason: "Too long" } }),
    item({ id: "n3", type: "request.approved", read_at: "2026-01-01T00:00:00.000Z", reason_flags: { title: "Ronin" } }),
    item({ id: "n4", type: "something.new", read_at: "2026-01-01T00:00:00.000Z" }),
  ],
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Notifications", () => {
  it("words each kind of entry and tells unread from read", async () => {
    vi.spyOn(catalog, "notifications").mockResolvedValue(inbox);
    mount();
    expect(await screen.findByRole("button", { name: "Unread: The Long Field" })).toBeTruthy();
    expect(screen.getByText("S1 · E3 · Harvest")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Unread: Heat" })).toBeTruthy();
    expect(screen.getByText("Your request was declined: Too long")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Ronin" })).toBeTruthy();
    expect(screen.getByText("Your request was approved.")).toBeTruthy();
    // A type this app does not know still shows.
    expect(screen.getByRole("button", { name: "Notification" })).toBeTruthy();
    expect(screen.getByText("something.new")).toBeTruthy();
  });

  it("opens an entry and marks it read", async () => {
    vi.spyOn(catalog, "notifications").mockResolvedValue(inbox);
    const read = vi.spyOn(catalog, "notificationRead").mockResolvedValue(null);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Unread: The Long Field" }));
    expect(read).toHaveBeenCalledWith("srv", "n1");
    expect(await screen.findByText("item page")).toBeTruthy();
  });

  it("marks everything read up to the list's cutoff", async () => {
    vi.spyOn(catalog, "notifications").mockResolvedValue(inbox);
    const all = vi.spyOn(catalog, "notificationsReadAll").mockResolvedValue(null);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Mark all read" }));
    expect(all).toHaveBeenCalledWith("srv", "cut");
    expect(screen.queryByRole("button", { name: /^Unread:/ })).toBeNull();
    expect(screen.queryByRole("button", { name: "Mark all read" })).toBeNull();
  });

  it("says so when the inbox is empty", async () => {
    vi.spyOn(catalog, "notifications").mockResolvedValue({ items: [], read_cutoff: "" });
    mount();
    expect(await screen.findByText("No notifications.")).toBeTruthy();
  });
});
