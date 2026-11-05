import { describe, expect, it } from "vitest";
import { widgetFor } from "./captcha";

describe("widgetFor", () => {
  it("uses the addon's declared widget and fills the site key", () => {
    const w = widgetFor({ enabled: true, engine: "recaptcha_v3", site_key: "k 1", forms: [], widget: { script: "https://www.google.com/recaptcha/api.js?render={site_key}", global: "grecaptcha", mode: "score" } });
    expect(w).toEqual({ script: "https://www.google.com/recaptcha/api.js?render=k%201", global: "grecaptcha", mode: "score" });
  });
  it("refuses non-https scripts and bad globals, falling back to known engines", () => {
    expect(widgetFor({ enabled: true, engine: "turnstile", forms: [], widget: { script: "http://evil/x.js", global: "turnstile", mode: "widget" } })?.script).toContain("challenges.cloudflare.com");
    expect(widgetFor({ enabled: true, engine: "custom", forms: [], widget: { script: "https://x.test/a.js", global: "a;alert(1)", mode: "widget" } })).toBeNull();
    expect(widgetFor({ enabled: true, engine: "custom", forms: [], widget: { script: "https://x.test/a.js", global: "myCaptcha", mode: "widget" } })?.global).toBe("myCaptcha");
    expect(widgetFor({ enabled: true, engine: "internal", forms: [] })).toBeNull();
  });
});
