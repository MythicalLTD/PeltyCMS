import en from "../../../locales/en.json";

/** Every UI string key. Using an unknown key is a compile error. */
export type TranslationKey = keyof typeof en;
export type Messages = Partial<Record<string, string>>;
export const baseMessages: Messages = en;

/**
 * `{name}` is the UI syntax; `{{name}}` (the e-mail template syntax) is
 * accepted too, so a text copied from a template or an older override still
 * renders without stray braces. Unknown names are left as written.
 */
export function interpolate(template: string, vars?: Record<string, string | number>): string {
  if (!vars) return template;
  return template.replace(/\{\{\s*(\w+)\s*\}\}|\{(\w+)\}/g, (m, a: string | undefined, b: string | undefined) => {
    const k = (a ?? b)!;
    return k in vars ? String(vars[k]) : m;
  });
}

/** Look up `key` in `messages`, fall back to English, then to the key. */
/**
 * Plurals: when `vars.count` is a number and a `key#<rule>` variant exists
 * (CLDR rule from Intl.PluralRules: zero/one/two/few/many/other), it wins.
 */
export function translate(messages: Messages, key: string, vars?: Record<string, string | number>, lang = "en"): string {
  if (typeof vars?.count === "number") {
    const rule = new Intl.PluralRules(lang).select(vars.count);
    const v = messages[`${key}#${rule}`] ?? messages[`${key}#other`];
    if (v) return interpolate(v, vars);
  }
  return interpolate(messages[key] ?? baseMessages[key] ?? key, vars);
}

export function readCookie(name: string, cookie: string): string | null {
  for (const part of cookie.split(";")) {
    const [k, ...v] = part.trim().split("=");
    if (k === name) return decodeURIComponent(v.join("="));
  }
  return null;
}

export function pickLanguage(available: string[], cookieLang: string | null, browser: readonly string[]): string {
  if (cookieLang && available.includes(cookieLang)) return cookieLang;
  for (const l of browser) {
    const lower = l.toLowerCase();
    if (available.includes(lower)) return lower;
    const primary = lower.split("-")[0];
    if (primary && available.includes(primary)) return primary;
  }
  return "en";
}

/** Translate a server code; unknown `status.*` codes (e.g. custom ticket statuses) fall back to a readable label. */
export function translateCode(messages: Messages, code: string): string {
  const v = messages[code] ?? baseMessages[code];
  if (v) return v;
  const m = /^status\.(.+)$/.exec(code);
  if (m) {
    const alt = messages[`tickets.status.${m[1]}`] ?? baseMessages[`tickets.status.${m[1]}`];
    if (alt) return alt;
    const words = m[1]!.replace(/[_-]+/g, " ").trim();
    return words.charAt(0).toUpperCase() + words.slice(1);
  }
  return code;
}
