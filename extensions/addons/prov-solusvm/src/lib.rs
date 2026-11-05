//! SolusVM provisioning addon.
//! The whole module lives in [`solusvm`]; this file only exports it to the host.
pub mod solusvm;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(solusvm::SolusVm);
