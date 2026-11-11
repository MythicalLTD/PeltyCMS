/**
 * Bundle ownership discount (module `bundle_ownership`): items a buyer
 * already owns complete a bundle, so they pay only for the rest.
 */
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useModules } from "@/lib/modules";

export const OWNERSHIP_MODES = ["share", "list"] as const;
export type OwnershipMode = (typeof OWNERSHIP_MODES)[number];
export interface OwnershipSettings { mode: OwnershipMode; min_percent: string }
export const DEFAULT_OWNERSHIP: OwnershipSettings = { mode: "share", min_percent: "0" };

/** Tolerant read of the module settings (mirrors the server). */
export function parseOwnership(v: Record<string, unknown> | undefined): OwnershipSettings {
  const mode = OWNERSHIP_MODES.includes(v?.mode as OwnershipMode) ? (v!.mode as OwnershipMode) : "share";
  const n = Number(v?.min_percent ?? 0);
  return { mode, min_percent: String(Number.isFinite(n) ? Math.min(100, Math.max(0, n)) : 0) };
}

export function validMinPercent(s: string): boolean {
  return /^\d{1,3}(\.\d{1,2})?$/.test(s.trim()) && Number(s) <= 100;
}

export interface OwnedBundlePrice { full: number; price: number; saved: number; owned: number; size: number; allOwned: boolean }

/**
 * What the rest of a bundle costs when `owned[i]` items are already owned
 * (`prices[i]` list prices). Share: the rest keeps the bundle percent. List:
 * the bundle price minus the owned list prices. Never below `min_percent` of
 * the rest's list price. Same math as the server's `ownership_percent`.
 */
export function ownedBundlePrice(prices: number[], owned: boolean[], percent: number, s: OwnershipSettings): OwnedBundlePrice {
  const size = prices.length;
  const have = owned.filter(Boolean).length;
  const rest = prices.reduce((a, p, i) => a + (owned[i] ? 0 : p), 0);
  const had = prices.reduce((a, p, i) => a + (owned[i] ? p : 0), 0);
  const cap = Math.max(0, 100 - Number(s.min_percent));
  let pct = s.mode === "list" && had > 0 && rest > 0 ? (percent * (rest + had)) / rest : percent;
  pct = Math.max(0, Math.min(pct, cap));
  const saved = Math.round(rest * pct) / 100;
  return { full: rest, price: Math.round((rest - saved) * 100) / 100, saved, owned: have, size, allOwned: size > 0 && have === size };
}

/** Module settings and the signed-in viewer's owned bundle items. */
export function useBundleOwnership() {
  const { enabled } = useModules();
  const on = enabled.has("bundle_ownership");
  const q = useQuery({
    queryKey: ["public", "bundle-ownership"],
    queryFn: () => api<{ mode: OwnershipMode; min_percent: string; owned: string[] }>("/public/bundle-ownership"),
    enabled: on,
    staleTime: 30_000,
  });
  const settings = parseOwnership(q.data as Record<string, unknown> | undefined);
  return { on: on && !!q.data, settings, owned: new Set(q.data?.owned ?? []) };
}
