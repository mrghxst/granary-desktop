import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { vi } from "vitest";
import { AppRoutes } from "../App";
import i18n from "../i18n";
import type { CommandError, ProfileView, ServerView } from "../lib/silo";

/* A fake host behind `invoke`, mirroring the Rust mock server (alice / pw, Kids PIN 1234). */
const host = vi.hoisted(() => {
  const state = {
    servers: [] as ServerView[],
    signedIn: false,
    calls: [] as { cmd: string; args?: Record<string, unknown> | undefined }[],
    revokedHandler: null as ((e: { payload: { serverId: string } }) => void) | null,
    revokeNext: false,
    profileSelected: false,
  };
  return state;
});

/* Catalog data shaped like the OAS schemas (snake_case), mirroring the Rust mock's demo catalog. */
const catalogData = vi.hoisted(() => {
  const movie = {
    content_id: "m1",
    title: "Northern Lights",
    type: "movie",
    year: 2023,
    runtime: 118,
    duration_seconds: 7080,
    genres: ["Drama"],
    poster_url: "/api/v2/artwork/poster-m1?exp=1&sig=a",
    backdrop_url: "/api/v2/artwork/backdrop-m1?exp=1&sig=a",
    position_seconds: 1520,
  };
  const version = {
    file_id: "m1-f1",
    container: "mkv",
    codec_video: "hevc",
    codec_audio: "eac3",
    resolution: "2160p",
    hdr: true,
    duration_seconds: 7080,
    file_size: 0,
    audio_tracks: [
      { codec: "eac3", channels: 6, language: "eng", default: true },
      { codec: "ac3", channels: 6, language: "deu", default: false },
    ],
    subtitle_tracks: [
      {
        index: 2,
        codec: "subrip",
        language: "eng",
        forced: false,
        hearing_impaired: false,
        default: false,
        external: false,
      },
      {
        index: 3,
        codec: "subrip",
        language: "deu",
        forced: true,
        hearing_impaired: false,
        default: true,
        external: false,
      },
    ],
  };
  const userData = {
    played: false,
    position_seconds: 1520,
    duration_seconds: 7080,
    is_in_progress: true,
    watched_count: 0,
    unplayed_count: 0,
    in_progress_count: 1,
  };
  return { movie, version, userData };
});

