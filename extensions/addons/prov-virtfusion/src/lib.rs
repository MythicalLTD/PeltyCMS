//! VirtFusion provisioning addon.
//! The whole module lives in [`virtfusion`]; this file only exports it to the host.
pub mod virtfusion;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(virtfusion::VirtFusion);
