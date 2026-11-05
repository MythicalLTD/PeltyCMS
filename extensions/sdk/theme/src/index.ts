/**
 * PeltyCMS theme SDK.
 *
 * A theme can ship a complete frontend (theme.json `"frontend": "dist"`).
 * Its only contract with the server is the HTTP API under `/api/v1`
 * (machine-readable at `/api/v1/openapi.json`). This module is a tiny,
 * dependency-free typed client for that API plus an i18n helper, usable from
 * a bundler or directly as an ES module (`<script type="module">`).
 */

export interface ApiErrorBody {
  error: { code: string; message: string; request_id?: string };
}

export class PeltyError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

export interface Me {
  id: string;
  email: string;
  display_name: string;
  locale: string | null;
  roles: string[];
  permissions: string[];
  csrf_token: string;
}

export interface Theme {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  color_scheme: "dark" | "light";
  tokens: Record<string, string>;
  logo_url: string | null;
  stylesheet_url: string | null;
  has_frontend: boolean;
}

export interface Language {
  code: string;
  name: string;
}

export interface Session {
  id: string;
  ip: string | null;
  user_agent: string | null;
  created_at: string;
  last_seen_at: string;
  current: boolean;
}

export interface ClientOptions {
  /** Base URL of the PeltyCMS server; empty = same origin. */
  baseUrl?: string;
  fetch?: typeof fetch;
}

export class PeltyClient {
  private csrf: string | null = null;
  private base: string;
  private f: typeof fetch;

  constructor(opts: ClientOptions = {}) {
    this.base = (opts.baseUrl ?? "").replace(/\/$/, "");
    this.f = opts.fetch ?? fetch.bind(globalThis);
  }

  /** Low-level request: JSON in/out, session cookie, CSRF header on writes. */
  async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const headers: Record<string, string> = { accept: "application/json" };
    if (body !== undefined) headers["content-type"] = "application/json";
    if (method !== "GET" && this.csrf) headers["x-csrf-token"] = this.csrf;
    const res = await this.f(`${this.base}/api/v1${path}`, {
      method,
      headers,
      credentials: "include",
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (res.status === 204) return undefined as T;
    const data = (await res.json().catch(() => null)) as unknown;
    if (!res.ok) {
      const e = (data as ApiErrorBody | null)?.error;
      throw new PeltyError(res.status, e?.code ?? "error.internal", e?.message ?? res.statusText);
    }
    return data as T;
  }

  async me(): Promise<Me | null> {
    try {
      const me = await this.request<Me>("GET", "/auth/me");
      this.csrf = me.csrf_token;
      return me;
    } catch (e) {
      if (e instanceof PeltyError && e.status === 401) return null;
      throw e;
    }
  }

  async login(email: string, password: string): Promise<Me> {
    const me = await this.request<Me>("POST", "/auth/login", { email, password });
    this.csrf = me.csrf_token;
    return me;
  }

  register(email: string, password: string, display_name = ""): Promise<{ id: string; email: string }> {
    return this.request("POST", "/auth/register", { email, password, display_name });
  }

  async logout(): Promise<void> {
    await this.request<void>("POST", "/auth/logout");
    this.csrf = null;
  }

  sessions(): Promise<{ items: Session[] }> {
    return this.request("GET", "/auth/sessions");
  }

  setLocale(locale: string | null): Promise<void> {
    return this.request("PUT", "/auth/locale", { locale });
  }

  activeTheme(): Promise<Theme> {
    return this.request("GET", "/themes/active");
  }

  languages(): Promise<{ languages: Language[]; current: string }> {
    return this.request("GET", "/i18n");
  }

  messages(lang: string): Promise<Record<string, string>> {
    return this.request("GET", `/i18n/${encodeURIComponent(lang)}`);
  }
}

/** Replace `{name}` placeholders. Unknown placeholders are left untouched. */
export function interpolate(s: string, vars?: Record<string, string | number>): string {
  if (!vars) return s;
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in vars ? String(vars[k]) : m));
}

export type Translator = (key: string, vars?: Record<string, string | number>) => string;

/**
 * Load translations for the server-negotiated language with per-key English
 * fallback. Plugin strings live under `plugin.<id>.*` in the same catalog.
 */
export async function createI18n(
  client: PeltyClient,
  lang?: string,
): Promise<{ lang: string; t: Translator; languages: Language[] }> {
  const info = await client.languages();
  const chosen = lang ?? info.current;
  const [en, cur] = await Promise.all([
    client.messages("en"),
    chosen === "en" ? Promise.resolve(null) : client.messages(chosen).catch(() => null),
  ]);
  const t: Translator = (key, vars) => interpolate(cur?.[key] ?? en[key] ?? key, vars);
  return { lang: chosen, t, languages: info.languages };
}

/** Apply theme tokens as `--pelty-*` CSS variables (server already validated them). */
export function applyThemeTokens(theme: Theme, el: HTMLElement = document.documentElement): void {
  for (const [k, v] of Object.entries(theme.tokens)) el.style.setProperty(`--pelty-${k}`, v);
  el.style.setProperty("color-scheme", theme.color_scheme);
}
