//! Manual bank transfer gateway addon: Bank transfer instructions.
//! The whole gateway lives in [`manual`]; this file only exports it to the host.
pub mod manual;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(manual::descriptor, manual::factory);
