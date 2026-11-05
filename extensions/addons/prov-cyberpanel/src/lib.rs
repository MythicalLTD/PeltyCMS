//! CyberPanel provisioning addon.
//! The whole module lives in [`cyberpanel`]; this file only exports it to the host.
//! The whole module lives in [`cyberpanel`]; this file only exports it to the host.
pub mod cyberpanel;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(cyberpanel::CyberPanel);
