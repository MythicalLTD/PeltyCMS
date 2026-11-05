import { describe, expect, it } from "vitest";
import { blankForm, formPayload, formProducts, missingAnswers, splitList, tldList } from "./orderForms";

describe("order forms", () => {
  it("normalises lists and tlds", () => {
    expect(splitList("a, B  b,a")).toEqual(["a", "b"]);
    expect(tldList(".com, RO ,,.co.uk")).toEqual(["com", "ro", "co.uk"]);
  });
  it("appends checkout and clears redirect", () => {
    const p = formPayload({ ...blankForm, slug: " VPS ", steps: ["product"], redirect_url: " ", fields: [{ key: "k", label: "K", kind: "text", required: false, options: ["x"] }] });
    expect(p.steps).toEqual(["product", "checkout"]);
    expect(p.slug).toBe("vps");
    expect(p.redirect_url).toBeNull();
    expect(p.fields[0]?.options).toEqual([]);
  });
  it("reports missing required answers", () => {
    const f = [
      { key: "a", label: "A", kind: "text" as const, required: true, options: [] },
      { key: "b", label: "B", kind: "checkbox" as const, required: true, options: [] },
      { key: "c", label: "C", kind: "text" as const, required: false, options: [] },
    ];
    expect(missingAnswers(f, { a: "", b: false })).toEqual(["a", "b"]);
    expect(missingAnswers(f, { a: "x", b: true })).toEqual([]);
  });
  it("filters products", () => {
    const items = [{ id: "1" }, { id: "2" }];
    expect(formProducts(null, items)).toHaveLength(2);
    expect(formProducts({ product_ids: ["2"] }, items)).toEqual([{ id: "2" }]);
  });
});
