//! ISPConfig provisioning addon.
//! The whole module lives in [`ispconfig`]; this file only exports it to the host.
//! The whole module lives in [`ispconfig`]; this file only exports it to the host.
pub mod ispconfig;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(ispconfig::IspConfig);
