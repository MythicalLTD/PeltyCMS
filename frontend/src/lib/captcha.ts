/** Browser widget a captcha addon declares in its descriptor. */
export interface CaptchaWidget { script: string; global: string; mode: "widget" | "score" }
export interface CaptchaConfig { enabled: boolean; engine?: string; mode?: string; site_key?: string; forms: string[]; widget?: CaptchaWidget | null }

/** Fallback for addons that predate `widget` in their descriptor. */
const KNOWN: Record<string, CaptchaWidget> = {
  turnstile: { script: "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit", global: "turnstile", mode: "widget" },
  hcaptcha: { script: "https://js.hcaptcha.com/1/api.js?render=explicit", global: "hcaptcha", mode: "widget" },
  recaptcha_v2: { script: "https://www.google.com/recaptcha/api.js?render=explicit", global: "grecaptcha", mode: "widget" },
  recaptcha_v3: { script: "https://www.google.com/recaptcha/api.js?render={site_key}", global: "grecaptcha", mode: "score" },
};

/** Widget for a config: the addon's own declaration (https only) or the known fallback. */
export function widgetFor(cfg: CaptchaConfig | undefined): CaptchaWidget | null {
  if (!cfg?.engine || cfg.engine === "internal") return null;
  const w = cfg.widget && cfg.widget.script?.startsWith("https://") && /^[A-Za-z_$][\w$]*$/.test(cfg.widget.global ?? "") ? cfg.widget : KNOWN[cfg.engine];
  if (!w) return null;
  return { ...w, script: w.script.replace("{site_key}", encodeURIComponent(cfg.site_key ?? "")) };
}
