import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { passkeysSupported } from "@/lib/passkeys";

export interface GeneralSettings {
  general: { company_name: string; email: string; domain: string; logo_url: string; pay_to_text: string; records_per_page: number };
  system_urls: { system_url: string; terms_url: string; privacy_url: string; support_url: string };
  maintenance: { enabled: boolean; message: string; redirect_url: string };
  localisation: { default_language: string; allow_language_choice: boolean; date_format: string; client_date_format: string; timezone: string; default_country: string };
  ordering: { default_order_form: string; tos_required: boolean; allow_domain_only: boolean; show_cart_after_add: boolean; grace_days: number; enable_promo_field: boolean };
  support: { attachment_max_mb: number; attachments_per_message: number; live_chat?: boolean };
}
export interface CustomCode { css: string; head_js: string; footer_js: string }
export interface PublicGeneral {
  company_name: string; logo_url: string;
  maintenance: GeneralSettings["maintenance"]; localisation: GeneralSettings["localisation"]; ordering: GeneralSettings["ordering"]; support?: GeneralSettings["support"];
  urls: { terms: string; privacy: string; support: string };
  custom_code: CustomCode;
  /** What this server can actually do (absent on older servers = assume yes). */
  features?: { passkeys: boolean; totp: boolean };
}

/** Passkeys work only with a valid public URL; false hides passkey UI. */
export function usePasskeysAvailable(): boolean {
  const g = usePublicGeneral();
  return passkeysSupported() && (g.data?.features?.passkeys ?? false);
}

export function usePublicGeneral() {
  return useQuery({ queryKey: ["public", "general"], queryFn: () => api<PublicGeneral>("/public/general"), staleTime: 60_000, retry: false });
}

export const httpsOrEmpty = (u: string) => u === "" || /^https:\/\/\S+$/.test(u);

/**
 * Inject admin-provided CSS/JS into the client area once per value.
 * Elements are tagged so a changed value replaces the previous one.
 */
export function applyCustomCode(doc: Document, c: CustomCode | undefined) {
  const put = (id: string, tag: "style" | "script", text: string, parent: HTMLElement) => {
    const old = doc.getElementById(id);
    if (old && old.dataset.v === text) return;
    old?.remove();
    if (!text.trim()) return;
    const el = doc.createElement(tag);
    el.id = id;
    el.dataset.v = text;
    el.textContent = text;
    parent.appendChild(el);
  };
  put("pelty-custom-css", "style", c?.css ?? "", doc.head);
  put("pelty-custom-head-js", "script", c?.head_js ?? "", doc.head);
  put("pelty-custom-footer-js", "script", c?.footer_js ?? "", doc.body);
}
