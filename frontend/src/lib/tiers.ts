/**
 * Tier upgrades (module `tier_upgrades`): ranked packages where a player who
 * owns a lower tier pays the higher one minus an upgrade credit.
 */
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useModules } from "@/lib/modules";

export const CREDIT_MODES = ["paid", "difference"] as const;
export type CreditMode = (typeof CREDIT_MODES)[number];

export interface Tier { product_id: string; name: string; rank: number }
export interface TierGroup {
  id: string;
  name: string;
  credit_mode: CreditMode;
  min_price: string;
  revoke_old: boolean;
  cancel_old_recurring: boolean;
  tiers: Tier[];
}
export interface UpgradeCredit { group_id: string; amount: string; source: string; from_name: string }

export interface TierDraft {
  name: string;
  credit_mode: CreditMode;
  min_price: string;
  revoke_old: boolean;
  cancel_old_recurring: boolean;
  /** Lowest tier first. */
  product_ids: string[];
}

export const MAX_TIERS = 20;

export function tierDraft(g?: TierGroup): TierDraft {
  if (!g) return { name: "", credit_mode: "paid", min_price: "0", revoke_old: false, cancel_old_recurring: true, product_ids: [] };
  return {
    name: g.name,
    credit_mode: g.credit_mode,
    min_price: String(Number(g.min_price)),
    revoke_old: g.revoke_old,
    cancel_old_recurring: g.cancel_old_recurring,
    product_ids: g.tiers.toSorted((a, b) => a.rank - b.rank).map((x) => x.product_id),
  };
}

/** Client mirror of the server checks. */
export function tierValid(d: TierDraft): boolean {
  const name = d.name.trim();
  const ids = new Set(d.product_ids);
  return name.length > 0 && name.length <= 100
    && (CREDIT_MODES as readonly string[]).includes(d.credit_mode)
    && /^\d{1,6}(\.\d{1,2})?$/.test(d.min_price.trim() || "0") && Number(d.min_price || 0) <= 100_000
    && d.product_ids.length >= 2 && d.product_ids.length <= MAX_TIERS && ids.size === d.product_ids.length;
}

export function tierBody(d: TierDraft) {
  return { name: d.name.trim(), credit_mode: d.credit_mode, min_price: d.min_price.trim() || "0", revoke_old: d.revoke_old, cancel_old_recurring: d.cancel_old_recurring, product_ids: d.product_ids };
}

/** Move one tier up (-1) or down (+1); out-of-range moves are no-ops. */
export function moveTier(ids: readonly string[], index: number, by: -1 | 1): string[] {
  const to = index + by;
  if (index < 0 || index >= ids.length || to < 0 || to >= ids.length) return [...ids];
  const out = [...ids];
  const a = out[index] as string;
  out[index] = out[to] as string;
  out[to] = a;
  return out;
}

/** Price after the upgrade credit, never below zero (display only; the
 * server prices every order). */
export function upgradePrice(price: string | number, credit: UpgradeCredit | undefined): number {
  const p = Number(price);
  if (!credit) return p;
  return Math.max(0, Math.round((p - Number(credit.amount)) * 100) / 100);
}

/** Tier groups and the viewer's upgrade credits for `currency`. */
export function useTierUpgrades(currency: string, giftTo?: string) {
  const { enabled } = useModules();
  const on = enabled.has("tier_upgrades");
  const gift = giftTo?.trim() ?? "";
  const q = useQuery({
    queryKey: ["public", "tier-upgrades", currency, gift],
    queryFn: () => api<{ groups: { id: string; name: string; tiers: Tier[] }[]; credits: Record<string, UpgradeCredit> }>(
      `/public/tier-upgrades?currency=${encodeURIComponent(currency)}${gift ? `&gift_to=${encodeURIComponent(gift)}` : ""}`,
    ),
    enabled: on,
    staleTime: 30_000,
  });
  return { on, groups: q.data?.groups ?? [], credits: q.data?.credits ?? {} };
}
