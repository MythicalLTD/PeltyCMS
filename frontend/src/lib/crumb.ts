import { useEffect, useSyncExternalStore } from "react";

/** Optional extra breadcrumb segment set by a detail page (e.g. an invoice number). */
let current: string | null = null;
const subs = new Set<() => void>();
const set = (v: string | null) => { current = v; subs.forEach((f) => f()); };

export function useCrumb(label: string | null | undefined) {
  useEffect(() => {
    set(label ?? null);
    return () => set(null);
  }, [label]);
}

export function useCurrentCrumb(): string | null {
  return useSyncExternalStore((f) => { subs.add(f); return () => subs.delete(f); }, () => current, () => null);
}
