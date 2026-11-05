//! Crypto (Coinbase Commerce-style) gateway addon: Hosted crypto charges with signed webhooks.
//! The whole gateway lives in [`crypto`]; this file only exports it to the host.
pub mod crypto;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(crypto::descriptor, crypto::factory);
