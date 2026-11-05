//! OpenStack provisioning addon.
//! The whole module lives in [`openstack`]; this file only exports it to the host.
pub mod openstack;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::provisioning_addon!(openstack::OpenStack);
