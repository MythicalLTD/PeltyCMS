/**
 * Feeds the addon event bus (lib/addon-events.ts) from the app shell:
 * navigation, records shown on detail pages, language and theme changes,
 * and successful mutations. Rendered once per layout by <Shell>.
 */
import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useI18n } from "@/lib/i18n";
import { emitHost } from "@/lib/addons/addon-events";
import { useRouteInfo } from "./AddonSlots";

export function HostEvents() {
  const { area, key, path, ctx } = useRouteInfo();
  const { lang } = useI18n();
  const qc = useQueryClient();
  const id = typeof ctx.id === "string" ? ctx.id : null;

  useEffect(() => {
    emitHost("route.changed", { path, area, key });
    if (id) emitHost("record.viewed", { kind: key, id, area });
  }, [path, area, key, id]);

  useEffect(() => { emitHost("ui.locale_changed", { locale: lang }); }, [lang]);

  useEffect(() => {
    const html = document.documentElement;
    let dark = html.classList.contains("dark");
    const o = new MutationObserver(() => {
      const d = html.classList.contains("dark");
      if (d !== dark) emitHost("ui.theme_changed", { dark: (dark = d) });
    });
    o.observe(html, { attributes: true, attributeFilter: ["class"] });
    return () => o.disconnect();
  }, []);

  useEffect(() => qc.getMutationCache().subscribe((e) => {
    if (e.type !== "updated" || e.action.type !== "success") return;
    const k = e.mutation.options.mutationKey?.[0];
    emitHost("data.changed", { key: typeof k === "string" ? k : "" });
  }), [qc]);

  return null;
}
