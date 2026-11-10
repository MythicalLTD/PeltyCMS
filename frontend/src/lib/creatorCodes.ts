/** Creator codes (module `creator_codes`): mirrors the server's rules. */
export interface CreatorCode { id: string; affiliate_id: string; code: string; discount_percent: string; enabled: boolean; created_at: string }
export interface CreatorCodeRow { id: string; affiliate_id: string; code: string; discount_percent: string; enabled: boolean; email: string; affiliate_status: string; orders: number }

/** Trimmed lower-case code, or null when it can't be a valid code (3–24 of a-z 0-9 _ -). */
export function normalizeCreatorCode(raw: string): string | null {
  const c = raw.trim().toLowerCase();
  return /^[a-z0-9_-]{3,24}$/.test(c) ? c : null;
}

/** Admin discount input: 0–50 with up to two decimals. */
export function validCreatorDiscount(v: string): boolean {
  return /^\d{1,2}(\.\d{1,2})?$/.test(v.trim()) && Number(v) >= 0 && Number(v) <= 50;
}
