import { useQuery } from "@tanstack/react-query";
import { api } from "../api";
import type { PluginPage, PluginSlotUi } from "./plugin-ui";

export type Slot = "client.nav" | "admin.nav" | "client.dashboard" | "admin.dashboard";

export interface PluginUiEntry {
  plugin: string;
  kind: "menu" | "widget";
  slot: Slot;
  /** Translation key in the plugin namespace (plugin.<id>.*). */
  label: string;
  path: string;
}

/** Frontend overrides (addons with the `ui.override` capability). */
export interface UiOverrides {
  /** Core routes (`<area>.<route-key>`) rendered by an addon page instead. */
  pages: Array<{ plugin: string; route: string; page: string }>;
  /** Core sidebar paths hidden by addons. */
  hide_nav: Array<{ plugin: string; path: string }>;
  conflicts?: Array<{ route: string; winner: string; plugin: string }>;
}

interface PluginUiResponse { items: PluginUiEntry[]; pages?: PluginPage[]; slots?: PluginSlotUi[]; overrides?: UiOverrides }

function usePluginUiData() {
  return useQuery({ queryKey: ["plugins", "ui"], queryFn: () => api<PluginUiResponse>("/plugins/ui"), staleTime: 60_000 });
}

/** UI entries contributed by enabled plugins, already filtered by permission server-side. */
export function usePluginSlot(slot: Slot): PluginUiEntry[] {
  const { data } = usePluginUiData();
  return (data?.items ?? []).filter((e) => e.slot === slot);
}

/** Declarative pages of enabled plugins for an area (filtered server-side). */
export function usePluginPages(area: "admin" | "client"): { pages: PluginPage[]; loading: boolean } {
  const { data, isLoading } = usePluginUiData();
  return { pages: (data?.pages ?? []).filter((p) => p.area === area), loading: isLoading };
}

const NO_OVERRIDES: UiOverrides = { pages: [], hide_nav: [] };

/** Page replacements and hidden nav entries contributed by addons. */
export function useUiOverrides(): UiOverrides {
  const { data } = usePluginUiData();
  return data?.overrides ?? NO_OVERRIDES;
}

/** Contributions to a render slot (named, or generic `<area>.<route>.<pos>`), by `order`. */
export function usePluginRenderSlot(slot: RenderSlot | string): PluginSlotUi[] {
  return usePluginRenderSlots([slot]);
}

/** Contributions to any of `slots` (in the given order, then by `order`). */
export function usePluginRenderSlots(slots: string[]): PluginSlotUi[] {
  const { data } = usePluginUiData();
  const all = data?.slots ?? [];
  return slots.flatMap((name) => all.filter((s) => s.slot === name).sort((a, b) => (a.order ?? 0) - (b.order ?? 0)));
}

export type RenderSlot = "admin.dashboard" | "client.dashboard" | "admin.service.detail" | "admin.customer.detail" | "admin.order.detail" | "admin.invoice.detail" | "client.home";

/** In-app route of a plugin page. */
export const pluginPagePath = (p: Pick<PluginPage, "plugin" | "id" | "area">) => (p.area === "admin" ? `/admin/p/${p.plugin}/${p.id}` : `/p/${p.plugin}/${p.id}`);

export interface SettingsSchema {
  type: "object";
  properties: Record<string, { type: "string" | "boolean" | "number" | "integer"; title?: string }>;
}
