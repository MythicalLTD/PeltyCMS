// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { applyCustomCode, httpsOrEmpty } from "./general";

describe("general settings", () => {
  it("validates https urls", () => {
    expect(httpsOrEmpty("")).toBe(true);
    expect(httpsOrEmpty("https://x.test")).toBe(true);
    expect(httpsOrEmpty("http://x.test")).toBe(false);
    expect(httpsOrEmpty("javascript:alert(1)")).toBe(false);
  });
  it("injects and replaces custom code", () => {
    applyCustomCode(document, { css: ".a{}", head_js: "", footer_js: "" });
    expect(document.getElementById("pelty-custom-css")?.textContent).toBe(".a{}");
    expect(document.getElementById("pelty-custom-head-js")).toBeNull();
    applyCustomCode(document, { css: ".b{}", head_js: "", footer_js: "" });
    expect(document.querySelectorAll("#pelty-custom-css").length).toBe(1);
    expect(document.getElementById("pelty-custom-css")?.textContent).toBe(".b{}");
    applyCustomCode(document, { css: "", head_js: "", footer_js: "" });
    expect(document.getElementById("pelty-custom-css")).toBeNull();
  });
});
