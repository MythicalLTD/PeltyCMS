//! Mollie gateway addon: Payments API, status-fetch webhooks, refunds.
//! The whole gateway lives in [`mollie`]; this file only exports it to the host.
pub mod mollie;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(mollie::descriptor, mollie::factory);
