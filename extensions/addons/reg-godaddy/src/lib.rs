//! GoDaddy registrar addon.
//! The whole registrar lives in [`godaddy`]; this file only exports it to the host.
pub mod godaddy;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<godaddy::GoDaddy> = std::sync::LazyLock::new(|| {
    godaddy::GoDaddy::new(pelty_provisioning::registrars::default_transport())
});
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(*M, pelty_addon_sdk::serde_json::json!(godaddy::settings()));
