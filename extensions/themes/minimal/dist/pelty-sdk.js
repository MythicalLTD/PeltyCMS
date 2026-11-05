/**
 * PeltyCMS theme SDK.
 *
 * A theme can ship a complete frontend (theme.json `"frontend": "dist"`).
 * Its only contract with the server is the HTTP API under `/api/v1`
 * (machine-readable at `/api/v1/openapi.json`). This module is a tiny,
 * dependency-free typed client for that API plus an i18n helper, usable from
 * a bundler or directly as an ES module (`<script type="module">`).
 */
export class PeltyError extends Error {
    status;
    code;
    constructor(status, code, message) {
        super(message);
        this.status = status;
        this.code = code;
    }
}
export class PeltyClient {
    csrf = null;
    base;
    f;
    constructor(opts = {}) {
        this.base = (opts.baseUrl ?? "").replace(/\/$/, "");
        this.f = opts.fetch ?? fetch.bind(globalThis);
    }
    /** Low-level request: JSON in/out, session cookie, CSRF header on writes. */
    async request(method, path, body) {
        const headers = { accept: "application/json" };
        if (body !== undefined)
            headers["content-type"] = "application/json";
        if (method !== "GET" && this.csrf)
            headers["x-csrf-token"] = this.csrf;
        const res = await this.f(`${this.base}/api/v1${path}`, {
            method,
            headers,
            credentials: "include",
            body: body === undefined ? undefined : JSON.stringify(body),
        });
        if (res.status === 204)
            return undefined;
        const data = (await res.json().catch(() => null));
        if (!res.ok) {
            const e = data?.error;
            throw new PeltyError(res.status, e?.code ?? "error.internal", e?.message ?? res.statusText);
        }
        return data;
    }
    async me() {
        try {
            const me = await this.request("GET", "/auth/me");
            this.csrf = me.csrf_token;
            return me;
        }
        catch (e) {
            if (e instanceof PeltyError && e.status === 401)
                return null;
            throw e;
        }
    }
    async login(email, password) {
        const me = await this.request("POST", "/auth/login", { email, password });
        this.csrf = me.csrf_token;
        return me;
    }
    register(email, password, display_name = "") {
        return this.request("POST", "/auth/register", { email, password, display_name });
    }
    async logout() {
        await this.request("POST", "/auth/logout");
        this.csrf = null;
    }
    sessions() {
        return this.request("GET", "/auth/sessions");
    }
    setLocale(locale) {
        return this.request("PUT", "/auth/locale", { locale });
    }
    activeTheme() {
        return this.request("GET", "/themes/active");
    }
    languages() {
        return this.request("GET", "/i18n");
    }
    messages(lang) {
        return this.request("GET", `/i18n/${encodeURIComponent(lang)}`);
    }
}
/** Replace `{name}` placeholders. Unknown placeholders are left untouched. */
export function interpolate(s, vars) {
    if (!vars)
        return s;
    return s.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
}
/**
 * Load translations for the server-negotiated language with per-key English
 * fallback. Plugin strings live under `plugin.<id>.*` in the same catalog.
 */
export async function createI18n(client, lang) {
    const info = await client.languages();
    const chosen = lang ?? info.current;
    const [en, cur] = await Promise.all([
        client.messages("en"),
        chosen === "en" ? Promise.resolve(null) : client.messages(chosen).catch(() => null),
    ]);
    const t = (key, vars) => interpolate(cur?.[key] ?? en[key] ?? key, vars);
    return { lang: chosen, t, languages: info.languages };
}
/** Apply theme tokens as `--pelty-*` CSS variables (server already validated them). */
export function applyThemeTokens(theme, el = document.documentElement) {
    for (const [k, v] of Object.entries(theme.tokens))
        el.style.setProperty(`--pelty-${k}`, v);
    el.style.setProperty("color-scheme", theme.color_scheme);
}
