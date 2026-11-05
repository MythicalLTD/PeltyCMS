export interface PaymentMethod {
  id: string;
  customer_id: string;
  gateway: string;
  brand: string;
  last4: string;
  exp_month: number;
  exp_year: number;
  is_default: boolean;
  created_at: string;
}
export interface AutopayAttempt {
  id: string;
  invoice_id: string;
  payment_id: string | null;
  attempt: number;
  status: "processing" | "succeeded" | "pending" | "declined" | "error";
  error: string | null;
  next_retry: string | null;
  created_at: string;
}
export interface MethodsOverview {
  methods: PaymentMethod[];
  auto_pay: boolean;
  gateways: string[];
  attempts: AutopayAttempt[];
}
export interface AutopaySettings {
  enabled: boolean;
  retry_days: number[];
}

/** True once the card's expiry month has fully passed. */
export function isExpired(m: Pick<PaymentMethod, "exp_month" | "exp_year">, now = new Date()): boolean {
  const y = now.getUTCFullYear();
  const mo = now.getUTCMonth() + 1;
  return m.exp_year < y || (m.exp_year === y && m.exp_month < mo);
}

/** "12/2031" */
export function expiry(m: Pick<PaymentMethod, "exp_month" | "exp_year">): string {
  return `${String(m.exp_month).padStart(2, "0")}/${m.exp_year}`;
}

/** Client-side token check mirroring the server (Stripe `pm_…`). */
export function tokenError(gateway: string, token: string): string | null {
  const v = token.trim();
  if (!v || v.length > 255) return "error.invalid_token";
  if (gateway === "stripe" && !/^pm_[A-Za-z0-9_]+$/.test(v)) return "error.invalid_token";
  return null;
}
