//! cPanel / WHM provisioning addon.
//! The whole module lives in [`cpanel`]; this file only exports it to the host.
//! The whole module lives in [`cpanel`]; this file only exports it to the host.
pub mod cpanel;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(cpanel::Cpanel);
