import { useEffect } from "react";
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { usePublicSite } from "@/lib/admin/site";
import { useModules } from "@/lib/modules";
import { accentStyle, type PublicStorefront } from "@/lib/storefront";

/** What the public site looks like: hosting landing, game-server store, or creator marketplace. */
export type PublicMode = "hosting" | "store" | "marketplace";
/** Admin setting (`SiteConfig.public_mode`). */
export type PublicModeSetting = "auto" | PublicMode;
export const PUBLIC_MODES: PublicModeSetting[] = ["auto", "hosting", "store", "marketplace"];

/**
 * The active look. `auto` (and a forced mode whose module is off) prefers the
 * game store, then the marketplace, else the hosting landing.
 */
export function resolveMode(setting: string | undefined, enabled: ReadonlySet<string>): PublicMode {
  const store = enabled.has("gaming_storefront");
  const market = enabled.has("creator_marketplace");
  if (setting === "hosting") return "hosting";
  if (setting === "store" && store) return "store";
  if (setting === "marketplace" && market) return "marketplace";
  return store ? "store" : market ? "marketplace" : "hosting";
}

export function usePublicMode() {
  const site = usePublicSite();
  const { enabled, loaded } = useModules();
  const mode = resolveMode(site.data?.config.public_mode, enabled);
  return {
    mode,
    loading: site.isLoading || !loaded,
    storeOn: enabled.has("gaming_storefront"),
    marketOn: enabled.has("creator_marketplace"),
    enabled,
  };
}

/** Public storefront layout (only fetched while `gaming_storefront` is on). */
export function useStorefront(on: boolean) {
  return useQuery({ queryKey: ["public", "storefront"], queryFn: () => api<PublicStorefront>("/public/storefront"), enabled: on, staleTime: 60_000 });
}

/** Accent presets of the public site. `panel` (default) = match the PeltyCMS panel / site brand; `default` is its old name. */
export const THEMES = ["panel", "minecraft", "fivem", "rust", "dark"] as const;
export type StoreTheme = (typeof THEMES)[number];

/** The preset as stored, with the legacy `default` (and anything unknown) read as `panel`. */
export function normalizeTheme(t: string | undefined): StoreTheme {
  return (THEMES as readonly string[]).includes(t ?? "") ? (t as StoreTheme) : "panel";
}

/** Accent tokens a preset may change. Surfaces, radius, font and components always stay the panel's. */
export const ACCENT_KEYS = ["--primary", "--primary-foreground", "--ring", "--chart-1", "--sidebar-primary", "--sidebar-primary-foreground", "--sidebar-ring"] as const;

const accent = (primary: string, fg: string): Record<string, string> => ({
  "--primary": primary, "--primary-foreground": fg, "--ring": primary, "--chart-1": primary,
  "--sidebar-primary": primary, "--sidebar-primary-foreground": fg, "--sidebar-ring": primary,
});

/** Game presets: accent colour only (inspired by the games, no official branding). */
export const THEME_VARS: Record<Exclude<StoreTheme, "panel">, Record<string, string>> = {
  minecraft: accent("#62c94a", "#0b1407"),
  fivem: accent("#f59e0b", "#1a1003"),
  rust: accent("#d9622b", "#1a0b03"),
  // Monochrome: follows the current light/dark surfaces.
  dark: accent("var(--foreground)", "var(--background)"),
};

/**
 * Accent variables of the active look (empty = the panel / site brand colours).
 * Only the store look has presets; the store accent wins over the preset.
 * Never returns anything outside {@link ACCENT_KEYS}.
 */
export function themeVars(mode: PublicMode, sf: Pick<PublicStorefront, "theme" | "accent"> | undefined, accentOf: (a: string) => Record<string, string> | undefined): Record<string, string> {
  if (mode !== "store" || !sf) return {};
  const t = normalizeTheme(sf.theme);
  const vars = { ...(t === "panel" ? {} : THEME_VARS[t]), ...accentOf(sf.accent) };
  return Object.fromEntries(Object.entries(vars).filter(([k]) => (ACCENT_KEYS as readonly string[]).includes(k)));
}

interface Root { style: { getPropertyValue(k: string): string; setProperty(k: string, v: string): void; removeProperty(k: string): void } }

/** Set `vars` on `root`; returns a function that restores what was there. Light/dark mode is left alone. */
export function applyVars(root: Root, vars: Record<string, string>): () => void {
  const keys = Object.keys(vars);
  if (keys.length === 0) return () => {};
  const prev = keys.map((k) => [k, root.style.getPropertyValue(k)] as const);
  for (const k of keys) root.style.setProperty(k, vars[k]!);
  return () => {
    for (const [k, v] of prev) if (v) root.style.setProperty(k, v); else root.style.removeProperty(k);
  };
}

/**
 * Apply the store's accent preset to the public site and client area. Never used on `/admin`.
 * Call it after any effect that sets the brand colours, passing that brand as `brand`.
 */
export function usePublicTheme(brand?: unknown) {
  const { mode, storeOn } = usePublicMode();
  const sf = useStorefront(storeOn && mode === "store");
  const vars = themeVars(mode, sf.data, accentVars);
  const key = JSON.stringify(vars);
  useEffect(() => {
    // Runs again whenever `brand` changes, after applyBrand has reset the colours.
    if (typeof document === "undefined" || brand === null) return;
    const v = JSON.parse(key) as Record<string, string>;
    const restore = applyVars(document.documentElement, v);
    if (Object.keys(v).length) document.documentElement.dataset.publicMode = mode;
    return () => { restore(); delete document.documentElement.dataset.publicMode; };
  }, [key, mode, brand]);
  return { mode, storefront: sf.data };
}

const accentVars = (a: string) => accentStyle(a) as Record<string, string> | undefined;

/** What players paste to join: FiveM/RedM use the F8 `connect` command. */
export function connectText(game: string, address: string): string {
  if (!address) return "";
  return game === "fivem" || game === "redm" ? `connect ${address}` : address;
}

/** Marketplace price label helper: cheapest price of a product in `currency`. */
export function cheapest(prices: { currency: string; amount: string }[] | undefined, currency: string): string | null {
  const list = (prices ?? []).filter((p) => p.currency === currency).map((p) => Number(p.amount)).filter((n) => Number.isFinite(n));
  return list.length ? String(Math.min(...list)) : null;
}
