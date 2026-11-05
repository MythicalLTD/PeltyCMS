import { afterEach, describe, expect, it, vi } from "vitest";
import { addAsyncFilter, addFilter, applyFilters, applyFiltersAsync, emit, emitFromAddon, hasAsyncFilters, matches, on, onFilterRegistry, onHost, resetBus, validFilterValue } from "./addon-events";
import { parseFrameMessage } from "./addon-frame";

afterEach(() => resetBus());

describe("addon event bus", () => {
  it("matches exact, prefix and wildcard patterns", () => {
    expect(matches("*", "route.changed")).toBe(true);
    expect(matches("record.*", "record.viewed")).toBe(true);
    expect(matches("record.*", "route.changed")).toBe(false);
    expect(matches("my-addon:*", "my-addon:ping")).toBe(true);
  });

  it("delivers to listeners and isolates failures", () => {
    const got: string[] = [];
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    on("*", () => { throw new Error("boom"); });
    const off = onHost("route.changed", (p) => got.push(p.path));
    emit("route.changed", { path: "/x", area: "client", key: "x" });
    off();
    emit("route.changed", { path: "/y", area: "client", key: "y" });
    expect(got).toEqual(["/x"]);
    warn.mockRestore();
  });

  it("namespaces addon events", () => {
    const got: string[] = [];
    on("a:*", (n) => got.push(n));
    expect(emitFromAddon("a", "route.changed", {})).toBe("a:route.changed");
    expect(got).toEqual(["a:route.changed"]);
  });

  it("runs filters by priority and skips bad ones", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    addFilter("page.title", (t) => `${t} B`, 10);
    addFilter("page.title", (t) => `${t} A`, -5);
    addFilter("page.title", () => 42 as unknown as string);
    addFilter("page.title", () => { throw new Error("x"); });
    expect(applyFilters("page.title", "T")).toBe("T A B");
    warn.mockRestore();
  });
});

describe("frame event messages", () => {
  it("validates subscribe and emit", () => {
    expect(parseFrameMessage({ type: "pelty:subscribe", events: ["route.changed", "BAD NAME", 3] })).toEqual({ type: "pelty:subscribe", events: ["route.changed"] });
    expect(parseFrameMessage({ type: "pelty:emit", name: "ping", payload: 1 })).toEqual({ type: "pelty:emit", name: "ping", payload: 1 });
    expect(parseFrameMessage({ type: "pelty:emit", name: "*" })).toBeNull();
    expect(parseFrameMessage({ type: "pelty:subscribe" })).toBeNull();
  });
});

describe("async (frame) filters", () => {
  it("run after sync filters by priority, on a copy, with validation", async () => {
    addFilter("page.title", (v) => `${v} s`);
    const seen: unknown[] = [];
    addAsyncFilter("page.title", async (v) => `${String(v)} b`, 10, "b");
    addAsyncFilter("page.title", async (v, ctx) => { seen.push(ctx.path); return `${String(v)} a`; }, 1, "a");
    addAsyncFilter("page.title", async () => 42, 20, "bad-type");
    addAsyncFilter("page.title", async () => "x".repeat(500), 21, "too-long");
    expect(hasAsyncFilters("page.title")).toBe(true);
    expect(await applyFiltersAsync("page.title", "T", { path: "/admin" })).toBe("T s a b");
    expect(seen).toEqual(["/admin"]);

    const items = [{ to: "/admin/x", label: "X" }];
    addAsyncFilter("nav.items", async (v) => { (v as Array<{ label: string }>)[0]!.label = "mutated"; return [{ to: 5 }]; });
    expect(await applyFiltersAsync("nav.items", items, {})).toEqual(items);
    expect(items[0]!.label).toBe("X");
  });

  it("fails open on timeouts and errors, and notifies registry watchers", async () => {
    vi.useFakeTimers();
    try {
      const changes = vi.fn<() => void>();
      onFilterRegistry(changes);
      const stop = addAsyncFilter("page.title", () => new Promise(() => {}), 0, "slow");
      addAsyncFilter("page.title", () => { throw new Error("boom"); }, 1, "throws");
      addAsyncFilter("page.title", async (v) => `${String(v)}!`, 2, "ok");
      expect(changes).toHaveBeenCalledTimes(3);
      const p = applyFiltersAsync("page.title", "T", {}, 50);
      await vi.advanceTimersByTimeAsync(60);
      await expect(p).resolves.toBe("T!");
      stop();
      expect(changes).toHaveBeenCalledTimes(4);
    } finally {
      vi.useRealTimers();
    }
  });

  it("frame filter messages are validated", () => {
    expect(parseFrameMessage({ type: "pelty:filter_register", name: "page.title", priority: 1e9 })).toEqual({ type: "pelty:filter_register", name: "page.title", priority: 1000 });
    expect(parseFrameMessage({ type: "pelty:filter_register", name: "invoice.total" })).toBeNull();
    expect(parseFrameMessage({ type: "pelty:filter_result", id: "1", value: 1 })).toBeNull();
    expect(validFilterValue("nav.items", [{ to: "/a", label: "A" }])).toBe(true);
    expect(validFilterValue("unknown", "x")).toBe(false);
  });
});
