import { describe, expect, it } from "vitest";
import { b64urlToBytes, bytesToB64url, credentialJson, creationOptions, requestOptions } from "./passkeys";

describe("passkeys encoding", () => {
  it("round-trips base64url without padding", () => {
    for (const s of ["", "AA", "AAE", "_-8", "TnH5LUwwlo_tqePK6Dwq11rWW4NYJFFKav8xOb3TEBs"]) expect(bytesToB64url(b64urlToBytes(s))).toBe(s);
    expect([...b64urlToBytes("_-8")]).toEqual([255, 239]);
  });
  it("decodes creation and request options", () => {
    const c = creationOptions({ publicKey: { challenge: "AQID", user: { id: "BAU", name: "a", displayName: "A" }, excludeCredentials: [{ type: "public-key", id: "Bg" }], rp: { id: "localhost" } } });
    expect([...new Uint8Array(c.challenge as ArrayBuffer)]).toEqual([1, 2, 3]);
    expect([...new Uint8Array(c.user.id as ArrayBuffer)]).toEqual([4, 5]);
    expect([...new Uint8Array(c.excludeCredentials![0]!.id as ArrayBuffer)]).toEqual([6]);
    expect(c.rp).toEqual({ id: "localhost" });
    const r = requestOptions({ publicKey: { challenge: "AQ", userVerification: "required" } });
    expect(r.allowCredentials).toEqual([]);
    expect(r.userVerification).toBe("required");
  });
  it("encodes an assertion", () => {
    const buf = (n: number[]) => new Uint8Array(n).buffer;
    const cred = { id: "x", rawId: buf([1]), type: "public-key", response: { clientDataJSON: buf([2]), authenticatorData: buf([3]), signature: buf([4]), userHandle: null } } as unknown as PublicKeyCredential;
    expect(credentialJson(cred)).toEqual({ id: "x", rawId: "AQ", type: "public-key", response: { clientDataJSON: "Ag", authenticatorData: "Aw", signature: "BA", userHandle: null }, extensions: {} });
  });
});
