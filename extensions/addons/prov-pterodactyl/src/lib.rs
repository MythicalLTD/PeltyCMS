//! Pterodactyl provisioning addon.
//! The whole module lives in [`pterodactyl`]; this file only exports it to the host.
pub mod pterodactyl;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(pterodactyl::PTERODACTYL);