const SERVER_ID = "srv-1";
const ORIGIN = "https://media.example.test";
const profiles: ProfileView[] = [
  { id: "p-main", name: "Alice", hasPin: false, isChild: false },
  { id: "p-kids", name: "Kids", hasPin: true, isChild: true },
];
const fail = (code: string): Error & CommandError => Object.assign(new Error(code), { code });

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    host.calls.push({ cmd, args });
    switch (cmd) {
      case "app_info":
        return Promise.resolve({ version: "0.1.0", libmpv: { ok: true, apiVersion: "2.6" } });
      case "servers_list":
        return Promise.resolve(host.servers);
      case "servers_probe":
        if (args?.url !== "media.example.test") return Promise.reject(fail("not_silo"));
        return Promise.resolve({
          origin: ORIGIN,
          serverId: SERVER_ID,
          serverVersion: "1.4.0",
          apiMajor: 2,
          untested: true,
          insecure: false,
          known: false,
        });
      case "servers_add": {
        const s: ServerView = {
          serverId: SERVER_ID,
          origin: ORIGIN,
          serverVersion: "1.4.0",
          untested: true,
          insecure: false,
          active: true,
        };
        host.servers = [s];
        return Promise.resolve(s);
      }
      case "auth_login":
        if (args?.username !== "alice" || args.password !== "pw") return Promise.reject(fail("invalid_credentials"));
        host.signedIn = true;
        host.servers = host.servers.map((s) => ({ ...s, account: { userId: "u1", username: "alice" } }));
        return Promise.resolve({ account: { userId: "u1", username: "alice" }, passwordChangeRequired: false });
      case "profiles_list":
        return host.signedIn ? Promise.resolve(profiles) : Promise.reject(fail("not_signed_in"));
      case "profiles_select":
        if (args?.profileId === "p-kids" && args.pin !== "1234") return Promise.reject(fail("wrong_pin"));
        host.profileSelected = true;
        return Promise.resolve(null);
      case "home_sections":
        if (!host.profileSelected) return Promise.reject(fail("profile_required"));
        return Promise.resolve([
          {
            id: "featured",
            title: "Featured",
            section_type: "featured",
            featured: true,
            items: [catalogData.movie],
            total_count: 1,
          },
          {
            id: "recent",
            title: "Recently added",
            section_type: "recently_added",
            featured: false,
            items: [catalogData.movie],
            total_count: 1,
          },
        ]);
      case "catalog_browse":
        return Promise.resolve({ items: [catalogData.movie], page: { has_more: false }, total: 1, total_exact: true });
      case "catalog_item":
        return Promise.resolve({
          ...catalogData.movie,
          genres: ["Drama"],
          studios: [],
          ratings: [],
          cast: [{ name: "Ada Brook", character: "Lead", order: 0 }],
          crew: [],
          versions: [catalogData.version],
          user_data: catalogData.userData,
        });
      case "watch_state":
        return Promise.resolve({
          content_id: "m1",
          title: "Northern Lights",
          type: "movie",
          versions: [catalogData.version],
          user_data: catalogData.userData,
          effective_subtitle_mode: "off",
        });
      case "player_play":
        return Promise.resolve(null);
      case "settings_get":
        return Promise.reject(fail("internal"));
      case "libraries_list":
        if (host.revokeNext) {
          host.signedIn = false;
          host.revokedHandler?.({ payload: { serverId: SERVER_ID } });
          return Promise.reject(fail("signed_out"));
        }
        return Promise.resolve([
          { id: "l1", name: "Movies", kind: "movie" },
          { id: "l2", name: "Shows", kind: "show" },
        ]);
      default:
        return Promise.reject(fail("internal"));
    }
  },
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (_event: string, handler: (e: { payload: { serverId: string } }) => void) => {
    host.revokedHandler = handler;
    return Promise.resolve(() => {
      host.revokedHandler = null;
    });
  },
}));

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <AppRoutes />
    </MemoryRouter>,
  );
}

beforeEach(async () => {
  await i18n.changeLanguage("en");
  host.servers = [];
  host.signedIn = false;
  host.calls = [];
  host.revokeNext = false;
  host.profileSelected = false;
});

