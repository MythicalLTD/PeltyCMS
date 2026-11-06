import { describe, expect, it } from "vitest";
import { defaultDesign, fromHtml, insertAt, move, newBlock, safeUrl, toHtml, toText } from "./emailBlocks";

describe("emailBlocks", () => {
  it("round-trips the design through HTML", () => {
    const d = defaultDesign();
    const html = toHtml(d);
    expect(html).toContain("<table");
    expect(fromHtml(html)).toEqual(d);
    expect(fromHtml("<p>hand written</p>")).toBeNull();
  });
  it("escapes text and blocks unsafe URLs", () => {
    const d = { accent: "#000", background: "#fff", blocks: [{ ...newBlock("text"), text: "<script>x</script>" }, { id: "b", type: "button" as const, text: "Go", url: "javascript:alert(1)" }] };
    const html = toHtml(d);
    expect(html).not.toContain("<script>");
    expect(html).toContain('href="#"');
    expect(safeUrl("{{invoice_url}}")).toBe("{{invoice_url}}");
    expect(safeUrl("https://x.test")).toBe("https://x.test");
    expect(safeUrl("/api/v1/media/550e8400-e29b-41d4-a716-446655440000")).toBe(
      "/api/v1/media/550e8400-e29b-41d4-a716-446655440000",
    );
  });
  it("moves blocks and inserts merge fields at the caret", () => {
    expect(move([1, 2, 3], 0, 2)).toEqual([2, 3, 1]);
    expect(insertAt("Hi !", 3, "name")).toEqual({ value: "Hi {{name}}!", caret: 11 });
    expect(toText(defaultDesign())).toContain("{{url}}");
    expect(toText(defaultDesign(["company"]))).not.toContain("{{");
  });
});
