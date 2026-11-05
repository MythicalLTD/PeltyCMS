import { describe, expect, it } from "vitest";
import { overallStatus, providerFields, searchable, searchHref } from "./adminTools";

describe("admin tools", () => {
  it("search length rule", () => {
    expect(searchable("a")).toBe(false);
    expect(searchable(" ab ")).toBe(true);
    expect(searchable("x".repeat(101))).toBe(false);
  });
  it("hrefs", () => {
    expect(searchHref("invoices", { id: "1" })).toBe("/admin/invoices/1");
    expect(searchHref("customers", { id: "1", email: "a@b.c" })).toBe("/admin/customers/1");
  });
  it("overall status", () => {
    expect(overallStatus([{ key: "a", status: "ok" }])).toBe("ok");
    expect(overallStatus([{ key: "a", status: "warning" }, { key: "b", status: "error" }])).toBe("error");
  });
});
import { displayName } from "./clientSummary";
describe("client summary", () => {
  it("display name falls back to organization then email", () => {
    expect(displayName({ first_name: "A", last_name: "B", organization: "O", email: "e" })).toBe("A B");
    expect(displayName({ first_name: "", last_name: "", organization: "O", email: "e" })).toBe("O");
    expect(displayName({ first_name: "", last_name: "", organization: "", email: "e" })).toBe("e");
  });
});

describe("providerFields", () => {
  it("asks for the right inputs per channel", () => {
    expect(providerFields("slack")).toEqual({ url: true, target: false });
    expect(providerFields("telegram")).toEqual({ url: true, target: true });
    expect(providerFields("sms_email")).toEqual({ url: false, target: true });
  });
});
