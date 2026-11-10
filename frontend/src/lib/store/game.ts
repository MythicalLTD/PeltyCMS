import type { Price } from "./storefront";

/** cfx.re join codes are short lowercase base36 ids (mirrors the server check). */
export const JOIN_CODE = /^[a-z0-9]{4,12}$/;

/** Accepts a bare code or a pasted `cfx.re/join/<code>` link; returns the bare code. */
export function parseJoinCode(input: string): string {
  const s = input.trim().toLowerCase();
  const m = /cfx\.re\/join\/([a-z0-9]+)/.exec(s);
  return m ? m[1]! : s;
}

/** Link that opens the server in FiveM, or null when no valid code is set. */
export function joinUrl(code: string | undefined | null): string | null {
  return code && JOIN_CODE.test(code) ? `https://cfx.re/join/${code}` : null;
}

/** The cycle a package can be added with in one click: it has exactly one price
 *  in this currency and nothing to configure. Otherwise the buyer picks on the
 *  product page. */
export function quickAddCycle(p: { prices: Price[]; options: unknown[] }, currency: string): string | null {
  const cycles = p.prices.filter((x) => x.currency === currency);
  return cycles.length === 1 && p.options.length === 0 ? cycles[0]!.cycle : null;
}
