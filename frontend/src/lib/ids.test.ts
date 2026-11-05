import { describe, expect, it } from "vitest";
import { isUuid, newUuid } from "./ids";

describe("newUuid", () => {
  it("returns a v4-shaped UUID without relying on crypto.randomUUID", () => {
    const orig = globalThis.crypto?.randomUUID;
    try {
      if (globalThis.crypto) {
        // Simulate plain-HTTP / non-secure context.
        Object.defineProperty(globalThis.crypto, "randomUUID", { value: undefined, configurable: true });
      }
      const id = newUuid();
      expect(isUuid(id)).toBe(true);
      expect(id.charAt(14)).toBe("4");
      expect("89ab").toContain(id.charAt(19).toLowerCase());
    } finally {
      if (globalThis.crypto && orig) {
        Object.defineProperty(globalThis.crypto, "randomUUID", { value: orig, configurable: true });
      }
    }
  });
});
