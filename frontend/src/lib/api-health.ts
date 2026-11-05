/**
 * When the API is unreachable (dev backend restarting, proxy refused, …),
 * stop hammering it: mark TanStack Query offline and probe with backoff.
 */
import { onlineManager } from "@tanstack/react-query";

const PROBE = "/healthz";
const MAX_DELAY = 30_000;
const BASE_DELAY = 2_000;

let probing = false;
let failures = 0;
let timer: ReturnType<typeof setTimeout> | undefined;
let fetchImpl: typeof fetch = (...args) => globalThis.fetch(...args);

/** Test seam. */
export function setHealthFetch(fn: typeof fetch) {
  fetchImpl = fn;
}

export function resetApiHealth() {
  failures = 0;
  probing = false;
  clearTimeout(timer);
  timer = undefined;
  if (!onlineManager.isOnline()) onlineManager.setOnline(true);
}

/** True for transport / gateway failures, not application 4xx/5xx from Pelty. */
export function isUnreachableError(e: unknown): boolean {
  if (e && typeof e === "object" && "status" in e && "code" in e) {
    const status = (e as { status: number }).status;
    const code = (e as { code: string }).code;
    if (status === 502 || status === 503 || status === 504) return true;
    if (status === 500 && code === "error.unavailable") return true;
  }
  // fetch() rejection: TypeError in browsers, often "Failed to fetch"
  return e instanceof TypeError;
}

function scheduleProbe() {
  if (probing) return;
  probing = true;
  const delay = Math.min(MAX_DELAY, BASE_DELAY * 2 ** Math.min(failures - 1, 4));
  clearTimeout(timer);
  timer = setTimeout(() => {
    void fetchImpl(PROBE, { credentials: "same-origin", cache: "no-store" })
      .then((res) => {
        if (res.ok) {
          resetApiHealth();
          return;
        }
        failures++;
        probing = false;
        scheduleProbe();
      })
      .catch(() => {
        failures++;
        probing = false;
        scheduleProbe();
      });
  }, delay);
}

/** Call after a failed request that looks like the API is down. */
export function noteApiUnreachable() {
  failures++;
  if (onlineManager.isOnline()) onlineManager.setOnline(false);
  scheduleProbe();
}

/** Call after any successful API response. */
export function noteApiReachable() {
  if (failures === 0 && onlineManager.isOnline()) return;
  resetApiHealth();
}
