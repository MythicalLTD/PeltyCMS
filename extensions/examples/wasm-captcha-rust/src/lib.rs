//! Example PeltyCMS captcha addon, compiled to a WASM component (wasm32-wasip2).
//! Accepts tokens of the form `ok-<secret>`; `loop` spins forever (sandbox demo).
// No handwritten unsafe here; the ABI glue emitted by wit-bindgen uses `unsafe` internally.
wit_bindgen::generate!({ path: "../../../crates/pelty-wasm/wit/captcha.wit", world: "captcha-addon" });

use exports::pelty::addon::captcha::{Guest, VerifyRequest, VerifyResult};
use pelty::addon::host::{log, Level};

struct Addon;

impl Guest for Addon {
    fn verify(req: VerifyRequest) -> Result<VerifyResult, String> {
        if req.secret.is_empty() {
            return Err("secret not configured".into());
        }
        if req.token == "loop" {
            let mut x: u64 = 0;
            loop {
                x = std::hint::black_box(x.wrapping_add(1));
            }
        }
        let ok = req.token.strip_prefix("ok-") == Some(req.secret.as_str());
        log(Level::Info, &format!("verify ip={:?} ok={ok}", req.remote_ip));
        Ok(VerifyResult { success: ok, error_codes: if ok { vec![] } else { vec!["invalid-input-response".into()] } })
    }
}

export!(Addon);
