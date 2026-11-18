import { describe, expect, it } from "vitest";
import { address, deliveryQueueQuery, slugify } from "./gaming";

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

describe("command placeholders", () => {
  it("finds and validates placeholders", async () => {
    const { commandPlaceholders, unknownPlaceholders } = await import("./gaming");
    expect(commandPlaceholders("lp user {username} parent add {package}")).toEqual(["username", "package"]);
    expect(unknownPlaceholders("give {username} {nope}", ["username"])).toEqual(["nope"]);
    expect(unknownPlaceholders("say hi", [])).toEqual([]);
  });
});

describe("delivery log query", () => {
  it("adds only the filters that are set", () => {
    expect(deliveryQueueQuery(null, null)).toBe("");
    expect(deliveryQueueQuery("failed", null)).toBe("?status=failed");
    expect(deliveryQueueQuery(null, "abc")).toBe("?server_id=abc");
    expect(deliveryQueueQuery("delivered", "abc")).toBe("?status=delivered&server_id=abc");
  });
});
