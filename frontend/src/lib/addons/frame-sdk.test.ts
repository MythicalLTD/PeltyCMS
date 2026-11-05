// @vitest-environment jsdom
/** The addon frame SDK (`extensions/frame-sdk/pelty-frame.js`) speaks the protocol AddonFrame checks. */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { parseFrameMessage } from "@/lib/addons/addon-frame";

const SDK = readFileSync(resolve(__dirname, "../../../../extensions/frame-sdk/pelty-frame.js"), "utf8");

type Pelty = {
  ready: (cb: (i: { locale: string }) => void) => void;
  request: (p: string, o?: { method?: string; body?: unknown }) => Promise<unknown>;
  on: (p: string, cb: (n: string, p: unknown) => void) => () => void;
  emit: (n: string, p?: unknown) => void;
  toast: (t: string, tone?: string) => void;
  navigate: (to: string) => void;
  filter: (name: string, fn: (v: unknown, ctx: Record<string, unknown>) => unknown, priority?: number) => () => void;
};

function message(w: Window, data: unknown, source: unknown) {
  const ev = new (w as unknown as { Event: typeof Event }).Event("message");
  Object.defineProperties(ev, { data: { value: data }, source: { value: source } });
  w.dispatchEvent(ev);
}

function frame() {
  const sent: unknown[] = [];
  const parent = { postMessage: (m: unknown) => void sent.push(m) };
  const el = document.createElement("iframe");
  document.body.appendChild(el);
  const w = el.contentWindow as Window & { eval: (s: string) => unknown; Pelty: Pelty };
  Object.defineProperty(w, "parent", { value: parent });
  w.eval(SDK);
  const deliver = (data: unknown) => message(w, data, parent);
  return { sent, w, deliver };
}

describe("pelty-frame.js", () => {
  it("handshakes, requests, subscribes and emits with messages the host accepts", async () => {
    const { sent, w, deliver } = frame();
    expect(sent[0]).toEqual({ type: "pelty:ready" });
    const ready = vi.fn();
    w.Pelty.ready(ready);
    const seen: string[] = [];
    w.Pelty.on("cart.*", (n) => seen.push(n));
    deliver({ type: "pelty:init", plugin: "demo", theme: { "--primary": "red" }, dark: true, locale: "de", ctx: {}, token: "t", viewer: null });
    expect(ready).toHaveBeenCalledWith(expect.objectContaining({ locale: "de" }));
    expect(w.document.documentElement.lang).toBe("de");
    expect(w.document.documentElement.classList.contains("dark")).toBe(true);

    const p = w.Pelty.request("/api/v1/addons/demo/items", { method: "post", body: { a: 1 } });
    const req = sent.find((m) => (m as { type: string }).type === "pelty:request") as { id: number };
    deliver({ type: "pelty:response", id: req.id, ok: true, status: 200, body: { items: [1] } });
    await expect(p).resolves.toEqual({ items: [1] });

    const bad = w.Pelty.request("/api/v1/admin/users");
    const req2 = sent.filter((m) => (m as { type: string }).type === "pelty:request")[1] as { id: number };
    deliver({ type: "pelty:response", id: req2.id, ok: false, status: 403, body: { error: { code: "error.forbidden" } } });
    await expect(bad).rejects.toEqual({ status: 403, error: { code: "error.forbidden" } });

    deliver({ type: "pelty:event", name: "cart.updated", payload: {} });
    deliver({ type: "pelty:event", name: "order.placed", payload: {} });
    expect(seen).toEqual(["cart.updated"]);

    w.Pelty.emit("demo:saved", { id: 1 });
    w.Pelty.toast("Saved");
    w.Pelty.navigate("/services");
    for (const m of sent) expect(parseFrameMessage(m), JSON.stringify(m)).not.toBeNull();
    expect(sent).toContainEqual({ type: "pelty:subscribe", events: ["cart.*"] });
    expect(sent).toContainEqual({ type: "pelty:emit", name: "demo:saved", payload: { id: 1 } });
  });

  it("filters: registers once per name, chains sync and async filters, fails open", async () => {
    const { sent, w, deliver } = frame();
    w.Pelty.filter("page.title", (v) => `${String(v)} A`, 5);
    w.Pelty.filter("page.title", async (v) => `${String(v)} B`);
    const stop = w.Pelty.filter("page.title", () => { throw new Error("boom"); });
    const regs = sent.filter((m) => (m as { type: string }).type === "pelty:filter_register");
    expect(regs).toEqual([{ type: "pelty:filter_register", name: "page.title", priority: 5 }]);
    expect(parseFrameMessage(regs[0])).not.toBeNull();

    const result = (id: number) => new Promise<unknown>((res) => {
      const tick = () => {
        const r = sent.find((m) => (m as { type: string; id?: number }).type === "pelty:filter_result" && (m as { id: number }).id === id);
        if (r) res(r); else setTimeout(tick, 1);
      };
      tick();
    });
    deliver({ type: "pelty:filter", id: 7, name: "page.title", value: "Invoices", ctx: { path: "/admin" } });
    const r = await result(7);
    expect(r).toEqual({ type: "pelty:filter_result", id: 7, value: "Invoices A B" });
    expect(parseFrameMessage(r)).toEqual(r);

    stop();
    deliver({ type: "pelty:filter", id: 8, name: "nav.items", value: [], ctx: {} });
    deliver({ type: "pelty:filter", id: 9, name: "page.title", value: "X", ctx: {} });
    expect(await result(9)).toEqual({ type: "pelty:filter_result", id: 9, value: "X A B" });
    expect(sent.some((m) => (m as { id?: number }).id === 8)).toBe(false);
  });

  it("the example addon ships the current SDK", () => {
    expect(readFileSync(resolve(__dirname, "../../../../extensions/examples/addon-guide/assets/pelty-frame.js"), "utf8")).toBe(SDK);
  });

  it("ignores messages that do not come from the parent", () => {
    const { w } = frame();
    const ready = vi.fn();
    w.Pelty.ready(ready);
    message(w, { type: "pelty:init" }, w);
    expect(ready).not.toHaveBeenCalled();
  });
});
