import { describe, expect, it } from "vitest";
import { defaultPort, formatMinutes, validEmail } from "./supportExt";

describe("supportExt", () => {
  it("ports", () => {
    expect(defaultPort("imap", true)).toBe(993);
    expect(defaultPort("pop3", false)).toBe(110);
  });
  it("minutes", () => {
    expect(formatMinutes(30)).toBe("30m");
    expect(formatMinutes(120)).toBe("2h");
    expect(formatMinutes(95)).toBe("1h 35m");
  });
  it("email", () => {
    expect(validEmail("a@b.co")).toBe(true);
    expect(validEmail("a@b")).toBe(false);
    expect(validEmail("a b@c.d")).toBe(false);
  });
});
