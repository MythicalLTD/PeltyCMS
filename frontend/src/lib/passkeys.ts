import { api } from "@/lib/api";

export interface PasskeyRow { id: string; name: string; created_at: string; last_used_at: string | null }

export function b64urlToBytes(s: string): Uint8Array<ArrayBuffer> {
  const b64 = s.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(s.length / 4) * 4, "=");
  const bin = atob(b64);
  const out = new Uint8Array(new ArrayBuffer(bin.length));
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}

export function bytesToB64url(b: ArrayBuffer | Uint8Array): string {
  const u = b instanceof Uint8Array ? b : new Uint8Array(b);
  let s = "";
  for (const x of u) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

type Desc = { type: string; id: string; transports?: string[] };
type CreationJson = { publicKey: Record<string, unknown> & { challenge: string; user: { id: string; name: string; displayName: string }; excludeCredentials?: Desc[] } };
type RequestJson = { publicKey: Record<string, unknown> & { challenge: string; allowCredentials?: Desc[] } };

const desc = (d: Desc) => ({ ...d, id: b64urlToBytes(d.id) }) as PublicKeyCredentialDescriptor;

/** Server JSON (base64url) → options for `navigator.credentials.create`. */
export function creationOptions(o: CreationJson): PublicKeyCredentialCreationOptions {
  const p = o.publicKey;
  return { ...p, challenge: b64urlToBytes(p.challenge), user: { ...p.user, id: b64urlToBytes(p.user.id) }, excludeCredentials: (p.excludeCredentials ?? []).map(desc) } as unknown as PublicKeyCredentialCreationOptions;
}

/** Server JSON (base64url) → options for `navigator.credentials.get`. */
export function requestOptions(o: RequestJson): PublicKeyCredentialRequestOptions {
  const p = o.publicKey;
  return { ...p, challenge: b64urlToBytes(p.challenge), allowCredentials: (p.allowCredentials ?? []).map(desc) } as unknown as PublicKeyCredentialRequestOptions;
}

/** A created/asserted credential → JSON webauthn-rs accepts. */
export function credentialJson(c: PublicKeyCredential): Record<string, unknown> {
  const r = c.response as AuthenticatorAttestationResponse & AuthenticatorAssertionResponse;
  const response: Record<string, unknown> = { clientDataJSON: bytesToB64url(r.clientDataJSON) };
  if ("attestationObject" in r && r.attestationObject) response.attestationObject = bytesToB64url(r.attestationObject);
  if ("authenticatorData" in r && r.authenticatorData) {
    response.authenticatorData = bytesToB64url(r.authenticatorData);
    response.signature = bytesToB64url(r.signature);
    response.userHandle = r.userHandle ? bytesToB64url(r.userHandle) : null;
  }
  return { id: c.id, rawId: bytesToB64url(c.rawId), type: c.type, response, extensions: {} };
}

export const passkeysSupported = () => typeof window !== "undefined" && !!window.PublicKeyCredential && !!navigator.credentials;

async function getAssertion(start: { challenge_id: string; options: RequestJson }, extra: Partial<CredentialRequestOptions> = {}) {
  const c = (await navigator.credentials.get({ ...extra, publicKey: requestOptions(start.options) })) as PublicKeyCredential | null;
  if (!c) throw new Error("cancelled");
  return { challenge_id: start.challenge_id, credential: credentialJson(c) };
}

export async function enrolPasskey(password: string, code: string, name: string) {
  const s = await api<{ challenge_id: string; options: CreationJson }>("/auth/passkeys/register/start", { method: "POST", body: { password, code } });
  const c = (await navigator.credentials.create({ publicKey: creationOptions(s.options) })) as PublicKeyCredential | null;
  if (!c) throw new Error("cancelled");
  return api<{ id: string }>("/auth/passkeys/register/finish", { method: "POST", body: { challenge_id: s.challenge_id, name, credential: credentialJson(c) } });
}

export async function passkeyLogin<T>(email: string): Promise<T> {
  const s = await api<{ challenge_id: string; options: RequestJson }>("/auth/passkeys/login/start", { method: "POST", body: { email } });
  return api<T>("/auth/passkeys/login/finish", { method: "POST", body: await getAssertion(s) });
}

/** Second sign-in step with a passkey, for a pending password-login challenge. */
export async function passkeyMfa<T>(challenge: string): Promise<T> {
  const s = await api<{ challenge_id: string; options: RequestJson }>("/auth/mfa/passkey/start", { method: "POST", body: { challenge } });
  return api<T>("/auth/mfa/passkey/finish", { method: "POST", body: { challenge, ...(await getAssertion(s)) } });
}

export async function passkeyReauth() {
  const s = await api<{ challenge_id: string; options: RequestJson }>("/auth/passkeys/reauth/start", { method: "POST" });
  await api("/auth/passkeys/reauth/finish", { method: "POST", body: await getAssertion(s) });
}

/** Browser can offer saved passkeys in the e-mail field's autofill. */
export async function conditionalMediationAvailable(): Promise<boolean> {
  if (!passkeysSupported()) return false;
  const f = (window.PublicKeyCredential as unknown as { isConditionalMediationAvailable?: () => Promise<boolean> }).isConditionalMediationAvailable;
  try { return f ? await f.call(window.PublicKeyCredential) : false; } catch { return false; }
}

/**
 * Sign in without typing an e-mail: the authenticator picks a discoverable
 * passkey and returns its user handle. `conditional` runs it as autofill
 * (pass an AbortSignal so the form can cancel it).
 */
export async function passkeyDiscoverLogin<T>(opts: { conditional?: boolean; signal?: AbortSignal } = {}): Promise<T> {
  const s = await api<{ challenge_id: string; options: RequestJson }>("/auth/passkeys/discover/start", { method: "POST" });
  const extra: Partial<CredentialRequestOptions> = { signal: opts.signal };
  if (opts.conditional) extra.mediation = "conditional" as CredentialMediationRequirement;
  return api<T>("/auth/passkeys/discover/finish", { method: "POST", body: await getAssertion(s, extra) });
}
