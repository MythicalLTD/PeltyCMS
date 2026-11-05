//! eNom registrar addon.
//! The whole registrar lives in [`enom`]; this file only exports it to the host.
pub mod enom;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<enom::Enom> = std::sync::LazyLock::new(|| enom::Enom::new(pelty_provisioning::registrars::default_transport()));
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(*M, pelty_addon_sdk::serde_json::json!(enom::settings()));
