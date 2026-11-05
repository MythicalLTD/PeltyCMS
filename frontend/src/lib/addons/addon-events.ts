/**
 * Frontend addon event bus. The host emits UI events (route changes, the
 * record a page shows, data changes); addon frames subscribe over
 * postMessage (`pelty:subscribe`) and receive `pelty:event`. Frames emit
 * their own events with `pelty:emit`; the host namespaces them as
 * `<addon-id>:<name>`, so one addon can never fake a host event or another
 * addon's event.
 *
 * Filters (`addFilter` / `applyFilters`) are synchronous value hooks for
 * host-side code (nav items, page titles). Every handler runs isolated: a
 * throwing listener or filter is skipped and logged.
 *
 * Async filters (`addAsyncFilter` / `applyFiltersAsync`) are how sandboxed
 * addon frames take part: the frame registers with `pelty:filter_register`,
 * the host sends `pelty:filter` and waits for `pelty:filter_result`. Each
 * async filter gets a copy of the value and a deadline; a timeout, an error
 * or an invalid result keeps the previous value (fail open).
 */

/** Events the host emits (payloads are plain JSON). */
export interface HostEvents {
  /** Every client-side navigation. */
  "route.changed": { path: string; area: "admin" | "client"; key: string };
  /** A detail page showed a record (`kind` = invoice, service, order, ticket, …). */
  "record.viewed": { kind: string; id: string; area: "admin" | "client" };
  /** The theme or language changed. */
  "ui.theme_changed": { dark: boolean };
  "ui.locale_changed": { locale: string };
  /** A host mutation succeeded (`key` = the query key root that was invalidated). */
  "data.changed": { key: string };
}

export const HOST_EVENTS: Array<keyof HostEvents> = ["route.changed", "record.viewed", "ui.theme_changed", "ui.locale_changed", "data.changed"];

/** Value filters the host applies (see `applyFilters`). */
export interface HostFilters {
  /** Sidebar entries contributed by addons, before rendering. */
  "nav.items": Array<{ to: string; label: string; plugin?: string }>;
  /** The page title shown in the header breadcrumb. */
  "page.title": string;
}

export const HOST_FILTERS: Array<keyof HostFilters> = ["nav.items", "page.title"];

type Listener = (name: string, payload: unknown) => void;
type FilterFn<T> = (value: T, ctx: Record<string, unknown>) => T;

const listeners = new Map<string, Set<Listener>>();
const filters = new Map<string, Array<{ fn: FilterFn<unknown>; priority: number; id: number }>>();
let seq = 0;

/** Does subscription `pattern` match `name`? `*` = all, `record.*` = prefix, `my-addon:*` = one addon's events. */
export function matches(pattern: string, name: string): boolean {
  if (pattern === "*" || pattern === name) return true;
  if (pattern.endsWith("*")) return name.startsWith(pattern.slice(0, -1));
  return false;
}

/** Subscribe to events matching `pattern`. Returns the unsubscribe function. */
export function on(pattern: string, fn: Listener): () => void {
  let set = listeners.get(pattern);
  if (!set) listeners.set(pattern, (set = new Set()));
  const s = set;
  s.add(fn);
  return () => {
    s.delete(fn);
    if (!s.size) listeners.delete(pattern);
  };
}

/** Typed subscription to one host event. */
export function onHost<K extends keyof HostEvents>(name: K, fn: (payload: HostEvents[K]) => void): () => void {
  return on(name, (_n, p) => fn(p as HostEvents[K]));
}

/** Deliver `name` to every matching listener (errors are isolated). */
export function emit(name: string, payload: unknown = null): void {
  // Snapshot so listeners can subscribe/unsubscribe during delivery.
  for (const [pattern, set] of Array.from(listeners)) {
    if (!matches(pattern, name)) continue;
    for (const fn of Array.from(set)) {
      try {
        fn(name, payload);
      } catch (e) {
        console.warn(`addon event listener for ${name} failed`, e);
      }
    }
  }
}

export function emitHost<K extends keyof HostEvents>(name: K, payload: HostEvents[K]): void {
  emit(name, payload);
}

/** An event a frame emitted: always namespaced by its addon. */
export function emitFromAddon(plugin: string, name: string, payload: unknown): string {
  const full = `${plugin}:${name}`;
  emit(full, payload);
  return full;
}

