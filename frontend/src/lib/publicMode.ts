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

export const THEMES = ["default", "minecraft", "fivem", "rust", "dark"] as const;
export type StoreTheme = (typeof THEMES)[number];

/** Colour presets: shadcn variables, always dark. Inspired by the games, no official branding. */
export const THEME_VARS: Record<Exclude<StoreTheme, "default"> | "marketplace", Record<string, string>> = {
  minecraft: {
    "--background": "#12160f", "--foreground": "#f1f5ea", "--card": "#1b2216", "--card-foreground": "#f1f5ea", "--popover": "#1b2216",
    "--popover-foreground": "#f1f5ea", "--primary": "#62c94a", "--primary-foreground": "#0b1407", "--secondary": "#263020",
    "--secondary-foreground": "#f1f5ea", "--muted": "#232c1d", "--muted-foreground": "#a9b59d", "--accent": "#2b3824",
    "--accent-foreground": "#f1f5ea", "--border": "#34412b", "--input": "#3f4d34", "--ring": "#62c94a", "--radius": "0.25rem",
  },
  fivem: {
    "--background": "#0d0d12", "--foreground": "#f5f3f0", "--card": "#16161d", "--card-foreground": "#f5f3f0", "--popover": "#16161d",
    "--popover-foreground": "#f5f3f0", "--primary": "#f59e0b", "--primary-foreground": "#1a1003", "--secondary": "#202029",
    "--secondary-foreground": "#f5f3f0", "--muted": "#1d1d25", "--muted-foreground": "#a8a6ae", "--accent": "#262630",
    "--accent-foreground": "#f5f3f0", "--border": "#2c2c37", "--input": "#383845", "--ring": "#f59e0b", "--radius": "0.6rem",
  },
  rust: {
    "--background": "#15110e", "--foreground": "#f3ece6", "--card": "#1f1914", "--card-foreground": "#f3ece6", "--popover": "#1f1914",
    "--popover-foreground": "#f3ece6", "--primary": "#d9622b", "--primary-foreground": "#1a0b03", "--secondary": "#2a221b",
    "--secondary-foreground": "#f3ece6", "--muted": "#261f19", "--muted-foreground": "#b2a397", "--accent": "#30271f",
    "--accent-foreground": "#f3ece6", "--border": "#3a2f26", "--input": "#473a2f", "--ring": "#d9622b", "--radius": "0.2rem",
  },
  dark: {
    "--background": "#0b0b0c", "--foreground": "#ededef", "--card": "#141416", "--card-foreground": "#ededef", "--popover": "#141416",
    "--popover-foreground": "#ededef", "--primary": "#e4e4e7", "--primary-foreground": "#111113", "--secondary": "#1c1c1f",
    "--secondary-foreground": "#ededef", "--muted": "#1a1a1d", "--muted-foreground": "#a1a1aa", "--accent": "#222226",
    "--accent-foreground": "#ededef", "--border": "#27272a", "--input": "#34343a", "--ring": "#a1a1aa", "--radius": "0.5rem",
  },
  marketplace: {
    "--background": "#0b0f1a", "--foreground": "#eef1f8", "--card": "#121829", "--card-foreground": "#eef1f8", "--popover": "#121829",
    "--popover-foreground": "#eef1f8", "--primary": "#6d7cff", "--primary-foreground": "#070a1c", "--secondary": "#1a2238",
    "--secondary-foreground": "#eef1f8", "--muted": "#171e32", "--muted-foreground": "#9aa3bd", "--accent": "#1e2740",
    "--accent-foreground": "#eef1f8", "--border": "#232c46", "--input": "#2e3858", "--ring": "#6d7cff", "--radius": "0.6rem",
  },
};

/** Variables of the active look (empty = keep the site brand). The store accent wins over the preset's primary. */
export function themeVars(mode: PublicMode, sf: Pick<PublicStorefront, "theme" | "accent"> | undefined, accent: (a: string) => Record<string, string> | undefined): Record<string, string> {
  if (mode === "marketplace") return THEME_VARS.marketplace;
  if (mode !== "store" || !sf) return {};
  const base = sf.theme && sf.theme !== "default" ? THEME_VARS[sf.theme as Exclude<StoreTheme, "default">] ?? {} : {};
  return { ...base, ...accent(sf.accent) };
}

interface Root { style: { getPropertyValue(k: string): string; setProperty(k: string, v: string): void; removeProperty(k: string): void }; classList: { contains(c: string): boolean; toggle(c: string, on: boolean): void } }

/** Set `vars` (and dark mode) on `root`; returns a function that restores what was there. */
export function applyVars(root: Root, vars: Record<string, string>): () => void {
  const keys = Object.keys(vars);
  if (keys.length === 0) return () => {};
  const prev = keys.map((k) => [k, root.style.getPropertyValue(k)] as const);
  const wasDark = root.classList.contains("dark");
  for (const k of keys) root.style.setProperty(k, vars[k]!);
  root.classList.toggle("dark", true);
  return () => {
    for (const [k, v] of prev) if (v) root.style.setProperty(k, v); else root.style.removeProperty(k);
    root.classList.toggle("dark", wasDark);
  };
}

/**
 * Recolour the public site and client area to the active store/marketplace look. Never used on `/admin`.
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
