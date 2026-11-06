import de from "./de.json";
import en from "./en.json";
import { detectLanguage } from "./index";

function keys(o: object, prefix = ""): string[] {
  return Object.entries(o).flatMap(([k, v]) =>
    typeof v === "object" && v !== null ? keys(v as object, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("i18n", () => {
  it("German has exactly the English keys", () => {
    expect(keys(de).sort()).toEqual(keys(en).sort());
  });

  it("detects the language from locales with English fallback", () => {
    expect(detectLanguage(["de-CH", "en-US"])).toBe("de");
    expect(detectLanguage(["fr-FR"])).toBe("en");
    expect(detectLanguage([])).toBe("en");
  });
});
