//! FeatherPanel provisioning addon.
//! The whole module lives in [`featherpanel`]; this file only exports it to the host.
pub mod featherpanel;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(featherpanel::FeatherPanel);
