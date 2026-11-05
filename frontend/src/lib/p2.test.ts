import { describe, expect, it } from "vitest";
import { fileSize, referralUrl } from "./p2";

describe("p2 helpers", () => {
  it("formats sizes", () => {
    expect(fileSize(512)).toBe("512 B");
    expect(fileSize(2048)).toBe("2.0 KB");
    expect(fileSize(25 * 1024 * 1024)).toBe("25.0 MB");
  });
  it("builds referral urls", () => {
    expect(referralUrl("/r/abc123def0", "https://x.test/")).toBe("https://x.test/r/abc123def0");
  });
});
