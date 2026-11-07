import { act, render, screen } from "@testing-library/react";
import { App } from "./App";
import i18n from "./i18n";

describe("App shell", () => {
  it("renders the home screen and the host status outside Tauri", async () => {
    await i18n.changeLanguage("en");
    render(<App />);
    expect(screen.getByRole("heading", { name: "Welcome to Granary" })).toBeTruthy();
    expect(await screen.findByText(/not running inside Granary/)).toBeTruthy();
  });

  it("shows first-run onboarding while no server is saved", async () => {
    await i18n.changeLanguage("en");
    render(<App />);
    // Outside Tauri the server list call fails and is treated as empty: the onboarding steps are shown.
    expect(await screen.findByTestId("onboarding")).toBeTruthy();
    expect(screen.getByText(/Add your Silo server by its address/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Connect to a server" })).toBeTruthy();
  });

  it("shows an offline banner while the network is down", async () => {
    await i18n.changeLanguage("de");
    render(<App />);
    expect(screen.queryByTestId("offline")).toBeNull();
    act(() => {
      window.dispatchEvent(new Event("offline"));
    });
    expect(screen.getByTestId("offline").textContent).toContain("offline");
    act(() => {
      window.dispatchEvent(new Event("online"));
    });
    expect(screen.queryByTestId("offline")).toBeNull();
    await i18n.changeLanguage("en");
  });

  it("opens the About screen with licence, compatibility note and log tools", async () => {
    await i18n.changeLanguage("en");
    window.location.hash = "#/about";
    render(<App />);
    expect(await screen.findByRole("heading", { name: "About Granary" })).toBeTruthy();
    expect(screen.getByText(/not affiliated with or endorsed by the Silo project/)).toBeTruthy();
    expect(await screen.findByText("AGPL-3.0-or-later")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Check for updates" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Export logs" })).toBeTruthy();
    window.location.hash = "";
  });
});
