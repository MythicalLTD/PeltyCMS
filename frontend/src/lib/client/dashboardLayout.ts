/** Client dashboard layout: order, visibility and size of each widget (persisted per user). */
export const CLIENT_WIDGETS = ["quick_actions", "quotes", "addons", "stats", "spend_chart", "recent_invoices", "home", "upcoming"] as const;
export type WidgetId = (typeof CLIENT_WIDGETS)[number];
export interface LayoutWidget { id: WidgetId; visible: boolean; size: "full" | "half" }
export interface DashboardLayout { widgets: LayoutWidget[] }

export const defaultLayout = (): DashboardLayout => ({
  widgets: CLIENT_WIDGETS.map((id) => ({ id, visible: true, size: id === "spend_chart" || id === "recent_invoices" ? "half" : "full" })),
});

/** Drop unknown ids and duplicates, append widgets missing from an older saved layout. */
export function normalizeLayout(l: Partial<DashboardLayout> | null | undefined): DashboardLayout {
  const seen = new Set<string>();
  const out: LayoutWidget[] = [];
  for (const w of l?.widgets ?? []) {
    if (!w || !(CLIENT_WIDGETS as readonly string[]).includes(w.id) || seen.has(w.id)) continue;
    seen.add(w.id);
    out.push({ id: w.id, visible: w.visible !== false, size: w.size === "half" ? "half" : "full" });
  }
  for (const d of defaultLayout().widgets) if (!seen.has(d.id)) out.push(d);
  return { widgets: out };
}

/** Move the widget `from` to the position of `to` (drag-and-drop drop target). */
export function reorder(l: DashboardLayout, from: WidgetId, to: WidgetId): DashboardLayout {
  const i = l.widgets.findIndex((w) => w.id === from);
  const j = l.widgets.findIndex((w) => w.id === to);
  if (i < 0 || j < 0 || i === j) return l;
  const ws = l.widgets.slice();
  const [x] = ws.splice(i, 1);
  ws.splice(j, 0, x as LayoutWidget);
  return { widgets: ws };
}

export function patchWidget(l: DashboardLayout, id: WidgetId, p: Partial<LayoutWidget>): DashboardLayout {
  return { widgets: l.widgets.map((w) => (w.id === id ? { ...w, ...p } : w)) };
}
