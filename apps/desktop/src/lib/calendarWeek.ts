/** Calendar days as `YYYY-MM-DD` strings in the viewer's own zone, and the Monday-first weeks they fall in. */

const pad = (n: number) => String(n).padStart(2, "0");

/** A local date as `YYYY-MM-DD`. */
export function isoDate(d: Date): string {
  return `${String(d.getFullYear())}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** `YYYY-MM-DD` as a local date (noon, so a daylight-saving change never moves it to the day before). */
export function parseDay(day: string): Date {
  const [y = 1970, m = 1, d = 1] = day.split("-").map(Number);
  return new Date(y, m - 1, d, 12);
}

export function addDays(day: string, n: number): string {
  const d = parseDay(day);
  d.setDate(d.getDate() + n);
  return isoDate(d);
}

/** The Monday of the week `date` falls in. */
export function weekStart(date: Date): string {
  const d = new Date(date.getFullYear(), date.getMonth(), date.getDate(), 12);
  const weekday = d.getDay(); // 0 = Sunday
  d.setDate(d.getDate() + (weekday === 0 ? -6 : 1 - weekday));
  return isoDate(d);
}

/** The seven days of the week that starts on `start`. */
export function weekDays(start: string): string[] {
  return Array.from({ length: 7 }, (_, i) => addDays(start, i));
}

export const isDay = (s: string | null): s is string => s !== null && /^\d{4}-\d{2}-\d{2}$/.test(s);

/** "Nov 16 – 22, 2026" or "16.–22. Nov. 2026": the week as the language writes a range of days. */
export function weekRange(start: string, locale: string): string {
  const format = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short", year: "numeric" });
  return format.formatRange(parseDay(start), parseDay(addDays(start, 6)));
}
