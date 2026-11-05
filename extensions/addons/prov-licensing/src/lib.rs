//! Licensing provisioning addon.
//! The whole module lives in [`licensing`]; this file only exports it to the host.
pub mod licensing;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(licensing::Licensing);
