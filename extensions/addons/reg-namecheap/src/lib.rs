//! Namecheap registrar addon.
//! The whole registrar lives in [`namecheap`]; this file only exports it to the host.
pub mod namecheap;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<namecheap::Namecheap> = std::sync::LazyLock::new(|| {
    namecheap::Namecheap::new(pelty_provisioning::registrars::default_transport())
});
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(
    *M,
    pelty_addon_sdk::serde_json::json!(namecheap::settings())
);