describe("connect flow", () => {
  it("adds a server, signs in, unlocks a PIN profile and lists libraries", async () => {
    renderAt("/");
    fireEvent.click(await screen.findByRole("button", { name: "Connect to a server" }));

    // Probe: an address that is not a Silo server shows the mapped message.
    const address = await screen.findByLabelText("Server address");
    fireEvent.change(address, { target: { value: "nope.example.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    expect(await screen.findByText("No Silo server (API v2) answers at this address.")).toBeTruthy();

    fireEvent.change(address, { target: { value: "media.example.test" } });
    fireEvent.click(screen.getByRole("button", { name: "Check" }));
    expect(await screen.findByText("Found a Silo server")).toBeTruthy();
    expect(screen.getByText(/not been tested with this server version/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Add server" }));

    // Login: wrong password first.
    expect(await screen.findByRole("heading", { name: `Sign in to ${ORIGIN}` })).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "alice" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "nope" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText("Wrong username or password.")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "pw" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));

    // Profiles: the Kids profile asks for its PIN; a wrong one is rejected.
    fireEvent.click(await screen.findByRole("button", { name: /Kids/ }));
    const pin = screen.getByLabelText("PIN for Kids");
    fireEvent.change(pin, { target: { value: "0000" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));
    expect(await screen.findByText("Wrong PIN.")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("PIN for Kids"), { target: { value: "1234" } });
    fireEvent.click(screen.getByRole("button", { name: "Unlock" }));

    // Server home inside the shell: featured hero, rows, libraries in the sidebar.
    expect(await screen.findByRole("heading", { name: "Northern Lights", level: 1 })).toBeTruthy();
    expect(await screen.findByRole("link", { name: "Movies" })).toBeTruthy();
    expect(screen.getByRole("link", { name: "Shows" })).toBeTruthy();

    // The password never travels anywhere but the auth_login command.
    const withPassword = host.calls.filter((c) => JSON.stringify(c.args ?? {}).includes('"pw"'));
    expect(withPassword.map((c) => c.cmd)).toEqual(["auth_login"]);
    expect(host.calls.find((c) => c.cmd === "profiles_select" && c.args?.pin === "1234")).toBeTruthy();
  });

  it("returns to sign-in when the server revokes the session", async () => {
    host.servers = [
      {
        serverId: SERVER_ID,
        origin: ORIGIN,
        serverVersion: "1.4.0",
        untested: false,
        insecure: false,
        active: true,
        account: { userId: "u1", username: "alice" },
      },
    ];
    host.signedIn = true;
    host.revokeNext = true;
    host.profileSelected = true;
    renderAt(`/servers/${SERVER_ID}/home`);
    await waitFor(() => {
      expect(host.revokedHandler).not.toBeNull();
    });
    expect(await screen.findByRole("heading", { name: `Sign in to ${ORIGIN}` })).toBeTruthy();
    expect(screen.getByText("Your session ended. Please sign in again.")).toBeTruthy();
  });

  it("lists saved servers with their account state", async () => {
    host.servers = [
      {
        serverId: SERVER_ID,
        origin: ORIGIN,
        serverVersion: "1.4.0",
        untested: false,
        insecure: true,
        active: true,
        account: { userId: "u1", username: "alice" },
      },
    ];
    renderAt("/");
    expect(await screen.findByText(ORIGIN)).toBeTruthy();
    expect(screen.getByText(/Signed in as alice/)).toBeTruthy();
    expect(screen.getByText(/Unencrypted/)).toBeTruthy();
    expect(screen.getByTestId("status").textContent).toContain("libmpv ready");
  });

  it("browses a library, opens details and hands the chosen file to the native player", async () => {
    host.servers = [
      {
        serverId: SERVER_ID,
        origin: ORIGIN,
        serverVersion: "1.4.0",
        untested: false,
        insecure: false,
        active: true,
        account: { userId: "u1", username: "alice" },
      },
    ];
    host.signedIn = true;
    host.profileSelected = true;
    renderAt(`/servers/${SERVER_ID}/library/l1`);
    expect(await screen.findByText("1 item")).toBeTruthy();
    const poster = await screen.findByRole("link", { name: "Northern Lights, 2023" });
    // Artwork only through the host scheme, never a direct server URL.
    const img = poster.querySelector("img");
    expect(img?.getAttribute("src")).toMatch(/^http:\/\/granary-img\.localhost\/srv-1\?u=%2Fapi%2Fv2%2Fartwork%2F/);
    fireEvent.click(poster);

    expect(await screen.findByRole("heading", { name: "Northern Lights", level: 1 })).toBeTruthy();
    expect(screen.getByText("Ada Brook")).toBeTruthy();
    // Pick the German audio track, then resume.
    fireEvent.change(await screen.findByLabelText("Audio"), { target: { value: "1" } });
    fireEvent.click(screen.getByRole("button", { name: "Resume from 25:20" }));
    await waitFor(() => {
      expect(host.calls.find((c) => c.cmd === "player_play")).toBeTruthy();
    });
    const play = host.calls.find((c) => c.cmd === "player_play");
    expect(play?.args).toEqual({
      request: {
        serverId: SERVER_ID,
        itemId: "m1",
        fileId: "m1-f1",
        startSeconds: 1520,
        audioIndex: 1,
        // Mode "off" with German audio picked: only the forced German track shows (web-app rule).
        subtitleIndex: 3,
      },
    });
    expect(document.querySelector("video")).toBeNull();
  });

  it("sends the user to the profile picker when no profile is selected", async () => {
    host.servers = [
      {
        serverId: SERVER_ID,
        origin: ORIGIN,
        serverVersion: "1.4.0",
        untested: false,
        insecure: false,
        active: true,
        account: { userId: "u1", username: "alice" },
      },
    ];
    host.signedIn = true;
    renderAt(`/servers/${SERVER_ID}/home`);
    expect(await screen.findByRole("button", { name: /Kids/ })).toBeTruthy();
  });
});
