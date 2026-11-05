import { describe, expect, it } from "vitest";
import { fieldError, fieldPayload, type ClientField } from "./ClientExtras";

const f = (o: Partial<ClientField>): ClientField => ({ id: "1", name: "F", description: "", field_type: "text", options: [], required: false, admin_only: false, sort_order: 0, ...o });

describe("fieldPayload", () => {
  it("splits dropdown options and drops them for other types", () => {
    const base = { name: " Src ", description: "", options: "A\n\n B \n", required: true, admin_only: false, sort_order: 2 };
    expect(fieldPayload({ ...base, field_type: "dropdown" })).toMatchObject({ name: "Src", options: ["A", "B"], required: true, sort_order: 2 });
    expect(fieldPayload({ ...base, field_type: "text" }).options).toEqual([]);
  });
});

describe("fieldError", () => {
  it("enforces required and link format", () => {
    expect(fieldError(f({ required: true }), " ")).toBe("error.field_required");
    expect(fieldError(f({}), "")).toBeNull();
    expect(fieldError(f({ field_type: "checkbox", required: true }), "0")).toBe("error.field_required");
    expect(fieldError(f({ field_type: "link" }), "ftp://x")).toBe("fields.bad_link");
    expect(fieldError(f({ field_type: "link" }), "https://a.b")).toBeNull();
  });
});
