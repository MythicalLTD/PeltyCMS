//! Cloudflare DNS host addon (zone + DNS records via RegistrarModule).
pub mod cloudflare;

#[cfg(all(target_family = "wasm", feature = "export"))]
static M: std::sync::LazyLock<cloudflare::Cloudflare> = std::sync::LazyLock::new(|| {
    cloudflare::Cloudflare::new(pelty_provisioning::registrars::default_transport())
});
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::registrar_addon!(
    *M,
    pelty_addon_sdk::serde_json::json!(cloudflare::settings())
);
