import { fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { afterEach, describe, expect, it, vi } from "vitest";
import "../i18n";
import { catalog, type RequestMediaResult, type TitleRequest } from "../lib/catalog";
import { Requests } from "./Requests";

function mount() {
  return render(
    <MemoryRouter initialEntries={["/servers/srv/requests"]}>
      <Routes>
        <Route path="/servers/:serverId/requests" element={<Requests />} />
      </Routes>
    </MemoryRouter>,
  );
}

const open = { state: "available", allowed: true, requests_enabled: true };
const result = (over: Partial<RequestMediaResult>): RequestMediaResult => ({
  tmdb_id: 949,
  media_type: "movie",
  title: "Heat",
  year: 1995,
  poster_path: "/heat.jpg",
  availability: "missing",
  request: { requestable: true, requested_by_viewer: false },
  ...over,
});
const made: TitleRequest = {
  id: "r-1",
  tmdb_id: 949,
  media_type: "movie",
  title: "Heat",
  year: 1995,
  state: "pending",
  outcome: "active",
  created_at: "2026-01-02T03:04:05.000Z",
  seasons: [],
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Requests", () => {
  it("offers what can be requested and sends the request", async () => {
    vi.spyOn(catalog, "requestStatus").mockResolvedValue(open);
    vi.spyOn(catalog, "requestDiscover").mockResolvedValue([
      {
        key: "trending_movies",
        title: "Trending movies",
        results: [
          result({}),
          result({ tmdb_id: 2, title: "Ronin", availability: "available", library_content_id: "m2" }),
          result({
            tmdb_id: 3,
            title: "Thief",
            request: { requestable: false, requested_by_viewer: true, state: "approved" },
          }),
          result({ tmdb_id: 4, title: "Ali", request: { requestable: false, requested_by_viewer: false } }),
        ],
      },
    ]);
    const create = vi.spyOn(catalog, "requestCreate").mockResolvedValue(made);
    mount();
    fireEvent.click(await screen.findByRole("button", { name: "Request Heat" }));
    expect(await screen.findByText("Pending")).toBeTruthy();
    expect(create).toHaveBeenCalledWith("srv", {
      media_type: "movie",
      tmdb_id: 949,
      title: "Heat",
      year: 1995,
      poster_path: "/heat.jpg",
    });
    // In the library: a way to the title, no button. Already requested or not requestable: no button either.
    expect(screen.getByRole("link", { name: "In library" }).getAttribute("href")).toBe("/servers/srv/item/m2");
    expect(screen.getByText("Approved")).toBeTruthy();
    expect(screen.getByText("Not requestable")).toBeTruthy();
    expect(screen.queryAllByRole("button", { name: /^Request / })).toHaveLength(0);
  });

  it("searches once typing pauses", async () => {
    vi.spyOn(catalog, "requestStatus").mockResolvedValue(open);
    vi.spyOn(catalog, "requestDiscover").mockResolvedValue([]);
    const search = vi
      .spyOn(catalog, "requestSearch")
      .mockResolvedValue({ page: 1, total_pages: 1, results: [result({})] });
    mount();
    fireEvent.click(await screen.findByRole("tab", { name: "Search" }));
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: " heat " } });
    expect(await screen.findByRole("button", { name: "Request Heat" }, { timeout: 2000 })).toBeTruthy();
    expect(search).toHaveBeenCalledWith("srv", "heat");
  });

  it("lists the account's requests and withdraws an open one", async () => {
    vi.spyOn(catalog, "requestStatus").mockResolvedValue(open);
    vi.spyOn(catalog, "requestDiscover").mockResolvedValue([]);
    const list = vi
      .spyOn(catalog, "requestList")
      .mockResolvedValueOnce([
        made,
        { ...made, id: "r-2", title: "Ronin", state: "available", library_content_id: "m2" },
      ])
      .mockResolvedValue([{ ...made, state: "cancelled", outcome: "cancelled" }]);
    const cancel = vi.spyOn(catalog, "requestCancel").mockResolvedValue(made);
    mount();
    fireEvent.click(await screen.findByRole("tab", { name: "My requests" }));
    fireEvent.click(await screen.findByRole("button", { name: "Withdraw the request for Heat" }));
    expect(cancel).toHaveBeenCalledWith("srv", "r-1");
    // The list is read again: the request is cancelled and can no longer be withdrawn.
    expect(await screen.findByText("Cancelled")).toBeTruthy();
    expect(list).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("button", { name: /Withdraw/ })).toBeNull();
  });

  it("says why nothing can be requested", async () => {
    vi.spyOn(catalog, "requestStatus").mockResolvedValue({
      state: "not_configured",
      allowed: false,
      requests_enabled: false,
    });
    mount();
    expect(await screen.findByText(/Requests are not set up on this server/)).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
  });
});
