import { describe, expect, it } from "vitest";
import { activationLabel, activationTarget, fileSize } from "./digital";

describe("digital helpers", () => {
  it("labels activations", () => {
    expect(activationLabel(1, 3)).toBe("1 / 3");
    expect(activationLabel(4, 0)).toBe("4 / ∞");
  });
  it("formats sizes", () => {
    expect(fileSize(512)).toBe("512 B");
    expect(fileSize(1536)).toBe("1.5 KB");
    expect(fileSize(50 * 1024 * 1024)).toBe("50 MB");
  });
  it("describes targets", () => {
    expect(activationTarget({ domain: "a.com", ip: "", hwid: "" })).toBe("a.com");
    expect(activationTarget({ domain: "", ip: "", hwid: "" })).toBe("—");
    expect(activationTarget({ domain: "", ip: "1.2.3.4", hwid: "abcdef0123456789" })).toBe("1.2.3.4 · HWID abcdef012345");
  });
});
