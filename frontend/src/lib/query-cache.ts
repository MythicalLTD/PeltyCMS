/**
 * Browser cache of query data (stale-while-revalidate across visits).
 *
 * Successful queries are persisted to IndexedDB under a per-user key, so a
 * revisited page renders the last-seen data immediately while TanStack
 * Query refetches it in the background (queries are stale at once).
 * - Keyed by user id; the cache of another user is never restored.
 * - Cleared on logout (and whenever `/auth/me` says nobody is signed in).
 * - Never persisted: `me` (holds the CSRF token), and any query whose key
 *   looks sensitive (secrets, tokens, passwords, 2FA, API keys, settings
 *   of gateways/SSO/SMTP, addon settings) or sets `meta: { persist: false }`.
 */
import type { Query, QueryClient } from "@tanstack/react-query";
import { persistQueryClientRestore, persistQueryClientSubscribe, type Persister } from "@tanstack/react-query-persist-client";
import { createAsyncStoragePersister } from "@tanstack/query-async-storage-persister";
import { del, get, set } from "idb-keyval";

const LAST_USER = "pelty-cache-user";
const PREFIX = "pelty-cache:";
const MAX_AGE = 24 * 60 * 60 * 1000;
/** Bump to drop every persisted cache after an incompatible change. */
export const CACHE_BUSTER = "1";

const SENSITIVE = /secret|token|passw|mfa|2fa|totp|backup|recovery|api[-_]?key|credential|sso|saml|oauth|smtp|gateway|settings|session|csrf|webhook|audit|private|key/i;

/** Should this query be written to the browser cache? */
export function persistable(q: Pick<Query, "queryKey" | "state" | "meta">): boolean {
  if (q.state.status !== "success" || q.meta?.persist === false) return false;
  const k = q.queryKey;
  if (k[0] === "me") return false;
  return !k.some((p) => typeof p === "string" && SENSITIVE.test(p));
}

const idb = { getItem: (k: string) => get<string>(k).then((v) => v ?? null), setItem: (k: string, v: string) => set(k, v), removeItem: (k: string) => del(k) };

function persister(userId: string): Persister {
  return createAsyncStoragePersister({ storage: idb, key: PREFIX + userId, throttleTime: 1000 });
}

let active: { user: string; stop: () => void } | null = null;

function lastUser(): string | null {
  try {
    return localStorage.getItem(LAST_USER);
  } catch {
    return null;
  }
}

/** Restore the last signed-in user's cache before the first render. */
export async function restoreCache(qc: QueryClient): Promise<void> {
  const user = lastUser();
  if (!user || typeof indexedDB === "undefined") return;
  try {
    await persistQueryClientRestore({ queryClient: qc, persister: persister(user), maxAge: MAX_AGE, buster: CACHE_BUSTER });
  } catch {
    /* a broken cache is simply ignored */
  }
}

/** Start (or switch) persisting for `userId`; `null` = signed out: wipe. */
export async function bindCache(qc: QueryClient, userId: string | null): Promise<void> {
  if (typeof indexedDB === "undefined") return;
  if (active && active.user === userId) return;
  const prev = active?.user ?? lastUser();
  active?.stop();
  active = null;
  if (prev && prev !== userId) {
    await del(PREFIX + prev).catch(() => undefined);
    // Another user's restored data must not stay on screen.
    if (userId) qc.removeQueries({ predicate: (q) => q.queryKey[0] !== "me" });
  }
  if (!userId) {
    try {
      localStorage.removeItem(LAST_USER);
    } catch {
      /* storage disabled */
    }
    return;
  }
  try {
    localStorage.setItem(LAST_USER, userId);
  } catch {
    /* storage disabled */
  }
  const stop = persistQueryClientSubscribe({ queryClient: qc, persister: persister(userId), buster: CACHE_BUSTER, dehydrateOptions: { shouldDehydrateQuery: persistable } });
  active = { user: userId, stop };
}

/** Logout: stop persisting and delete the user's cache. */
export async function clearCache(): Promise<void> {
  const u = active?.user ?? lastUser();
  active?.stop();
  active = null;
  try {
    localStorage.removeItem(LAST_USER);
  } catch {
    /* storage disabled */
  }
  if (u && typeof indexedDB !== "undefined") await del(PREFIX + u).catch(() => undefined);
}
