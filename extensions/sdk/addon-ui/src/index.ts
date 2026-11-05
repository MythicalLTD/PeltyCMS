/**
 * @peltycms/addon-ui: the frame side of the PeltyCMS addon UI bridge.
 *
 * Addon pages and slots declared with a `frame` run in a sandboxed iframe.
 * This module wraps the postMessage protocol (see the host side in
 * frontend/src/components/plugin-ui/AddonFrame.tsx) in a typed API:
 *
 * ```ts
 * import { connect } from "./pelty-ui.js";
 * const ui = await connect();              // waits for `pelty:init`
 * document.body.style.background = ui.context.theme["--background"] ?? "";
 * const stats = await ui.api.get<{ open: number }>(`/api/v1/addons/${ui.context.plugin}/stats`);
 * ui.on("record.viewed", (e) => console.log(e.kind, e.id));
 * ui.emit("refreshed", { at: Date.now() });   // host sees `<plugin>:refreshed`
 * ui.toast(ui.t({ en: "Saved", ro: "Salvat" }));
 * ```
 *
 * Everything goes through the host: requests are limited to the addon's own
 * routes plus the core GET prefixes approved at install (`bridge`).
 */

export type Area = "admin" | "client";

export interface Viewer { id: string; display_name: string; locale: string; staff: boolean; permissions: string[] }

/** Payloads of the host events (frontend/src/lib/addons/addon-events.ts). */
export interface HostEvents {
  "route.changed": { area: Area; path: string; key: string };
  "record.viewed": { area: Area; kind: string; id: string };
  "ui.theme_changed": { dark: boolean };
  "ui.locale_changed": { locale: string };
  "data.changed": { key: string };
}

export interface Context {
  plugin: string;
  /** Theme tokens (`--background`, `--primary`, …) of the host page. */
  theme: Record<string, string>;
  dark: boolean;
  locale: string;
  /** Route context: `route`, `area` and page-specific values (`id`, …). */
  ctx: Record<string, unknown> & { route?: string; area?: Area };
  viewer?: Viewer;
  /** Host events this host can deliver. */
  events: string[];
  /** Host protocol version (2 = event bus). */
  sdk: number;
  /** Short-lived UI token (also sent automatically by `api`). */
  token?: string;
}

export class BridgeError extends Error {
  constructor(public status: number, public code: string) { super(`${status} ${code}`); }
}

type Listener = (payload: never, name: string) => void;

/** Minimal window surface (tests pass a fake). */
export interface FrameWindow {
  parent: { postMessage(m: unknown, origin: string): void };
  addEventListener(t: "message", f: (e: { source: unknown; data: unknown }) => void): void;
  removeEventListener(t: "message", f: (e: { source: unknown; data: unknown }) => void): void;
}

export interface Connection {
  readonly context: Context;
  api: {
    get<T = unknown>(path: string): Promise<T>;
    post<T = unknown>(path: string, body?: unknown): Promise<T>;
    put<T = unknown>(path: string, body?: unknown): Promise<T>;
    patch<T = unknown>(path: string, body?: unknown): Promise<T>;
    delete<T = unknown>(path: string): Promise<T>;
  };
  /** Subscribe to a host or addon event (`*` wildcards: `record.*`, `billing:*`). */
  on<K extends keyof HostEvents>(name: K, f: (payload: HostEvents[K], name: string) => void): () => void;
  on(pattern: string, f: (payload: unknown, name: string) => void): () => void;
  /** Publish `<plugin>:<name>` on the host bus. */
  emit(name: string, payload?: unknown): void;
  navigate(to: string): void;
  toast(text: string, tone?: "success" | "error"): void;
  resize(height: number): void;
  /** Follow the document height (ResizeObserver). Returns a stop function. */
  autoResize(el?: Element): () => void;
  /** Pick a translation for the viewer's locale (falls back to `en`, then the first). */
  t(texts: Record<string, string>): string;
  /** Called on every (re)init: theme switch, locale change, token refresh. */
  onContext(f: (c: Context) => void): () => void;
  close(): void;
}

export function matches(pattern: string, name: string): boolean {
  if (pattern === "*" || pattern === name) return true;
  if (!pattern.includes("*")) return false;
  const re = new RegExp(`^${pattern.split("*").map((p) => p.replace(/[.+?^${}()|[\]\\]/g, "\\$&")).join(".*")}$`);
  return re.test(name);
}

