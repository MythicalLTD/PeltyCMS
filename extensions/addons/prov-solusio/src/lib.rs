//! SolusIO / SolusVM 2 provisioning addon.
pub mod solusio;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(solusio::SolusIo);
