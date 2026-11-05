import { describe, expect, it } from "vitest";
import { applyTheme, type Theme } from "./theme";

describe("applyTheme", () => {
  it("sets only safe CSS variables", () => {
    const set: Record<string, string> = {};
    const root = { style: { setProperty: (k: string, v: string) => void (set[k] = v) } };
    const theme: Theme = {
      id: "x", name: "X", version: "1", author: "", description: "", color_scheme: "light",
      tokens: { primary: "oklch(0.5 0.2 260)", radius: "1rem;}body{display:none", "Bad Key": "red" },
      logo_url: null, stylesheet_url: null,
    };
    expect(applyTheme(theme, root)).toBe(1);
    expect(set["--primary"]).toBe("oklch(0.5 0.2 260)");
    expect(set["--sidebar-primary"]).toBe("oklch(0.5 0.2 260)");
    expect(set["--radius"]).toBeUndefined();
    expect(set["color-scheme"]).toBe("light");
  });
  it("removes tokens of the previous theme when switching", () => {
    const set: Record<string, string> = {};
    const root = { style: { setProperty: (k: string, v: string) => void (set[k] = v), removeProperty: (k: string) => void delete set[k] } };
    const base = { id: "a", name: "A", version: "1", author: "", description: "", logo_url: null, stylesheet_url: null };
    applyTheme({ ...base, color_scheme: "dark", tokens: { primary: "red", accent: "blue" } }, root);
    applyTheme({ ...base, id: "b", color_scheme: "light", tokens: { primary: "green" } }, root);
    expect(set).toEqual({ "--primary": "green", "--sidebar-primary": "green", "--chart-1": "green", "color-scheme": "light" });
  });
});
