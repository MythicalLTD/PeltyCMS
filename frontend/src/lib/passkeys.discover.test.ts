import { afterEach, describe, expect, it, vi } from "vitest";

const calls: { path: string; body?: unknown }[] = [];
vi.mock("@/lib/api", () => ({
  api: vi.fn<(path: string, o?: { body?: unknown }) => Promise<unknown>>(async (path: string, o?: { body?: unknown }) => {
    calls.push({ path, body: o?.body });
    if (path.endsWith("/discover/start")) return { challenge_id: "c1", options: { publicKey: { challenge: "AQ", userVerification: "required" } } };
    return { email: "a@example.com" };
  }),
}));

import { conditionalMediationAvailable, passkeyDiscoverLogin } from "./passkeys";

const buf = (n: number[]) => new Uint8Array(n).buffer;

afterEach(() => { calls.length = 0; vi.unstubAllGlobals(); });

function stub(get: (o: CredentialRequestOptions) => unknown, conditional = true) {
  const pkc = { isConditionalMediationAvailable: async () => conditional };
  vi.stubGlobal("PublicKeyCredential", pkc);
  if (typeof window === "undefined") vi.stubGlobal("window", { PublicKeyCredential: pkc });
  else vi.stubGlobal("window", Object.assign(window, { PublicKeyCredential: pkc }));
  vi.stubGlobal("navigator", { credentials: { get: vi.fn<(o: CredentialRequestOptions) => Promise<unknown>>(async (o: CredentialRequestOptions) => get(o)) } });
}

describe("usernameless passkey login", () => {
  it("sends no e-mail, no credential hint, and forwards the user handle", async () => {
    let seen: CredentialRequestOptions | undefined;
    stub((o) => {
      seen = o;
      return { id: "k", rawId: buf([9]), type: "public-key", response: { clientDataJSON: buf([1]), authenticatorData: buf([2]), signature: buf([3]), userHandle: buf([7, 7]) } };
    });
    const ac = new AbortController();
    const me = await passkeyDiscoverLogin<{ email: string }>({ conditional: true, signal: ac.signal });
    expect(me.email).toBe("a@example.com");
    expect(calls[0]).toEqual({ path: "/auth/passkeys/discover/start", body: undefined });
    expect(seen?.mediation).toBe("conditional");
    expect(seen?.signal).toBe(ac.signal);
    expect(seen?.publicKey?.allowCredentials).toEqual([]);
    const fin = calls[1]!.body as { challenge_id: string; credential: { response: { userHandle: string } } };
    expect(calls[1]!.path).toBe("/auth/passkeys/discover/finish");
    expect(fin.challenge_id).toBe("c1");
    expect(fin.credential.response.userHandle).toBe("Bwc");
  });
  it("explicit button runs without conditional mediation; cancel throws", async () => {
    let seen: CredentialRequestOptions | undefined;
    stub((o) => { seen = o; return null; });
    await expect(passkeyDiscoverLogin()).rejects.toThrow("cancelled");
    expect(seen?.mediation).toBeUndefined();
    expect(calls.map((c) => c.path)).toEqual(["/auth/passkeys/discover/start"]);
  });
  it("detects conditional UI support", async () => {
    stub(() => null, true);
    expect(await conditionalMediationAvailable()).toBe(true);
    stub(() => null, false);
    expect(await conditionalMediationAvailable()).toBe(false);
  });
});
