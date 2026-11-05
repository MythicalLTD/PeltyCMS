//! Proxmox VE provisioning addon.
//! The whole module lives in [`proxmox`]; this file only exports it to the host.
pub mod proxmox;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(proxmox::Proxmox);
