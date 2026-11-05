/** Locale-consistent formatting. en uses ISO-like dates (2026-10-29), ro uses dd.mm.yyyy. */
const DATE_LOCALE: Record<string, string> = { en: "en-CA", ro: "ro-RO" };
export const dateLocale = (lang: string) => DATE_LOCALE[lang] ?? lang;

function toDate(d: string): Date {
  return new Date(d.length === 10 ? `${d}T00:00:00Z` : d);
}

export function formatDate(d: string | null | undefined, lang: string): string {
  if (!d) return "-";
  const v = toDate(d);
  if (Number.isNaN(v.getTime())) return d;
  // Plain dates are calendar dates: render in UTC so they never shift a day.
  return v.toLocaleDateString(dateLocale(lang), { year: "numeric", month: "2-digit", day: "2-digit", ...(d.length === 10 ? { timeZone: "UTC" } : {}) });
}

export function formatDateTime(d: string | null | undefined, lang: string, timeZone?: string): string {
  if (!d) return "-";
  const v = toDate(d);
  if (Number.isNaN(v.getTime())) return d;
  return v.toLocaleString(dateLocale(lang), { year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", hour12: false, timeZone });
}

export function formatNumber(n: number | string, lang: string): string {
  return new Intl.NumberFormat(lang).format(n as unknown as number);
}

/** "Firefox on Windows" from a raw User-Agent. Unknown parts become null. */
export function parseUserAgent(ua: string | null | undefined): { browser: string | null; os: string | null } {
  if (!ua) return { browser: null, os: null };
  const browser =
    /Edg(e|A|iOS)?\//.test(ua) ? "Edge" :
    /OPR\/|Opera/.test(ua) ? "Opera" :
    /Firefox\/|FxiOS/.test(ua) ? "Firefox" :
    /CriOS|Chrome\//.test(ua) ? "Chrome" :
    /Safari\//.test(ua) ? "Safari" :
    /curl\//i.test(ua) ? "curl" : null;
  const os =
    /Windows/.test(ua) ? "Windows" :
    /iPhone|iPad|iPod/.test(ua) ? "iOS" :
    /Android/.test(ua) ? "Android" :
    /CrOS/.test(ua) ? "ChromeOS" :
    /Mac OS X|Macintosh/.test(ua) ? "macOS" :
    /Linux/.test(ua) ? "Linux" : null;
  return { browser, os };
}
