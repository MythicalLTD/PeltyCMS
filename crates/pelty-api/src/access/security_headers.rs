//! Baseline security headers on every response. Handlers that already set a
//! stricter value (e.g. file downloads with `sandbox` CSP) keep theirs.
//!
//! The CSP fits the bundled SPA and the native plugin UI renderer (declarative
//! JSON rendered by React, no plugin scripts): scripts only from self, inline
//! styles allowed for component libraries, images from self/data/https (plugin
//! and theme logos), no framing (frame-ancestors 'none'). `frame-src 'self' blob:`
//! covers sandboxed addon frames and the admin email HTML preview iframe. Addon
//! frames (`/api/v1/addons/<id>/assets/`, which set
//! their own CSP and `X-Frame-Options: SAMEORIGIN`).
use crate::AppState;
use axum::extract::{Request, State};
use axum::http::{HeaderName, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;

// `blob:` on frame-src: admin email-template HTML preview (object URL iframe).
pub const CSP: &str = "default-src 'self'; script-src 'self' https://challenges.cloudflare.com https://js.hcaptcha.com https://*.hcaptcha.com https://www.google.com/recaptcha/ https://www.gstatic.com/recaptcha/; frame-src 'self' blob: https://challenges.cloudflare.com https://js.hcaptcha.com https://*.hcaptcha.com https://www.google.com/recaptcha/ https://www.gstatic.com/recaptcha/; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob: https:; font-src 'self' data:; connect-src 'self' https://*.hcaptcha.com; object-src 'none'; base-uri 'self'; form-action 'self' https:; frame-ancestors 'none'";
pub const PERMISSIONS: &str =
    "camera=(), microphone=(), geolocation=(), payment=(self), usb=(), interest-cohort=()";
pub const HSTS: &str = "max-age=31536000; includeSubDomains";

pub async fn apply(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let mut set = |k: &'static str, v: &'static str| {
        let name = HeaderName::from_static(k);
        if !h.contains_key(&name) {
            h.insert(name, HeaderValue::from_static(v));
        }
    };
    set("content-security-policy", CSP);
    set("x-frame-options", "DENY");
    set("referrer-policy", "strict-origin-when-cross-origin");
    set("permissions-policy", PERMISSIONS);
    set("x-content-type-options", "nosniff");
    set("cross-origin-opener-policy", "same-origin");
    if s.cfg.cookie_secure {
        set("strict-transport-security", HSTS);
    }
    res
}