/** Apply theme tokens as CSS variables on `root` (default: documentElement). */
export function applyTheme(c: Context, root?: { style: { setProperty(k: string, v: string): void }; classList?: { toggle(c: string, on: boolean): void } }) {
  const r = root ?? (globalThis as unknown as { document: { documentElement: NonNullable<typeof root> } }).document.documentElement;
  for (const [k, v] of Object.entries(c.theme)) r.style.setProperty(k, v);
  r.classList?.toggle("dark", c.dark);
}

/** Connect to the host: sends `pelty:ready` and resolves on the first `pelty:init`. */
export function connect(opts: { window?: FrameWindow; timeoutMs?: number } = {}): Promise<Connection> {
  const w = opts.window ?? (globalThis as unknown as FrameWindow);
  const host = w.parent;
  const post = (m: unknown) => host.postMessage(m, "*");
  let context: Context | null = null;
  let seq = 0;
  const pending = new Map<number, { ok: (v: unknown) => void; err: (e: unknown) => void }>();
  const listeners: Array<{ pattern: string; f: Listener }> = [];
  const ctxListeners = new Set<(c: Context) => void>();
  let first: ((c: Connection) => void) | null = null;

  const subscribe = () => post({ type: "pelty:subscribe", events: [...new Set(listeners.map((l) => l.pattern))].slice(0, 50) });

  const request = <T>(method: string, path: string, body?: unknown) =>
    new Promise<T>((ok, err) => {
      const id = ++seq;
      pending.set(id, { ok: ok as (v: unknown) => void, err });
      post({ type: "pelty:request", id, method, path, body });
    });

  const conn: Connection = {
    get context() { return context!; },
    api: {
      get: (p) => request("GET", p),
      post: (p, b) => request("POST", p, b),
      put: (p, b) => request("PUT", p, b),
      patch: (p, b) => request("PATCH", p, b),
      delete: (p) => request("DELETE", p),
    },
    on(pattern: string, f: (payload: never, name: string) => void) {
      const l = { pattern, f: f as Listener };
      listeners.push(l);
      subscribe();
      return () => { const i = listeners.indexOf(l); if (i >= 0) listeners.splice(i, 1); subscribe(); };
    },
    emit: (name, payload) => post({ type: "pelty:emit", name, payload }),
    navigate: (to) => post({ type: "pelty:navigate", to }),
    toast: (text, tone = "success") => post({ type: "pelty:toast", text, tone }),
    resize: (height) => post({ type: "pelty:resize", height }),
    autoResize(el) {
      const target = el ?? (globalThis as unknown as { document: { documentElement: Element } }).document.documentElement;
      const RO = (globalThis as unknown as { ResizeObserver?: typeof ResizeObserver }).ResizeObserver;
      if (!RO) return () => {};
      const ro = new RO(() => conn.resize(Math.ceil(target.getBoundingClientRect().height)));
      ro.observe(target);
      return () => ro.disconnect();
    },
    t(texts) {
      const loc = context?.locale ?? "en";
      return texts[loc] ?? texts[loc.split("-")[0]!] ?? texts.en ?? Object.values(texts)[0] ?? "";
    },
    onContext(f) { ctxListeners.add(f); return () => ctxListeners.delete(f); },
    close() { w.removeEventListener("message", onMessage); for (const p of pending.values()) p.err(new BridgeError(0, "error.closed")); pending.clear(); },
  };

  function onMessage(e: { source: unknown; data: unknown }) {
    if (e.source !== host || !e.data || typeof e.data !== "object") return;
    const m = e.data as Record<string, unknown>;
    if (m.type === "pelty:init") {
      context = { theme: {}, dark: false, locale: "en", ctx: {}, events: [], sdk: 1, ...(m as object) } as unknown as Context;
      delete (context as unknown as Record<string, unknown>).type;
      for (const f of ctxListeners) f(context);
      if (first) { const f = first; first = null; f(conn); }
    } else if (m.type === "pelty:response" && typeof m.id === "number") {
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      if (m.ok) p.ok(m.body);
      else p.err(new BridgeError(Number(m.status) || 500, String((m.body as { error?: { code?: string } } | undefined)?.error?.code ?? "error.internal")));
    } else if (m.type === "pelty:event" && typeof m.name === "string") {
      for (const l of [...listeners]) {
        if (!matches(l.pattern, m.name)) continue;
        try { l.f(m.payload as never, m.name); } catch (err) { console.error("[pelty-ui] listener failed", err); }
      }
    }
  }

  w.addEventListener("message", onMessage);
  return new Promise((ok, err) => {
    first = ok;
    const t = setTimeout(() => { if (first) { first = null; err(new BridgeError(0, "error.timeout")); } }, opts.timeoutMs ?? 10_000);
    const done = ok;
    first = (c) => { clearTimeout(t); done(c); };
    post({ type: "pelty:ready" });
  });
}
