//! DirectAdmin provisioning addon.
//! The whole module lives in [`directadmin`]; this file only exports it to the host.
//! The whole module lives in [`directadmin`]; this file only exports it to the host.
pub mod directadmin;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(directadmin::DirectAdmin);
