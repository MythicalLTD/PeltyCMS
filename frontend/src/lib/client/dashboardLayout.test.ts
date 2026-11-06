import { describe, expect, it } from "vitest";
import { CLIENT_WIDGETS, defaultLayout, normalizeLayout, patchWidget, reorder } from "./dashboardLayout";

describe("dashboard layout", () => {
  it("normalizes saved layouts", () => {
    const n = normalizeLayout({ widgets: [{ id: "stats", visible: false, size: "half" }, { id: "stats", visible: true, size: "full" }, { id: "bogus" as never, visible: true, size: "full" }] });
    expect(n.widgets[0]).toEqual({ id: "stats", visible: false, size: "half" });
    expect(n.widgets).toHaveLength(CLIENT_WIDGETS.length);
    expect(normalizeLayout(null)).toEqual(defaultLayout());
  });
  it("reorders and patches", () => {
    const l = reorder(defaultLayout(), "upcoming", "quick_actions");
    expect(l.widgets[0]?.id).toBe("upcoming");
    expect(patchWidget(l, "stats", { visible: false }).widgets.find((w) => w.id === "stats")?.visible).toBe(false);
  });
});
