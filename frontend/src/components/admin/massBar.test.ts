import { describe, expect, it } from "vitest";
import { massBody } from "./MassBar";

describe("massBody", () => {
  it("sends value only for actions with options", () => {
    expect(massBody({ key: "suspend", label: "" }, ["a"], null)).toEqual({ ids: ["a"], action: "suspend" });
    expect(massBody({ key: "set_status", label: "", options: [] }, ["a", "b"], "closed")).toEqual({ ids: ["a", "b"], action: "set_status", value: "closed" });
  });
  it("maps the none sentinel to null and honours custom bodies", () => {
    expect(massBody({ key: "set_group", label: "", options: [] }, ["a"], "__none")).toEqual({ ids: ["a"], action: "set_group", value: null });
    expect(massBody({ key: "x", label: "", body: (ids) => ({ ids, action: "assign", value: null }) }, ["a"], null)).toEqual({ ids: ["a"], action: "assign", value: null });
  });
});
