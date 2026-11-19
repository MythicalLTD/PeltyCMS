/**
 * PeltyCMS Premium (optional `premium` module): status from
 * `/admin/premium`. Verification is soft: when MyMythical cannot be reached
 * Premium keeps working and `warning` is set.
 */
import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { useModules } from "@/lib/modules";

export interface PremiumEntitlements { white_label: boolean; priority_support: boolean; higher_limits: boolean }
export type PremiumWarning = "check_failed" | "expired";

export interface PremiumStatus {
  enabled: boolean;
  install_id: string;
  linked: boolean;
  team_name: string;
  active: boolean;
  warning: PremiumWarning | null;
  entitlements: PremiumEntitlements;
  plan: string;
  reason: string | null;
  seats: number;
  seats_used: number;
  expires_at: string | null;
  checked_at: string | null;
  last_ok_at: string | null;
  error: string | null;
  buy_url: string;
  support_url: string | null;
  prefs: { hide_powered_by: boolean; source_url: string };
}

export const PREMIUM_KEY = ["admin", "premium"] as const;
export const ENTITLEMENT_KEYS = ["white_label", "priority_support", "higher_limits"] as const;

/** Premium status; disabled (no request) while the module is off. */
export function usePremium() {
  const { enabled } = useModules();
  const on = enabled.has("premium");
  return useQuery({ queryKey: PREMIUM_KEY, queryFn: () => api<PremiumStatus>("/admin/premium"), enabled: on, retry: false, staleTime: 300_000 });
}

/** Only https links from MyMythical are rendered as links. */
export const safeHttps = (u: string | null | undefined) => (u && u.startsWith("https://") ? u : null);
