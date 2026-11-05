//! Convoy provisioning addon.
//! The whole module lives in [`convoy`]; this file only exports it to the host.
pub mod convoy;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(convoy::Convoy);
