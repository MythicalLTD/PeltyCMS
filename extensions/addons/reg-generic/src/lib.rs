//! Generic HTTP registrar registrar addon.
//! The whole registrar lives in [`generic`]; this file only exports it to the host.
pub mod generic;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<generic::Generic> = std::sync::LazyLock::new(|| generic::Generic::new(pelty_provisioning::registrars::default_transport()));
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(*M, pelty_addon_sdk::serde_json::json!(generic::settings()));
