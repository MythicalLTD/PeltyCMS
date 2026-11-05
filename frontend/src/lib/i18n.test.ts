import { describe, expect, it } from "vitest";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { interpolate, pickLanguage, readCookie, translate, translateCode } from "./i18n-core";
import { STATUSES } from "./support/support";

const root = join(__dirname, "../../..");
const locale = (l: string) => JSON.parse(readFileSync(join(root, "locales", `${l}.json`), "utf8")) as Record<string, string>;

function files(dir: string, ext: RegExp): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === "node_modules" || f === "target" ? [] : files(p, ext);
    return ext.test(f) ? [p] : [];
  });
}

describe("i18n core", () => {
  it("interpolates and falls back to English then key", () => {
    expect(interpolate("Hi {name}", { name: "Ana" })).toBe("Hi Ana");
    expect(translate({ "nav.users": "Utilizatori" }, "nav.users")).toBe("Utilizatori");
    expect(translate({}, "nav.users")).toBe("Users");
    expect(translate({}, "does.not.exist")).toBe("does.not.exist");
  });
  it("picks language from cookie, browser, default", () => {
    expect(pickLanguage(["en", "ro"], "ro", ["en"])).toBe("ro");
    expect(pickLanguage(["en", "ro"], null, ["ro-RO"])).toBe("ro");
    expect(pickLanguage(["en", "ro"], "de", ["de-DE"])).toBe("en");
    expect(readCookie("pelty_lang", "a=1; pelty_lang=ro")).toBe("ro");
  });
});

describe("translation key coverage", () => {
  const en = locale("en");
  it("every frontend key exists in en", () => {
    const used = new Set<string>();
    for (const f of files(join(root, "frontend/src"), /\.tsx?$/)) {
      const src = readFileSync(f, "utf8");
      for (const m of src.matchAll(/\bt\(\s*"([a-z0-9_.]+)"/g)) used.add(m[1]!);
      for (const m of src.matchAll(/(?:label|section):\s*"([a-z0-9_.]+)"/g)) used.add(m[1]!);
    }
    expect(used.size).toBeGreaterThan(30);
    expect([...used].filter((k) => !(k in en))).toEqual([]);
  });
  it("every backend error code exists in en", () => {
    const used = new Set<string>();
    for (const f of files(join(root, "crates"), /\.rs$/)) {
      for (const m of readFileSync(f, "utf8").matchAll(/"(error\.[a-z_]+)"/g)) used.add(m[1]!);
    }
    expect(used.size).toBeGreaterThan(10);
    expect([...used].filter((k) => !(k in en))).toEqual([]);
  });
  it("UI texts fill {var} and {{var}} alike, so no stray braces render", () => {
    expect(interpolate("PeltyCMS {version} is available", { version: "1.2" })).toBe("PeltyCMS 1.2 is available");
    expect(interpolate("PeltyCMS {{version}} / {{ version }}", { version: "1.2" })).toBe("PeltyCMS 1.2 / 1.2");
    expect(interpolate("{a} {{b}} {c}", { a: 1 })).toBe("1 {{b}} {c}");
  });
  it("every locale has the same keys as en", () => {
    for (const f of readdirSync(join(root, "locales")).filter((f) => f.endsWith(".json"))) {
      const l = locale(f.replace(".json", ""));
      const missing = Object.keys(en).filter((k) => !(k in l));
      const extra = Object.keys(l).filter((k) => !(k in en));
      expect({ file: f, missing, extra }).toEqual({ file: f, missing: [], extra: [] });
    }
  });
  it("plugin and theme locale files match their own en", () => {
    for (const kind of ["plugins", "themes"]) {
      for (const id of readdirSync(join(root, "extensions", kind))) {
        const dir = join(root, "extensions", kind, id, "locales");
        let files: string[] = [];
        try { files = readdirSync(dir).filter((f) => f.endsWith(".json")); } catch { continue; }
        const base = JSON.parse(readFileSync(join(dir, "en.json"), "utf8")) as Record<string, string>;
        for (const f of files) {
          const l = JSON.parse(readFileSync(join(dir, f), "utf8")) as Record<string, string>;
          expect({ ext: `${kind}/${id}/${f}`, missing: Object.keys(base).filter((k) => !(k in l)) }).toEqual({ ext: `${kind}/${id}/${f}`, missing: [] });
        }
      }
    }
  });
});

import { translate as tr } from "./i18n-core";
describe("plurals", () => {
  const m = { "x": "{count} items", "x#one": "{count} item", "x#few": "{count} itemi", "x#other": "{count} de itemi" };
  it("uses CLDR rules per language", () => {
    expect(tr(m, "x", { count: 1 }, "en")).toBe("1 item");
    expect(tr(m, "x", { count: 5 }, "en")).toBe("5 de itemi");
    expect(tr(m, "x", { count: 5 }, "ro")).toBe("5 itemi");
    expect(tr(m, "x", { count: 25 }, "ro")).toBe("25 de itemi");
  });
});

describe("status and error codes", () => {
  const en = locale("en");
  const ro = locale("ro");
  it("every ticket status has status.* in en and ro", () => {
    for (const s of STATUSES) {
      expect(en[`status.${s}`], s).toBeTruthy();
      expect(ro[`status.${s}`], s).toBeTruthy();
    }
  });
  it("every status.* / error.* key in en exists in ro", () => {
    const keys = Object.keys(en).filter((k) => k.startsWith("status.") || k.startsWith("error."));
    expect(keys.filter((k) => !(k in ro))).toEqual([]);
  });
  it("literal status.* / error.* codes used in the frontend exist in both locales", () => {
    const used = new Set<string>();
    for (const f of files(join(root, "frontend/src"), /\.tsx?$/).filter((f) => !/\.test\.tsx?$/.test(f))) {
      for (const m of readFileSync(f, "utf8").matchAll(/["'`]((?:status|error)\.[a-z0-9_]+)["'`]/g)) used.add(m[1]!);
    }
    expect([...used].filter((k) => !(k in en) || !(k in ro))).toEqual([]);
  });
  it("unknown custom statuses fall back to a readable label", () => {
    expect(translateCode({}, "status.waiting_on_vendor")).toBe("Waiting on vendor");
    expect(translateCode({ "tickets.status.foo": "Foo!" }, "status.foo")).toBe("Foo!");
    expect(translateCode({}, "error.nope")).toBe("error.nope");
  });
});
