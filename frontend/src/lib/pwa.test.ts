import { describe, expect, it, vi } from "vitest";
import { registerServiceWorker } from "./pwa";
describe("registerServiceWorker", () => {
  it("registers /sw.js in production", () => {
    const register = vi.fn(async () => ({}));
    expect(registerServiceWorker({ serviceWorker: { register } } as unknown as Navigator, true)).toBe(true);
    expect(register).toHaveBeenCalledWith("/sw.js", { scope: "/" });
  });
  it("skips in dev or unsupported browsers", () => {
    const register = vi.fn();
    expect(registerServiceWorker({ serviceWorker: { register } } as unknown as Navigator, false)).toBe(false);
    expect(registerServiceWorker({} as Navigator, true)).toBe(false);
    expect(register).not.toHaveBeenCalled();
  });
});
