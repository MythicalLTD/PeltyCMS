import { describe, expect, it } from "vitest";
import { connect, matches, type FrameWindow } from "./index";

function fakeWindow() {
  const sent: Array<Record<string, unknown>> = [];
  const parent = { postMessage: (m: unknown) => { sent.push(m as Record<string, unknown>); } };
  let handler: ((e: { source: unknown; data: unknown }) => void) | null = null;
  const w: FrameWindow = { parent, addEventListener: (_t, f) => { handler = f; }, removeEventListener: () => { handler = null; } };
  const deliver = (data: unknown, source: unknown = parent) => handler?.({ source, data });
  return { w, sent, deliver, parent };
}

describe("addon-ui sdk", () => {
  it("handshakes, requests, subscribes and emits", async () => {
    const { w, sent, deliver } = fakeWindow();
    const p = connect({ window: w });
    expect(sent[0]).toEqual({ type: "pelty:ready" });
    deliver({ type: "pelty:init", plugin: "demo", theme: { "--primary": "red" }, dark: true, locale: "ro", ctx: { id: "7" }, events: ["record.viewed"], sdk: 2 });
    const ui = await p;
    expect(ui.context.plugin).toBe("demo");
    expect(ui.t({ en: "Saved", ro: "Salvat" })).toBe("Salvat");

    const r = ui.api.get<{ n: number }>("/api/v1/addons/demo/x");
    const req = sent.find((m) => m.type === "pelty:request")!;
    expect(req).toMatchObject({ method: "GET", path: "/api/v1/addons/demo/x" });
    deliver({ type: "pelty:response", id: req.id, ok: true, status: 200, body: { n: 3 } });
    expect(await r).toEqual({ n: 3 });

    const bad = ui.api.post("/api/v1/invoices", {});
    const req2 = sent.filter((m) => m.type === "pelty:request")[1]!;
    deliver({ type: "pelty:response", id: req2.id, ok: false, status: 403, body: { error: { code: "error.forbidden" } } });
    await expect(bad).rejects.toMatchObject({ status: 403, code: "error.forbidden" });

    const got: unknown[] = [];
    const off = ui.on("record.viewed", (e) => got.push(e.id));
    expect(sent.at(-1)).toEqual({ type: "pelty:subscribe", events: ["record.viewed"] });
    deliver({ type: "pelty:event", name: "record.viewed", payload: { kind: "invoices", id: "9", area: "admin" } });
    deliver({ type: "pelty:event", name: "record.viewed", payload: { id: "x" } }, {} /* not the host */);
    expect(got).toEqual(["9"]);
    off();
    expect(sent.at(-1)).toEqual({ type: "pelty:subscribe", events: [] });

    ui.emit("saved", { a: 1 });
    expect(sent.at(-1)).toEqual({ type: "pelty:emit", name: "saved", payload: { a: 1 } });

    // Re-init updates the context.
    const seen: string[] = [];
    ui.onContext((c) => seen.push(c.locale));
    deliver({ type: "pelty:init", plugin: "demo", theme: {}, dark: false, locale: "en", ctx: {}, events: [], sdk: 2 });
    expect(seen).toEqual(["en"]);
    expect(ui.context.locale).toBe("en");
  });

  it("times out without a host", async () => {
    const { w } = fakeWindow();
    await expect(connect({ window: w, timeoutMs: 10 })).rejects.toMatchObject({ code: "error.timeout" });
  });

  it("matches wildcards", () => {
    expect(matches("record.*", "record.viewed")).toBe(true);
    expect(matches("billing:*", "crm:x")).toBe(false);
  });
});
