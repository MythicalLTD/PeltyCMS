//! Authorize.net Accept Hosted gateway addon.
pub mod authorizenet;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(authorizenet::descriptor, authorizenet::factory);
