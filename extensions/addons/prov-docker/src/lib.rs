//! Docker provisioning addon.
//! The whole module lives in [`docker`]; this file only exports it to the host.
pub mod docker;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(docker::DockerHost);
