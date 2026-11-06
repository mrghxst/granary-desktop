import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { TFunction } from "i18next";
import de from "../i18n/de.json";
import en from "../i18n/en.json";
import i18n from "../i18n";
import { ERROR_CODES, errorMessage } from "./errors";

/** Codes the Rust host can produce: every `=> "code"` arm in crates/silo-api/src/error.rs. */
function rustCodes(): string[] {
  const src = readFileSync(resolve(process.cwd(), "../../crates/silo-api/src/error.rs"), "utf8");
  return [...src.matchAll(/=>\s*"([a-z_]+)"/g)].map((m) => m[1] ?? "");
}

describe("error codes", () => {
  it("covers every code the host can return", () => {
    const codes = rustCodes();
    expect(codes.length).toBeGreaterThan(20);
    for (const c of codes) expect(ERROR_CODES).toContain(c);
  });

  it("has a message for every code in both languages", () => {
    for (const c of [...ERROR_CODES, "unknown"]) {
      expect(en.errors).toHaveProperty(c);
      expect(de.errors).toHaveProperty(c);
    }
  });

  it("translates command errors and falls back for anything else", async () => {
    await i18n.changeLanguage("en");
    const t = i18n.t.bind(i18n) as TFunction;
    expect(errorMessage(t, { code: "wrong_pin", message: "x" })).toBe("Wrong PIN.");
    expect(errorMessage(t, { code: "http_error", message: "x", status: 418 })).toContain("418");
    expect(errorMessage(t, { code: "made_up", message: "x" })).toBe("Something went wrong.");
    expect(errorMessage(t, new Error("boom"))).toBe("Something went wrong.");
  });
});
