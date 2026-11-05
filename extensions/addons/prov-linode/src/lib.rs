//! Linode (Akamai) provisioning addon.
//! The whole module lives in [`linode`]; this file only exports it to the host.
//! The whole module lives in [`linode`]; this file only exports it to the host.
pub mod linode;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(linode::Linode);
