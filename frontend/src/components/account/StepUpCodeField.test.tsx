// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { I18nProvider } from "@/lib/i18n";
import { StepUpCodeField } from "./StepUpCodeField";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
function mount(mfa: boolean) {
  vi.stubGlobal("fetch", vi.fn(async (u: string) => new Response(JSON.stringify(u.endsWith("/i18n") ? { languages: [] } : { id: "u", email: "a@b.c", mfa_enabled: mfa }), { status: 200 })));
  const qc = new QueryClient();
  render(<QueryClientProvider client={qc}><I18nProvider><p>ready</p><StepUpCodeField id="c" value="" onChange={() => {}} /></I18nProvider></QueryClientProvider>);
  return qc;
}
describe("StepUpCodeField", () => {
  it("hides the code field once the account is known to have no 2FA", async () => {
    const qc = mount(false);
    await screen.findByText("ready");
    await waitFor(() => expect(qc.getQueryState(["me"])?.status).toBe("success"));
    expect(qc.getQueryData<{ mfa_enabled: boolean }>(["me"])?.mfa_enabled).toBe(false);
    expect(document.getElementById("c")).toBeNull();
  });
  it("shows a required code field when the account has 2FA", async () => {
    mount(true);
    await waitFor(() => expect(document.getElementById("c")).not.toBeNull());
    expect((document.getElementById("c") as HTMLInputElement).required).toBe(true);
  });
});
