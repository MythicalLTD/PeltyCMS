import { describe, expect, it } from "vitest";
import { parseUris } from "./OAuthClients";

describe("parseUris", () => {
  it("splits lines, trims and drops blanks", () => {
    expect(parseUris(" https://a.example/cb \r\n\nhttp://localhost:3000/cb\n  ")).toEqual(["https://a.example/cb", "http://localhost:3000/cb"]);
  });
});
