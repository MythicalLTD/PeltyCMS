import { describe, expect, it } from "vitest";
import { formatDate, formatDateTime, formatNumber, parseUserAgent } from "./format";

describe("format", () => {
  it("dates per locale", () => {
    expect(formatDate("2026-11-29", "en")).toBe("2026-11-29");
    expect(formatDate("2026-11-29", "ro")).toBe("29.11.2026");
    expect(formatDate("2026-11-29T10:00:00Z", "ro")).toMatch(/^29\.11\.2026$/);
    expect(formatDateTime("2026-11-29T10:05:00Z", "ro", "UTC")).toContain("29.11.2026");
    expect(formatDateTime("2026-11-29T10:05:00Z", "en", "UTC")).toContain("2026-11-29");
    expect(formatDate(null, "en")).toBe("-");
  });
  it("numbers", () => {
    expect(formatNumber(1234.5, "ro")).toBe("1.234,5");
    expect(formatNumber(1234.5, "en")).toBe("1,234.5");
  });
  it("user agents", () => {
    expect(parseUserAgent("Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0")).toEqual({ browser: "Firefox", os: "Windows" });
    expect(parseUserAgent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1")).toEqual({ browser: "Safari", os: "iOS" });
    expect(parseUserAgent("Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36")).toEqual({ browser: "Chrome", os: "Android" });
    expect(parseUserAgent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36 Edg/130.0")).toEqual({ browser: "Edge", os: "macOS" });
    expect(parseUserAgent(null)).toEqual({ browser: null, os: null });
  });
});
