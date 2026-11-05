import { describe, expect, it } from "vitest";
import { pathAllowed, type ClientAccess } from "./clientAccess";

const base: ClientAccess = {
  owner: false,
  services: false,
  invoices: false,
  tickets: false,
  orders: false,
  domains: false,
};

describe("pathAllowed", () => {
  it("allows everything for owners and when access is unknown", () => {
    expect(pathAllowed("/invoices", undefined)).toBe(true);
    expect(pathAllowed("/invoices", { ...base, owner: true })).toBe(true);
  });

  it("gates billing, services, tickets, orders, domains", () => {
    expect(pathAllowed("/invoices/x", base)).toBe(false);
    expect(pathAllowed("/billing", { ...base, invoices: true })).toBe(true);
    expect(pathAllowed("/quotes/1", { ...base, invoices: true })).toBe(true);
    expect(pathAllowed("/services/1", base)).toBe(false);
    expect(pathAllowed("/store", { ...base, services: true })).toBe(true);
    expect(pathAllowed("/order", { ...base, services: true })).toBe(true);
    expect(pathAllowed("/tickets", base)).toBe(false);
    expect(pathAllowed("/projects/1", { ...base, tickets: true })).toBe(true);
    expect(pathAllowed("/orders", base)).toBe(false);
    expect(pathAllowed("/orders/1", { ...base, orders: true })).toBe(true);
    expect(pathAllowed("/domains", base)).toBe(false);
    expect(pathAllowed("/domains/1", { ...base, domains: true })).toBe(true);
    expect(pathAllowed("/ssl", base)).toBe(false);
    expect(pathAllowed("/ssl", { ...base, domains: true })).toBe(true);
  });

  it("allows account and knowledge routes by default", () => {
    expect(pathAllowed("/account", base)).toBe(true);
    expect(pathAllowed("/my-details", base)).toBe(true);
    expect(pathAllowed("/kb", base)).toBe(true);
    expect(pathAllowed("/dashboard", base)).toBe(true);
    expect(pathAllowed("/emails", base)).toBe(true);
  });

  it("gates downloads to services and affiliate to owners", () => {
    expect(pathAllowed("/downloads", base)).toBe(false);
    expect(pathAllowed("/downloads", { ...base, services: true })).toBe(true);
    expect(pathAllowed("/affiliate", base)).toBe(false);
    expect(pathAllowed("/affiliate", { ...base, owner: true })).toBe(true);
  });
});
