//! SSL reseller provisioning addon.
//! The whole module lives in [`ssl`]; this file only exports it to the host.
pub mod ssl;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(ssl::SslReseller);
