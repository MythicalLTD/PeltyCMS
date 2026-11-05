//! HestiaCP provisioning addon.
//! The whole module lives in [`hestia`]; this file only exports it to the host.
//! The whole module lives in [`hestia`]; this file only exports it to the host.
pub mod hestia;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(hestia::Hestia);
