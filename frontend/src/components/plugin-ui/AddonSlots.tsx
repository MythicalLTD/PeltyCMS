/**
 * Addon injection points on every admin and client page (no per-page
 * wiring): the Shell renders `before`/`after`/`tabs`/`detail-panel` and the
 * layout slots; PageHeader renders `toolbar`, ResponsiveTable `row-actions`,
 * `<Replaceable section>` lets an addon swap out a named section.
 * Slot names: see lib/addon-frame.ts.
 */
import { lazy, Suspense, type ReactNode } from "react";
import { Outlet, useLocation, useSearchParams } from "react-router";
import { Card, CardContent } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useI18n } from "@/lib/i18n";
import { usePluginPages, usePluginRenderSlots, useUiOverrides } from "@/lib/addons/plugins";
import { hasId, layoutSlot, pageSlots, routeKey, type Area, type LayoutPosition, type PagePosition } from "@/lib/addons/addon-frame";
import type { RowData } from "@tanstack/react-table";
import type { Column } from "@/components/ResponsiveTable";
import { PluginSlot, SlotItem } from "./PluginSlot";

/** Area, route key and id context of the current page. */
export function useRouteInfo(pathname?: string) {
  const loc = useLocation();
  const path = pathname ?? loc.pathname;
  const area: Area = path.startsWith("/admin") ? "admin" : "client";
  const id = path.split("/").filter(Boolean).toReversed().find((s) => hasId(`/${s}`));
  return { area, key: routeKey(path, area), path, ctx: { route: path, ...(id ? { id } : {}) } as Record<string, unknown> };
}

const PluginView = lazy(() => import("./PluginRenderer").then((m) => ({ default: m.PluginView })));

/** The routed page, unless an addon with `ui.override` replaced this route
 *  (`ui.overrides.pages`). `?core=1` always shows the core page. */
export function OverridableOutlet() {
  const { area, key, ctx } = useRouteInfo();
  const [params] = useSearchParams();
  const { pages: overrides } = useUiOverrides();
  const { pages } = usePluginPages(area);
  const o = params.get("core") === "1" ? undefined : overrides.find((x) => x.route === `${area}.${key}`);
  const p = o && pages.find((x) => x.plugin === o.plugin && x.id === o.page);
  if (!p) return <Outlet />;
  return (
    <div data-overridden={`${area}.${key}`} data-override-plugin={p.plugin}>
      <Suspense fallback={null}><PluginView key={`${p.plugin}/${p.id}`} plugin={p.plugin} data={p.data} body={p.body} frame={p.frame} bridge={p.bridge} sameOrigin={p.same_origin} base={{ ctx, params: {} }} /></Suspense>
    </div>
  );
}

/** Contributions for one page position of the current route (route-specific, then `*`). */
export function RouteSlot({ pos, className }: { pos: PagePosition; className?: string }) {
  const { area, key, ctx } = useRouteInfo();
  return <PluginSlot slots={pageSlots(area, key, pos)} ctx={ctx} className={className} />;
}

export function LayoutSlot({ area, pos, className }: { area: Area; pos: LayoutPosition; className?: string }) {
  const { ctx } = useRouteInfo();
  return <PluginSlot slot={layoutSlot(area, pos)} ctx={ctx} className={className} />;
}

/** A page section addons may replace (`<area>.<route>.replace:<section>`,
 *  first contribution by `order` wins) or extend (`before:<section>`,
 *  `after:<section>`). `ctx` adds section data (ids) to the page context. */
export function Replaceable({ section, children, ctx: extra }: { section: string; children: ReactNode; ctx?: Record<string, unknown> }) {
  const { area, key, ctx: base } = useRouteInfo();
  const ctx = extra ? { ...base, ...extra } : base;
  const replace = usePluginRenderSlots(pageSlots(area, key, `replace:${section}`));
  const before = usePluginRenderSlots(pageSlots(area, key, `before:${section}`));
  const after = usePluginRenderSlots(pageSlots(area, key, `after:${section}`));
  if (!replace.length && !before.length && !after.length) return <>{children}</>;
  return (
    <>
      {before.length > 0 && <div className="mb-3 flex flex-col gap-3" data-plugin-slot={`before:${section}`}>{before.map((s, i) => <SlotItem key={i} s={s} ctx={ctx} />)}</div>}
      {replace.length ? <div data-replaced={section}><SlotItem s={replace[0]!} ctx={ctx} /></div> : children}
      {after.length > 0 && <div className="mt-3 flex flex-col gap-3" data-plugin-slot={`after:${section}`}>{after.map((s, i) => <SlotItem key={i} s={s} ctx={ctx} />)}</div>}
    </>
  );
}

/** Extra tabs after the page: each contribution is a tab (`body.label` or the addon id). */
export function AddonTabs() {
  const { tCode } = useI18n();
  const { area, key, ctx } = useRouteInfo();
  const items = usePluginRenderSlots(pageSlots(area, key, "tabs"));
  if (!items.length) return null;
  const label = (s: (typeof items)[number]) => (typeof s.body?.label === "string" ? tCode(s.body.label) : s.plugin);
  return (
    <Tabs defaultValue="0" className="mt-6" data-plugin-slot="tabs">
      <TabsList>{items.map((s, i) => <TabsTrigger key={i} value={String(i)}>{label(s)}</TabsTrigger>)}</TabsList>
      {items.map((s, i) => <TabsContent key={i} value={String(i)}><SlotItem s={s} ctx={ctx} /></TabsContent>)}
    </Tabs>
  );
}

/** Side information on detail pages (routes with an id segment). */
export function AddonDetailPanels() {
  const { area, key, ctx, path } = useRouteInfo();
  const items = usePluginRenderSlots(pageSlots(area, key, "detail-panel"));
  if (!items.length || !hasId(path)) return null;
  return (
    <div className="mt-6 grid gap-4 md:grid-cols-2" data-plugin-slot="detail-panel">
      {items.map((s, i) => <Card key={i}><CardContent><SlotItem s={s} ctx={ctx} /></CardContent></Card>)}
    </div>
  );
}

/** Extra actions column for tables (`<area>.<route>.row-actions`); `ctx.row_id` and `row` are bound. */
export function useRowActionsColumn<T extends RowData>(rowKey: (r: T) => string | number): Column<T> | null {
  const { area, key, ctx } = useRouteInfo();
  const items = usePluginRenderSlots(pageSlots(area, key, "row-actions"));
  if (!items.length) return null;
  return {
    header: "",
    className: "w-0 whitespace-nowrap text-right",
    cell: (row: T) => (
      <span className="inline-flex items-center gap-1" data-plugin-slot="row-actions">
        {items.map((s, i) => <SlotItem key={i} s={s} ctx={{ ...ctx, row_id: String(rowKey(row)) }} row={row} />)}
      </span>
    ),
  };
}
