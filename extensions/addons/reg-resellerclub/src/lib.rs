//! ResellerClub registrar addon.
//! The whole registrar lives in [`resellerclub`]; this file only exports it to the host.
pub mod resellerclub;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<resellerclub::ResellerClub> = std::sync::LazyLock::new(|| {
    resellerclub::ResellerClub::new(pelty_provisioning::registrars::default_transport())
});
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(
    *M,
    pelty_addon_sdk::serde_json::json!(resellerclub::settings())
);
