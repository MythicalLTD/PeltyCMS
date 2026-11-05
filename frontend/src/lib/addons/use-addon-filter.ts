import { useEffect, useState } from "react";
import { applyFilters, applyFiltersAsync, hasAsyncFilters, onFilterRegistry, type HostFilters } from "@/lib/addons/addon-events";

/**
 * `applyFilters` plus the async (addon frame) filters: returns the sync
 * result at once and switches to the async result when it arrives. Re-runs
 * when `key` changes or a frame registers / unregisters a filter.
 */
export function useAddonFilter<K extends keyof HostFilters>(name: K, value: HostFilters[K], ctx: Record<string, unknown>, key: string): HostFilters[K] {
  const [ver, setVer] = useState(0);
  const [done, setDone] = useState<{ k: string; v: HostFilters[K] } | null>(null);
  useEffect(() => onFilterRegistry(() => setVer((x) => x + 1)), []);
  const k = `${key}\u0000${ver}`;
  const sync = applyFilters(name, value, ctx);
  useEffect(() => {
    if (!hasAsyncFilters(name)) return;
    let live = true;
    void applyFiltersAsync(name, value, ctx).then((v) => { if (live) setDone({ k, v }); });
    return () => { live = false; };
    // `key` stands for value and ctx.
  }, [name, k]);
  return done && done.k === k && hasAsyncFilters(name) ? done.v : sync;
}
