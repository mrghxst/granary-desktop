import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import de from "./de.json";
import en from "./en.json";

export const resources = { en: { translation: en }, de: { translation: de } } as const;
export const supportedLanguages = ["en", "de"] as const;

/** Pick the UI language from the OS/browser locale; English is the fallback. */
export function detectLanguage(locales: readonly string[] = navigator.languages): string {
  for (const l of locales) {
    const base = l.toLowerCase().split("-")[0];
    if (base && (supportedLanguages as readonly string[]).includes(base)) return base;
  }
  return "en";
}

void i18n.use(initReactI18next).init({
  resources,
  lng: detectLanguage(),
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

export default i18n;
