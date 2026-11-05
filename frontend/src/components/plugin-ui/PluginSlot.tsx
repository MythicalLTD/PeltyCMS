import { lazy, Suspense } from "react";
import { usePluginRenderSlots, type RenderSlot } from "@/lib/addons/plugins";
import type { PluginSlotUi } from "@/lib/addons/plugin-ui";

// Loaded on demand: pages without plugin contributions never fetch the renderer.
const PluginView = lazy(() => import("@/components/plugin-ui/PluginRenderer").then((m) => ({ default: m.PluginView })));

/** One contribution: declarative tree or sandboxed addon frame. */
export function SlotItem({ s, ctx = {}, row }: { s: PluginSlotUi; ctx?: Record<string, unknown>; row?: unknown }) {
  return <Suspense fallback={null}><PluginView plugin={s.plugin} data={s.data} body={s.body} frame={s.frame} bridge={s.bridge} sameOrigin={s.same_origin} base={{ ctx, params: {}, row }} /></Suspense>;
}

/** Renders every enabled plugin's contribution to the given slot(s). */
export function PluginSlot({ slot, slots, ctx = {}, className = "flex flex-col gap-3" }: { slot?: RenderSlot | string; slots?: string[]; ctx?: Record<string, unknown>; className?: string }) {
  const items = usePluginRenderSlots(slots ?? (slot ? [slot] : []));
  if (!items.length) return null;
  return (
    <div className={className} data-plugin-slot={slot ?? slots?.[0]}>
      {items.map((s, i) => <SlotItem key={`${s.plugin}-${s.slot}-${i}`} s={s} ctx={ctx} />)}
    </div>
  );
}
