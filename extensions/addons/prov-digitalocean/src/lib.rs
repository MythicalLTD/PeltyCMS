//! DigitalOcean provisioning addon.
//! The whole module lives in [`digitalocean`]; this file only exports it to the host.
//! The whole module lives in [`digitalocean`]; this file only exports it to the host.
pub mod digitalocean;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(digitalocean::DigitalOcean);
