import { describe, expect, it } from "vitest";
import { cleanMapping, missingRequired } from "./csvImport";

describe("csv import mapping", () => {
  it("lists unmapped required fields and cleans blanks", () => {
    const f = [{ key: "email", required: true }, { key: "phone", required: false }];
    expect(missingRequired(f, { phone: "Tel" })).toEqual(["email"]);
    expect(missingRequired(f, { email: "Mail" })).toEqual([]);
    expect(cleanMapping({ email: "Mail", phone: "" })).toEqual({ email: "Mail" });
  });
});
