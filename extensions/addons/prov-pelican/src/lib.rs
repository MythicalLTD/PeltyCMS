//! Pelican (Wings) provisioning addon.
//! Pelican speaks the Pterodactyl application API; the code is shared with the `prov-pterodactyl` addon crate.
#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(pelty_addon_provisioning_pterodactyl::pterodactyl::PELICAN);
