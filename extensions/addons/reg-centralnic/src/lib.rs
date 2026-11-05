//! CentralNic Reseller registrar addon.
//! The whole registrar lives in [`centralnic`]; this file only exports it to the host.
pub mod centralnic;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<centralnic::CentralNic> = std::sync::LazyLock::new(|| {
    centralnic::CentralNic::new(pelty_provisioning::registrars::default_transport())
});
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(
    *M,
    pelty_addon_sdk::serde_json::json!(centralnic::settings())
);