/** Register a value filter; lower priority runs first. Returns the remover. */
export function addFilter<K extends keyof HostFilters>(name: K, fn: FilterFn<HostFilters[K]>, priority = 0): () => void {
  const list = filters.get(name) ?? [];
  const entry = { fn: fn as FilterFn<unknown>, priority, id: seq++ };
  filters.set(name, [...list, entry].toSorted((a, b) => a.priority - b.priority || a.id - b.id));
  return () => filters.set(name, (filters.get(name) ?? []).filter((e) => e !== entry));
}

/** Run the filters of `name`. A filter that throws or returns a value of
 *  another type (or `undefined`) is skipped. */
export function applyFilters<K extends keyof HostFilters>(name: K, value: HostFilters[K], ctx: Record<string, unknown> = {}): HostFilters[K] {
  let v: unknown = value;
  for (const { fn } of filters.get(name) ?? []) {
    try {
      const out = fn(v, ctx);
      if (out !== undefined && Array.isArray(out) === Array.isArray(v) && typeof out === typeof v) v = out;
    } catch (e) {
      console.warn(`addon filter ${name} failed`, e);
    }
  }
  return v as HostFilters[K];
}

// ----- async filters (addon frames) -----

type AsyncFilterFn = (value: unknown, ctx: Record<string, unknown>) => Promise<unknown>;
const asyncFilters = new Map<string, Array<{ fn: AsyncFilterFn; priority: number; id: number; owner: string }>>();
const registryWatchers = new Set<() => void>();

/** Per-filter deadline for one async filter (ms). */
export const ASYNC_FILTER_TIMEOUT_MS = 300;

/** Is `v` an acceptable value for filter `name`? Async results come from
 *  untrusted frames, so they are checked by shape and size. */
export function validFilterValue(name: string, v: unknown): boolean {
  if (name === "page.title") return typeof v === "string" && v.length <= 200;
  if (name === "nav.items") {
    return Array.isArray(v) && v.length <= 50 && v.every((e) => !!e && typeof e === "object" && typeof (e as { to?: unknown }).to === "string" && typeof (e as { label?: unknown }).label === "string" && (e as { label: string }).label.length <= 80);
  }
  return false;
}

/** Register an async filter (`owner` = the addon id). Returns the remover. */
export function addAsyncFilter(name: keyof HostFilters, fn: AsyncFilterFn, priority = 0, owner = ""): () => void {
  const list = asyncFilters.get(name) ?? [];
  const entry = { fn, priority, id: seq++, owner };
  asyncFilters.set(name, [...list, entry].toSorted((a, b) => a.priority - b.priority || a.id - b.id));
  notifyRegistry();
  return () => {
    asyncFilters.set(name, (asyncFilters.get(name) ?? []).filter((e) => e !== entry));
    notifyRegistry();
  };
}

export const hasAsyncFilters = (name: string) => (asyncFilters.get(name)?.length ?? 0) > 0;

/** Called whenever an async filter is added or removed. Returns the remover. */
export function onFilterRegistry(fn: () => void): () => void {
  registryWatchers.add(fn);
  return () => void registryWatchers.delete(fn);
}

function notifyRegistry() {
  for (const fn of Array.from(registryWatchers)) {
    try { fn(); } catch (e) { console.warn("filter registry watcher failed", e); }
  }
}

function withTimeout<T>(p: Promise<T>, ms: number): Promise<T> {
  return new Promise((res, rej) => {
    const t = setTimeout(() => rej(new Error("timeout")), ms);
    p.then((v) => { clearTimeout(t); res(v); }, (e: unknown) => { clearTimeout(t); rej(e); });
  });
}

/** The sync filters, then every async filter in priority order. Each async
 *  filter sees a JSON copy, has `timeoutMs`, and is skipped on timeout,
 *  error or an invalid result. Never rejects. */
export async function applyFiltersAsync<K extends keyof HostFilters>(name: K, value: HostFilters[K], ctx: Record<string, unknown> = {}, timeoutMs = ASYNC_FILTER_TIMEOUT_MS): Promise<HostFilters[K]> {
  let v: unknown = applyFilters(name, value, ctx);
  for (const { fn, owner } of Array.from(asyncFilters.get(name) ?? [])) {
    try {
      const copy: unknown = JSON.parse(JSON.stringify(v));
      const out = await withTimeout(Promise.resolve().then(() => fn(copy, { ...ctx })), timeoutMs);
      if (validFilterValue(name, out)) v = out;
    } catch (e) {
      console.warn(`async addon filter ${name}${owner ? ` (${owner})` : ""} skipped`, e);
    }
  }
  return v as HostFilters[K];
}

/** Test helper: drop every listener and filter. */
export function resetBus(): void {
  listeners.clear();
  filters.clear();
  asyncFilters.clear();
  registryWatchers.clear();
}
