/**
 * Optional domain packs (gaming store, digital products, creator marketplace).
 * The server says which are on (`/public/modules`); routes and navigation of a
 * disabled module are hidden, and its API answers 404.
 */
import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";

export const MODULE_IDS = ["game_profiles", "game_delivery", "server_community", "gaming_storefront", "digital_products", "creator_marketplace"] as const;
export type ModuleId = (typeof MODULE_IDS)[number];

/** SPA route prefixes owned by a module (longest prefix wins). */
export const MODULE_ROUTES: readonly (readonly [string, ModuleId])[] = [];

export function moduleForPath(path: string): ModuleId | undefined {
  return MODULE_ROUTES.filter(([p]) => path === p || path.startsWith(`${p}/`)).toSorted((a, b) => b[0].length - a[0].length)[0]?.[1];
}

/** True when the path belongs to no module or its module is enabled. */
export function routeAllowed(path: string, enabled: ReadonlySet<string>): boolean {
  const m = moduleForPath(path);
  return !m || enabled.has(m);
}

/** `loaded` is true once the list arrived; `failed` when it could not be read. */
export function useModules(): { enabled: ReadonlySet<string>; loaded: boolean; failed: boolean } {
  const q = useQuery({
    queryKey: ["public", "modules"],
    queryFn: () => api<{ enabled: string[] }>("/public/modules"),
    staleTime: 60_000,
  });
  const enabled = useMemo<ReadonlySet<string>>(() => new Set(q.data?.enabled ?? []), [q.data]);
  return { enabled, loaded: q.isSuccess, failed: q.isError && !q.data };
}
