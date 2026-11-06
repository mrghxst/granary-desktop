import { render, screen } from "@testing-library/react";
import { App } from "./App";
import i18n from "./i18n";

describe("App shell", () => {
  it("renders the home screen and the host status outside Tauri", async () => {
    await i18n.changeLanguage("en");
    render(<App />);
    expect(screen.getByRole("heading", { name: "Welcome to Granary" })).toBeTruthy();
    expect(await screen.findByText(/not running inside Granary/)).toBeTruthy();
  });
});
