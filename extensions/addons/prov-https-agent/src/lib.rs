//! HTTPS agent provisioning addon.
//! The whole module lives in [`agent`]; this file only exports it to the host.
pub mod agent;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(agent::HttpsAgent);
