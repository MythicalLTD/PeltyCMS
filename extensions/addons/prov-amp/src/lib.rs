//! AMP provisioning addon.
//! The whole module lives in [`amp`]; this file only exports it to the host.
pub mod amp;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(amp::Amp);
