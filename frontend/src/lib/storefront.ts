import type { CSSProperties } from "react";
import type { Goal, ServerStatus } from "@/lib/community";

/** Gaming storefront (optional `gaming_storefront` module). */
export const ACCENTS = ["default", "emerald", "violet", "amber", "rose", "sky"] as const;
export type Accent = (typeof ACCENTS)[number];
export const PERIODS = ["day", "week", "month", "year", "all"] as const;
export const WIDGET_TYPES = ["text", "top_customer", "recent_payments", "featured_package", "server_status", "goal", "discord"] as const;
export type WidgetType = (typeof WIDGET_TYPES)[number];

/** Accent swatches (readable with the dark primary foreground). */
const ACCENT_COLORS: Record<Exclude<Accent, "default">, string> = {
  emerald: "#34d399", violet: "#a78bfa", amber: "#fbbf24", rose: "#fb7185", sky: "#38bdf8",
};

/** CSS variables that recolor primary buttons and rings inside the store. */
export function accentStyle(accent: string): CSSProperties | undefined {
  const c = ACCENT_COLORS[accent as keyof typeof ACCENT_COLORS];
  return c ? ({ "--primary": c, "--ring": c, "--primary-foreground": "#08121c" } as CSSProperties) : undefined;
}

export type WidgetConfig =
  | { type: "text"; title: string; body: string }
  | { type: "top_customer"; period: string; show_amount: boolean }
  | { type: "recent_payments"; limit: number }
  | { type: "featured_package"; product_id: string }
  | { type: "server_status"; server_id?: string | null }
  | { type: "goal" }
  | { type: "discord"; invite_url: string; label: string };

export interface StorefrontConfig { hero_title: string; hero_subtitle: string; hero_image: string; accent: string; layout: "list" | "grid"; sidebar: "left" | "right"; widgets: WidgetConfig[] }

export type PublicWidget =
  | { type: "text"; title: string; body: string }
  | { type: "top_customer"; period: string; currency: string; customer: { name: string; total?: string } | null }
  | { type: "recent_payments"; items: { name: string; products: string[]; paid_at: string }[] }
  | { type: "featured_package"; product: { id: string; name: string; description: string; image_url: string } }
  | { type: "server_status"; servers: ServerStatus[] }
  | { type: "goal"; goal: Goal }
  | { type: "discord"; invite_url: string; label: string };

export interface PublicStorefront extends Omit<StorefrontConfig, "widgets"> { widgets: PublicWidget[] }

/** A fresh widget of a type, with the server's valid defaults. */
export function newWidget(type: WidgetType, productId = ""): WidgetConfig {
  switch (type) {
    case "text": return { type, title: "", body: "" };
    case "top_customer": return { type, period: "month", show_amount: false };
    case "recent_payments": return { type, limit: 5 };
    case "featured_package": return { type, product_id: productId };
    case "server_status": return { type, server_id: null };
    case "goal": return { type };
    case "discord": return { type, invite_url: "https://discord.gg/", label: "" };
  }
}

/** Client-side mirror of the server checks, to disable Save early. */
export function widgetValid(w: WidgetConfig): boolean {
  switch (w.type) {
    case "text": return w.body.trim().length > 0 && w.body.length <= 2000 && w.title.length <= 80;
    case "recent_payments": return Number.isInteger(w.limit) && w.limit >= 1 && w.limit <= 10;
    case "featured_package": return /^[0-9a-f-]{36}$/i.test(w.product_id);
    case "discord": return w.invite_url.length <= 200 && /^https:\/\/(discord\.gg|discord\.com\/invite)\/[A-Za-z0-9_-]+$/.test(w.invite_url) && w.label.length <= 60;
    case "top_customer": return (PERIODS as readonly string[]).includes(w.period);
    default: return true;
  }
}

/** `https://` or a site-relative path; empty is allowed. */
export function heroImageValid(u: string): boolean {
  return u === "" || (u.length <= 500 && (u.startsWith("https://") || (u.startsWith("/") && !u.startsWith("//"))) && !/[\s"<]/.test(u));
}

/** Move a list item by `delta` places (clamped). */
export function move<T>(list: T[], i: number, delta: number): T[] {
  const j = Math.max(0, Math.min(list.length - 1, i + delta));
  if (i === j) return list;
  const next = [...list];
  next.splice(j, 0, ...next.splice(i, 1));
  return next;
}
