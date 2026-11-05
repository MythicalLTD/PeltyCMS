//! Hetzner Cloud provisioning addon.
//! The whole module lives in [`hetzner`]; this file only exports it to the host.
//! The whole module lives in [`hetzner`]; this file only exports it to the host.
pub mod hetzner;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(hetzner::HetznerCloud);
