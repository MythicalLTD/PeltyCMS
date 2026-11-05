import { describe, expect, it } from "vitest";
import { apiRel, evaluate, fillPath, lookup, resolve, text, truthy } from "./plugin-ui";

const t = (k: string) => `T(${k})`;
const scope = {
  data: { s: { items: [{ id: "a b", n: 2, name: "x" }, { id: "c", n: 3 }] } },
  ctx: { service_id: "42" },
  row: { name: "Node", status: "ok" },
};

describe("plugin-ui bindings", () => {
  it("looks up paths without prototype access", () => {
    expect(lookup(scope, "data.s.items.length")).toBe(2);
    expect(lookup(scope, "data.s.items.0.name")).toBe("x");
    expect(lookup(scope, "row.constructor")).toBeUndefined();
    expect(lookup(scope, "row.__proto__")).toBeUndefined();
  });
  it("evaluates helpers", () => {
    expect(evaluate("sum data.s.items n", scope, t)).toBe(5);
    expect(evaluate("count data.s.items", scope, t)).toBe(2);
    expect(evaluate("row.status == ok", scope, t)).toBe(true);
    expect(evaluate("row.status != ok", scope, t)).toBe(false);
    expect(evaluate("not row.missing", scope, t)).toBe(true);
    expect(evaluate("t:plugin.x.y", scope, t)).toBe("T(plugin.x.y)");
  });
  it("keeps raw values for whole bindings, interpolates otherwise", () => {
    expect(resolve("{{data.s.items}}", scope, t)).toHaveLength(2);
    expect(resolve("Hi {{row.name}}!", scope, t)).toBe("Hi Node!");
    expect(resolve({ a: "{{ctx.service_id}}", b: [1, "{{row.name}}"] }, scope, t)).toEqual({ a: "42", b: [1, "Node"] });
  });
  it("translates key-like labels only", () => {
    expect(text("plugin.demo.title", scope, t)).toBe("T(plugin.demo.title)");
    expect(text("Plain words", scope, t)).toBe("Plain words");
    expect(text("{{row.name}}", scope, t)).toBe("Node");
  });
  it("fills paths safely", () => {
    expect(fillPath("/api/v1/x/{{data.s.items.0.id}}", scope, t)).toBe("/api/v1/x/a%20b");
    expect(fillPath("/api/v1/x/{{ctx.nope}}", scope, t)).toBeNull();
    expect(fillPath("/api/v1/x/{{row.name}}/../y", { row: { name: "a" } }, t)).toBeNull();
    expect(fillPath("/api/v1/x/{{row.name}}", { row: { name: "../../admin" } }, t)).toBe("/api/v1/x/..%2F..%2Fadmin");
    expect(apiRel("/api/v1/admin/servers")).toBe("/admin/servers");
    expect(apiRel("/elsewhere")).toBeNull();
  });
  it("truthiness", () => {
    expect(truthy([])).toBe(false);
    expect(truthy("0")).toBe(true);
    expect(truthy(0)).toBe(false);
  });
});
