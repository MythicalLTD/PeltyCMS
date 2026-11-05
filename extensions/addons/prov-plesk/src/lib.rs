//! Plesk provisioning addon.
//! The whole module lives in [`plesk`]; this file only exports it to the host.
//! The whole module lives in [`plesk`]; this file only exports it to the host.
pub mod plesk;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(plesk::Plesk);
