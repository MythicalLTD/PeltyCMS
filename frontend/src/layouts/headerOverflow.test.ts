import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

// jsdom has no layout engine, so guard the classes that keep the header within a 390px viewport.
const shell = readFileSync(resolve(__dirname, "Shell.tsx"), "utf8");
const header = shell.slice(shell.indexOf('data-testid="app-header"'), shell.indexOf("</header>"));

describe("app header on mobile", () => {
  it("clips instead of overflowing", () => {
    expect(header).toMatch(/min-w-0[^"]*overflow-hidden/);
  });
  it("keeps the breadcrumb on one shrinkable line", () => {
    expect(header).toMatch(/<Breadcrumb className="min-w-0 flex-1 overflow-hidden">/);
    expect(header).toContain("flex-nowrap");
    expect(header).toContain("truncate");
  });
  it("hides secondary controls below sm and keeps actions unshrinkable", () => {
    expect(header).toContain('<div className="hidden sm:block"><LanguageSwitcher /></div>');
    expect(header).toContain("ml-auto flex shrink-0");
  });
});
