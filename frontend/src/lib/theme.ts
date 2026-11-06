import { api } from "./api";

export interface Theme {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  color_scheme: "dark" | "light";
  tokens: Record<string, string>;
  logo_url: string | null;
  stylesheet_url: string | null;
  has_frontend?: boolean;
  templates?: Record<string, string>;
  custom_css?: string;
}

export interface ThemeCustomization {
  tokens: Record<string, string>;
  custom_css: string;
  color_scheme: "dark" | "light" | null;
}

/** Keys accepted by the server; anything else is ignored client-side too. */
const KEY = /^[a-z][a-z-]{1,30}$/;

interface StyleRoot {
  style: { setProperty(k: string, v: string): void; removeProperty?(k: string): void };
  classList?: { toggle(c: string, on: boolean): void };
}
const appliedKeys = new WeakMap<object, Set<string>>();

/**
 * PeltyCMS theme token -> shadcn/ui CSS variables it drives. Tokens without
 * an entry map to the variable of the same name (e.g. `primary` -> `--primary`).
 */
export const TOKEN_VARS: Record<string, string[]> = {
  surface: ["--card", "--popover", "--sidebar"],
  "surface-foreground": ["--card-foreground", "--popover-foreground", "--sidebar-foreground"],
  muted: ["--muted", "--secondary"],
  foreground: ["--foreground", "--secondary-foreground"],
  primary: ["--primary", "--sidebar-primary", "--chart-1"],
  "primary-foreground": ["--primary-foreground", "--sidebar-primary-foreground"],
  accent: ["--accent", "--sidebar-accent"],
  "accent-foreground": ["--accent-foreground", "--sidebar-accent-foreground"],
  border: ["--border", "--sidebar-border"],
  ring: ["--ring", "--sidebar-ring"],
  danger: ["--destructive", "--chart-4"],
  warning: ["--warning", "--chart-3"],
  success: ["--success", "--chart-2"],
  sidebar: ["--sidebar"],
  "sidebar-foreground": ["--sidebar-foreground"],
};

/**
 * Apply theme tokens as shadcn CSS variables. Variables set by a previously
 * applied theme that the new one does not define are removed, so switching
 * themes restyles fully without a reload. Returns the number of tokens used.
 */
export function applyTheme(theme: Theme, root: StyleRoot): number {
  const prev = appliedKeys.get(root) ?? new Set<string>();
  const next = new Set<string>();
  let applied = 0;
  for (const [k, v] of Object.entries(theme.tokens)) {
    if (!KEY.test(k) || /[;{}<>\\]/.test(v)) continue;
    applied++;
    for (const name of TOKEN_VARS[k] ?? [`--${k}`]) {
      root.style.setProperty(name, v);
      next.add(name);
    }
  }
  for (const k of prev) if (!next.has(k)) root.style.removeProperty?.(k);
  appliedKeys.set(root, next);
  root.style.setProperty("color-scheme", theme.color_scheme);
  root.classList?.toggle("dark", theme.color_scheme === "dark");
  return applied;
}

export async function loadActiveTheme(): Promise<Theme | null> {
  try {
    const theme = await api<Theme | null>("/themes/active");
    if (!theme) return null;
    applyTheme(theme, document.documentElement);
    document.getElementById("pelty-theme-css")?.remove();
    if (theme.stylesheet_url) {
      const link = document.createElement("link");
      link.id = "pelty-theme-css";
      link.rel = "stylesheet";
      link.href = theme.stylesheet_url;
      document.head.appendChild(link);
    }
    applyCustomCss(theme.custom_css ?? "");
    applyTemplateStyles(theme.templates ?? {});
    return theme;
  } catch {
    return null;
  }
}

/** Inject admin-authored custom CSS (already sanitized server-side). */
export function applyCustomCss(css: string) {
  let el = document.getElementById("pelty-theme-custom") as HTMLStyleElement | null;
  if (!css) {
    el?.remove();
    return;
  }
  if (!el) {
    el = document.createElement("style");
    el.id = "pelty-theme-custom";
    document.head.appendChild(el);
  }
  el.textContent = css;
}

/** Load theme template CSS overrides (HTML templates are fetched by ThemeSlot). */
export function applyTemplateStyles(templates: Record<string, string>) {
  document.querySelectorAll("link[data-pelty-theme-slot]").forEach((n) => n.remove());
  for (const [slot, href] of Object.entries(templates)) {
    if (!href.endsWith(".css")) continue;
    const link = document.createElement("link");
    link.rel = "stylesheet";
    link.href = href;
    link.dataset.peltyThemeSlot = slot;
    document.head.appendChild(link);
  }
}
