//! Stripe gateway addon (Checkout Sessions, signed webhooks, refunds, saved cards).
//! The whole gateway lives in [`stripe`]; this file only exports it to the host.
pub mod stripe;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(stripe::descriptor, stripe::factory);
