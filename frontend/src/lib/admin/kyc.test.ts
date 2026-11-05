import { describe, expect, it } from "vitest";
import { checkFile, missingKinds, statusVariant } from "./kyc";

describe("kyc", () => {
  it("lists missing kinds", () => {
    expect(missingKinds(["identity", "address"], [])).toEqual(["identity", "address"]);
    expect(missingKinds(["identity", "address"], [{ kind: "identity", status: "pending" }, { kind: "address", status: "rejected" }])).toEqual(["address"]);
    expect(missingKinds(["identity"], [{ kind: "identity", status: "approved" }])).toEqual([]);
  });
  it("checks files", () => {
    expect(checkFile({ type: "image/png", size: 10 }, 5)).toBeNull();
    expect(checkFile({ type: "text/plain", size: 10 }, 5)).toBe("error.attachment_type");
    expect(checkFile({ type: "application/pdf", size: 6 * 1024 * 1024 }, 5)).toBe("error.attachment_too_large");
    expect(checkFile({ type: "image/jpeg", size: 0 }, 5)).toBe("error.attachment_invalid");
  });
  it("maps variants", () => {
    expect(statusVariant("verified")).toBe("default");
    expect(statusVariant("rejected")).toBe("destructive");
    expect(statusVariant("none")).toBe("outline");
  });
});
