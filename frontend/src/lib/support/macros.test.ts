import { describe, expect, it } from "vitest";
import { blankMacro, macroActions, macroValid } from "@/lib/support/support";

describe("ticket macros", () => {
  it("needs a name and an action", () => {
    expect(macroValid(blankMacro)).toBe(false);
    expect(macroValid({ ...blankMacro, name: "Close", set_status: "closed" })).toBe(true);
    expect(macroValid({ ...blankMacro, name: "  ", set_status: "closed" })).toBe(false);
    expect(macroValid({ ...blankMacro, name: "Blank reply", reply_body: "   " })).toBe(false);
    expect(macroValid({ ...blankMacro, name: "Mine", assign_mode: "self" })).toBe(true);
    expect(macroValid({ ...blankMacro, name: "To", assign_mode: "user" })).toBe(false);
    expect(macroValid({ ...blankMacro, name: "To", assign_mode: "user", assign_to: "u1" })).toBe(true);
  });
  it("lists actions in run order", () => {
    expect(macroActions({ ...blankMacro, reply_body: "hi", set_status: "closed", add_tags: ["x"], assign_mode: "unassign" }))
      .toEqual(["macros.do.reply", "macros.do.status", "macros.do.assign_unassign", "macros.do.tags"]);
    expect(macroActions({ ...blankMacro, reply_body: "n", reply_internal: true, set_priority: "high", set_department: "d" }))
      .toEqual(["macros.do.note", "macros.do.priority", "macros.do.department"]);
  });
});
