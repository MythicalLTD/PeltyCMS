//! Virtualizor provisioning addon.
//! The whole module lives in [`virtualizor`]; this file only exports it to the host.
pub mod virtualizor;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(virtualizor::Virtualizor);
