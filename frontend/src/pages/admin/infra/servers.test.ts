import { describe, expect, it } from "vitest";
import { fromConfig, toConfig, provisionVariant, type FieldSpec } from "./Servers";

const fields: FieldSpec[] = [
  { key: "memory", label: "provisioning.field.memory", kind: "number", required: true },
  { key: "image", label: "provisioning.field.docker_image", kind: "text", required: true },
  { key: "env", label: "provisioning.field.environment", kind: "json", required: false },
];

describe("module config forms", () => {
  it("types values and drops empties", () => {
    expect(toConfig(fields, { memory: "2048", image: " img ", env: "" })).toEqual({ memory: 2048, image: "img" });
    expect(toConfig(fields, { env: '{"A":"1"}' })).toEqual({ env: { A: "1" } });
  });
  it("reports bad JSON with the field key", () => {
    expect(() => toConfig(fields, { env: "{nope" })).toThrow("json:env");
  });
  it("round-trips stored config", () => {
    const v = fromConfig(fields, { memory: 512, env: { A: "1" } });
    expect(v.memory).toBe("512");
    expect(v.image).toBe("");
    expect(toConfig(fields, v)).toEqual({ memory: 512, env: { A: "1" } });
  });
  it("maps status to badge variants", () => {
    expect(provisionVariant("failed")).toBe("destructive");
    expect(provisionVariant("ok")).toBe("default");
  });
});
