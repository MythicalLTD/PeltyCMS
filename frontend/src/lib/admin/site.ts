import { useQuery } from "@tanstack/react-query";
import { api } from "../api";

export interface SiteLink { label: string; href: string }
export interface FooterColumn { title: string; links: SiteLink[] }
export interface SocialLink { network: string; href: string }
export const SOCIAL_NETWORKS = ["x", "github", "discord", "facebook", "instagram", "linkedin", "youtube", "mastodon", "email"] as const;
export interface SiteConfig {
  site_name: string; tagline: string; logo_url: string; primary_color: string; font: "manrope" | "inter" | "system";
  default_mode: "dark" | "light" | "system"; homepage: string; nav: SiteLink[]; footer_columns: FooterColumn[];
  footer_text: string; social: SocialLink[]; copyright: string; show_store_in_nav: boolean; client_dashboard_widgets: string[];
}
export type BlockType = "hero" | "features" | "pricing" | "faq" | "cta" | "text" | "image" | "stats" | "products" | "comparison";
export interface Block { id?: string; type: BlockType; props: Record<string, unknown> }
export interface SitePage { id: string; slug: string; title: string; description: string; blocks: Block[]; published: boolean; show_in_nav: boolean; sort_order: number }
export interface PublicSite { config: SiteConfig; nav_pages: { slug: string; title: string }[] }

export const BLOCK_TYPES: BlockType[] = ["hero", "features", "pricing", "products", "comparison", "faq", "cta", "text", "image", "stats"];
export const FONTS: Record<SiteConfig["font"], string> = {
  manrope: '"Manrope Variable", Manrope, system-ui, sans-serif',
  inter: "Inter, system-ui, sans-serif",
  system: "system-ui, -apple-system, Segoe UI, Roboto, sans-serif",
};

export function usePublicSite() {
  return useQuery({ queryKey: ["public", "site"], queryFn: async (): Promise<PublicSite> => { const r = await api<Partial<PublicSite> | null>("/public/site"); return { config: normalizeConfig(r?.config), nav_pages: arr(r?.nav_pages) }; }, staleTime: 60_000 });
}

/** Default props for a freshly added block (editable in the admin). */
export function newBlock(type: BlockType): Block {
  const id = Math.random().toString(36).slice(2, 10);
  const props: Record<BlockType, Record<string, unknown>> = {
    hero: { eyebrow: "", title: "Headline", subtitle: "", primary_label: "View plans", primary_href: "/store", secondary_label: "", secondary_href: "" },
    features: { title: "Features", columns: 3, items: [{ icon: "HardDrives", title: "Feature", text: "Describe this feature." }] },
    pricing: { title: "Plans", subtitle: "", group: "", highlight: -1 },
    faq: { title: "FAQ", items: [{ q: "Your question", a: "Your answer." }] },
    cta: { title: "Ready to order?", text: "", label: "Create account", href: "/register" },
    text: { title: "", body: "Text" },
    image: { src: "", alt: "", caption: "" },
    stats: { items: [{ value: "", label: "Metric label" }] },
    products: { title: "Choose your plan", subtitle: "", product_ids: [], cycle_toggle: true, default_cycle: "monthly", highlight: -1, badge: "Most popular", cta_label: "Order now" },
    comparison: { title: "Compare plans", plans: "Starter, Pro, Business", rows: [{ feature: "NVMe storage", values: "10 GB, 50 GB, 200 GB" }, { feature: "Daily backups", values: "—, ✓, ✓" }] },
  };
  return { id, type, props: props[type] };
}

export function moveBlock<T>(list: T[], from: number, to: number): T[] {
  if (to < 0 || to >= list.length || from === to) return list;
  const out = [...list];
  const [x] = out.splice(from, 1);
  out.splice(to, 0, x as T);
  return out;
}

