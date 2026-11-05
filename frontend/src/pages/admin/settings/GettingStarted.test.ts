import { describe, expect, it } from "vitest";
import { requiredProgress } from "./GettingStarted";

describe("requiredProgress", () => {
  it("counts only required steps", () => {
    const steps = [
      { id: "company", done: true, optional: false, href: "/" },
      { id: "email", done: false, optional: false, href: "/" },
      { id: "tax", done: true, optional: true, href: "/" },
    ];
    expect(requiredProgress({ steps, complete: false, dismissed: false, show: true })).toEqual([1, 2]);
  });
});
