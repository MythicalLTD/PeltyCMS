//! OpenSRS registrar addon.
//! The whole registrar lives in [`opensrs`]; this file only exports it to the host.
pub mod opensrs;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<opensrs::OpenSrs> = std::sync::LazyLock::new(|| opensrs::OpenSrs::new(pelty_provisioning::registrars::default_transport()));
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(*M, pelty_addon_sdk::serde_json::json!(opensrs::settings()));
