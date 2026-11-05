//! Paddle gateway addon: Paddle Billing transactions and webhooks.
//! The whole gateway lives in [`paddle`]; this file only exports it to the host.
pub mod paddle;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(paddle::descriptor, paddle::factory);
