import { describe, expect, it } from "vitest";
import { createI18n, interpolate, PeltyClient, PeltyError } from "./index";

function fakeFetch(routes: Record<string, [number, unknown]>, seen: RequestInit[] = []) {
  return (async (url: string, init: RequestInit) => {
    seen.push(init);
    const key = `${init.method} ${url}`;
    const [status, body] = routes[key] ?? [404, { error: { code: "error.not_found", message: "nf" } }];
    return new Response(status === 204 ? null : JSON.stringify(body), { status });
  }) as unknown as typeof fetch;
}

describe("theme sdk", () => {
  it("sends csrf after login and maps errors", async () => {
    const seen: RequestInit[] = [];
    const c = new PeltyClient({
      fetch: fakeFetch(
        {
          "POST /api/v1/auth/login": [200, { id: "1", csrf_token: "tok" }],
          "POST /api/v1/auth/logout": [204, null],
          "GET /api/v1/auth/me": [401, { error: { code: "error.unauthorized", message: "no" } }],
        },
        seen,
      ),
    });
    await c.login("a@b.c", "x");
    await c.logout();
    expect((seen[1]!.headers as Record<string, string>)["x-csrf-token"]).toBe("tok");
    expect(await c.me()).toBeNull();
    await expect(c.sessions()).rejects.toBeInstanceOf(PeltyError);
  });

  it("activates a theme with csrf", async () => {
    const seen: RequestInit[] = [];
    const c = new PeltyClient({
      fetch: fakeFetch(
        {
          "GET /api/v1/auth/me": [200, { id: "1", csrf_token: "tok", permissions: ["settings.manage"] }],
          "PUT /api/v1/admin/themes/active": [204, null],
        },
        seen,
      ),
    });
    await c.setActiveTheme("default");
    expect(seen[0]!.method).toBe("GET");
    expect(seen[1]!.method).toBe("PUT");
    expect((seen[1]!.headers as Record<string, string>)["x-csrf-token"]).toBe("tok");
    expect(JSON.parse(String(seen[1]!.body))).toEqual({ id: "default" });
  });

  it("i18n falls back per key to en", async () => {
    const c = new PeltyClient({
      fetch: fakeFetch({
        "GET /api/v1/i18n": [200, { languages: [{ code: "en", name: "English" }], current: "ro" }],
        "GET /api/v1/i18n/en": [200, { a: "A", b: "Hi {n}" }],
        "GET /api/v1/i18n/ro": [200, { a: "Ă" }],
      }),
    });
    const { t, lang } = await createI18n(c);
    expect(lang).toBe("ro");
    expect(t("a")).toBe("Ă");
    expect(t("b", { n: 1 })).toBe("Hi 1");
    expect(t("zzz")).toBe("zzz");
    expect(interpolate("{x}{y}", { x: 1 })).toBe("1{y}");
  });
});
