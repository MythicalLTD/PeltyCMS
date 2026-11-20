import { describe, expect, it } from "vitest";
import { descriptionSummary, parseDescription } from "./RichDescription";

describe("parseDescription", () => {
  it("groups bullets into lists and keeps headings and paragraphs", () => {
    const parts = parseDescription("Intro text.\nPerks:\n- One\n* Two\n• Three\n\nOutro");
    expect(parts).toEqual([
      { kind: "p", text: "Intro text." },
      { kind: "h", text: "Perks" },
      { kind: "list", items: ["One", "Two", "Three"] },
      { kind: "p", text: "Outro" },
    ]);
  });
  it("summary is the first paragraph", () => {
    expect(descriptionSummary("- a\nHello\nWorld")).toBe("Hello");
    expect(descriptionSummary("")).toBe("");
  });
});
