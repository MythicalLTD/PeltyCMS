import { describe, expect, it } from "vitest";
import { address, slugify } from "./gaming";

describe("gaming helpers", () => {
  it("slugifies names", () => {
    expect(slugify("Survival EU #1")).toBe("survival-eu-1");
    expect(slugify("Ștefan's Server")).toBe("stefan-s-server");
  });
  it("formats addresses", () => {
    expect(address({ host: "play.x", port: 25565 })).toBe("play.x:25565");
    expect(address({ host: "play.x", port: null })).toBe("play.x");
    expect(address({ host: "", port: 1 })).toBe("");
  });
});