/** Only safe links: same-site paths, http(s), mailto, anchors (mirrors the server). */
export function safeHref(h: unknown): string | null {
  if (typeof h !== "string" || !h) return null;
  // Reject whitespace and C0 controls so hrefs cannot smuggle newlines / nulls.
  // oxlint-disable-next-line eslint/no-control-regex -- intentional control-char reject
  if (/[\s\u0000-\u001f]/.test(h)) return null;
  if ((h.startsWith("/") && !h.startsWith("//")) || /^(https?:\/\/|mailto:|#)/.test(h)) return h;
  return null;
}

export const isInternal = (h: string) => h.startsWith("/") && !h.startsWith("//");

/** Brand settings → theme CSS variables (no custom CSS, only variables). */
export function applyBrand(
  c: Pick<SiteConfig, "primary_color" | "font"> & { default_mode?: SiteConfig["default_mode"] },
  root: { style: { setProperty(k: string, v: string): void; removeProperty(k: string): void }; classList?: { toggle(c: string, on: boolean): void } },
  prefersDark = false,
) {
  if (c.default_mode && root.classList) root.classList.toggle("dark", c.default_mode === "dark" || (c.default_mode === "system" && prefersDark));
  if (/^#[0-9a-f]{6}$/i.test(c.primary_color)) {
    root.style.setProperty("--primary", c.primary_color);
    root.style.setProperty("--ring", c.primary_color);
  } else {
    root.style.removeProperty("--primary");
    root.style.removeProperty("--ring");
  }
  root.style.setProperty("--font-sans", FONTS[c.font] ?? FONTS.manrope);
}

export const str = (v: unknown) => (typeof v === "string" ? v : "");
export const arr = <T,>(v: unknown): T[] => (Array.isArray(v) ? (v as T[]) : []);

export const DEFAULT_CONFIG: SiteConfig = {
  site_name: "PeltyCMS", tagline: "", logo_url: "", primary_color: "", font: "manrope", default_mode: "dark", homepage: "home",
  nav: [], footer_columns: [], footer_text: "", social: [], copyright: "", show_store_in_nav: true, client_dashboard_widgets: [],
};

/** Accepts `{config}` (server shape), a bare config, or null; always returns a complete config. */
export function normalizeConfig(raw: unknown): SiteConfig {
  const r = (raw && typeof raw === "object" ? raw : {}) as Record<string, unknown>;
  const c = (r.config && typeof r.config === "object" ? r.config : r) as Partial<SiteConfig>;
  const links = (v: unknown): SiteLink[] => arr<SiteLink>(v).filter((l) => l && typeof l === "object").map((l) => ({ label: str(l.label), href: str(l.href) }));
  return {
    ...DEFAULT_CONFIG, ...c,
    nav: links(c.nav),
    footer_columns: arr<FooterColumn>(c.footer_columns).map((f) => ({ title: str(f?.title), links: links(f?.links) })),
    client_dashboard_widgets: arr<string>(c.client_dashboard_widgets),
    social: arr<SocialLink>(c.social).filter((x) => x && typeof x === "object").map((x) => ({ network: str(x.network), href: str(x.href) })),
    copyright: str(c.copyright),
  };
}

/** Accepts `{items}` or a bare array; fills missing page fields. */
export function normalizePages(raw: unknown): SitePage[] {
  const list = Array.isArray(raw) ? raw : arr<SitePage>((raw as { items?: unknown } | null)?.items);
  return list.filter((p) => p && typeof p === "object").map((p: Partial<SitePage>) => ({
    id: str(p.id), slug: str(p.slug), title: str(p.title), description: str(p.description),
    blocks: arr<Block>(p.blocks).filter((b) => b && typeof b === "object").map((b) => ({ ...b, props: b.props && typeof b.props === "object" ? b.props : {} })), published: !!p.published, show_in_nav: !!p.show_in_nav, sort_order: Number(p.sort_order ?? 0),
  }));
}

/** Cycles offered by any of `products` in `currency`, in billing order. */
export const CYCLE_ORDER = ["monthly", "quarterly", "semiannual", "annual", "biennial", "triennial", "one_time"];
export function cyclesOf(products: { prices: { currency: string; cycle: string }[] }[], currency: string): string[] {
  const set = new Set(products.flatMap((p) => p.prices.filter((x) => x.currency === currency).map((x) => x.cycle)));
  return [...set].toSorted((a, b) => (CYCLE_ORDER.indexOf(a) + 99) % 99 - (CYCLE_ORDER.indexOf(b) + 99) % 99);
}

/** Comma list → trimmed cells ("✓, —" → ["✓", "—"]). */
export const cells = (s: unknown): string[] => str(s).split(",").map((x) => x.trim());

const preset = (type: BlockType, props: Record<string, unknown>): Block => { const n = newBlock(type); return { ...n, props: { ...n.props, ...props } }; };

/** Ready-made landing page for one or more products (admin "from template"). */
export function productLanding(name: string, productIds: string[]): { title: string; slug: string; description: string; blocks: Block[] } {
  const slug = name.toLowerCase().normalize("NFKD").replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 60) || "landing";
  return {
    title: name, slug, description: `${name}: plans and pricing`,
    blocks: [
      preset("hero", { eyebrow: "", title: name, subtitle: "Fast, reliable and supported 24/7.", primary_label: "See plans", primary_href: "#plans", secondary_label: "Contact sales", secondary_href: "/contact" }),
      preset("features", { title: "Why choose us", columns: 3, items: [{ icon: "Lightning", title: "Fast", text: "NVMe storage and modern CPUs." }, { icon: "ShieldCheck", title: "Secure", text: "DDoS protection and daily backups." }, { icon: "Headset", title: "Supported", text: "Real humans, around the clock." }] }),
      preset("products", { title: "Plans", product_ids: productIds }),
      preset("comparison", {}),
      preset("faq", { title: "FAQ", items: [{ q: "Can I upgrade later?", a: "Yes, upgrades are prorated automatically." }, { q: "Is there a setup fee?", a: "Only where shown on the plan." }] }),
      preset("cta", { title: "Ready to get started?", text: "Deploy in minutes.", label: "Order now", href: "/store" }),
    ],
  };
}
