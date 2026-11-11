/**
 * Optional domain packs (gaming store, digital products, creator marketplace).
 * The server says which are on (`/public/modules`); routes and navigation of a
 * disabled module are hidden, and its API answers 404.
 */
import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

export const MODULE_IDS = ["game_profiles", "game_delivery", "server_community", "gaming_storefront", "digital_products", "creator_marketplace", "product_reviews", "timed_sales", "creator_codes", "discord_roles", "tier_upgrades"] as const;
export type ModuleId = (typeof MODULE_IDS)[number];

/** SPA route prefixes owned by a module (longest prefix wins). */
export const MODULE_ROUTES: readonly (readonly [string, ModuleId])[] = [
  ["/admin/game-servers", "game_profiles"],
  ["/game-accounts", "game_profiles"],
  ["/admin/game-delivery", "game_delivery"],
  ["/admin/digital-products", "digital_products"],
  ["/digital", "digital_products"],
  ["/admin/creator-market", "creator_marketplace"],
  ["/creator", "creator_marketplace"],
  ["/wishlist", "creator_marketplace"],
  ["/messages", "creator_marketplace"],
  ["/admin/product-reviews", "product_reviews"],
  ["/admin/community", "server_community"],
  ["/admin/storefront", "gaming_storefront"],
  ["/admin/sales", "timed_sales"],
  ["/admin/creator-codes", "creator_codes"],
  ["/admin/discord-roles", "discord_roles"],
  ["/admin/tier-groups", "tier_upgrades"],
  ["/community", "server_community"],
];

export function moduleForPath(path: string): ModuleId | undefined {
  return MODULE_ROUTES.filter(([p]) => path === p || path.startsWith(`${p}/`)).toSorted((a, b) => b[0].length - a[0].length)[0]?.[1];
}

/** True when the path belongs to no module or its module is enabled. */
export function routeAllowed(path: string, enabled: ReadonlySet<string>): boolean {
  const m = moduleForPath(path);
  return !m || enabled.has(m);
}

/** Never restored from the browser cache: a module toggled in another tab
 * (or by another admin) would otherwise 404 its pages, or show a disabled
 * one, until the cached answer goes stale. */
export const MODULES_QUERY = { queryKey: ["public", "modules"], meta: { persist: false } } as const;

/** `loaded` is true once the list arrived; `failed` when it could not be read. */
export function useModules(): { enabled: ReadonlySet<string>; loaded: boolean; failed: boolean } {
  const q = useQuery({
    ...MODULES_QUERY,
    queryFn: () => api<{ enabled: string[] }>("/public/modules"),
    staleTime: 60_000,
  });
  const enabled = useMemo<ReadonlySet<string>>(() => new Set(q.data?.enabled ?? []), [q.data]);
  return { enabled, loaded: q.isSuccess, failed: q.isError && !q.data };
}
