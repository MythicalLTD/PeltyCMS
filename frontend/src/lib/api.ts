/** Same-origin API client. Cookies carry the session; mutating requests send the CSRF token. */
import { isUnreachableError, noteApiReachable, noteApiUnreachable } from "./api-health";

export { isUnreachableError } from "./api-health";

/** Fired when the server asks for step-up re-authentication. */
export const REAUTH_EVENT = "pelty:reauth";

export class ApiError extends Error {
  constructor(public status: number, public code: string, message: string, public requestId?: string) {
    super(message);
  }
}

let csrfToken: string | null = null;
let captchaToken: string | null = null;
/** One-shot captcha answer sent as `x-captcha-token` with the next mutating request. */
export function setCaptchaToken(t: string | null) {
  captchaToken = t;
}
export function setCsrfToken(t: string | null) {
  csrfToken = t;
}

export async function api<T>(path: string, init: { method?: string; body?: unknown; raw?: Blob; captcha?: boolean; headers?: Record<string, string> } = {}): Promise<T> {
  const method = init.method ?? "GET";
  const headers: Record<string, string> = { accept: "application/json", ...init.headers };
  if (init.raw) headers["content-type"] = init.raw.type || "application/octet-stream";
  else if (init.body !== undefined) headers["content-type"] = "application/json";
  if (method !== "GET" && csrfToken) headers["x-csrf-token"] = csrfToken;
  const usedCaptcha = (method !== "GET" || init.captcha === true) && captchaToken !== null;
  if (usedCaptcha) {
    headers["x-captcha-token"] = captchaToken as string;
    captchaToken = null;
  }
  let res: Response;
  try {
    res = await fetch(`/api/v1${path}`, {
      method,
      headers,
      credentials: "same-origin",
      body: init.raw ?? (init.body === undefined ? undefined : JSON.stringify(init.body)),
    });
  } catch (e) {
    noteApiUnreachable();
    throw e;
  }
  if (usedCaptcha && typeof window !== "undefined") window.dispatchEvent(new Event("pelty:captcha-reset"));
  if (res.status === 204) {
    noteApiReachable();
    return undefined as T;
  }
  const data: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const err = (data as { error?: { code?: string; message?: string; request_id?: string } } | null)?.error;
    // Vite proxy ECONNREFUSED often yields a non-JSON 500; treat like a gateway error.
    const code = err?.code ?? (res.status >= 500 && data === null ? "error.unavailable" : "error.internal");
    const error = new ApiError(res.status, code, err?.message ?? res.statusText, err?.request_id);
    if (isUnreachableError(error)) noteApiUnreachable();
    if (code === "error.reauth_required" && typeof window !== "undefined") window.dispatchEvent(new Event(REAUTH_EVENT));
    throw error;
  }
  noteApiReachable();
  return data as T;
}

export interface Me {
  id: string;
  email: string;
  display_name: string;
  locale: string | null;
  email_verified: boolean;
  mfa_enabled: boolean;
  /** Passkey opted in as a standalone second factor. */
  passkey_2fa?: boolean;
  avatar_version: number | null;
  roles: string[];
  permissions: string[];
  csrf_token: string;
}

export interface Paged<T> {
  items: T[];
  page: number;
  per_page: number;
  total: number;
}
