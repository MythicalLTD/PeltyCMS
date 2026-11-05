//! Addon runtime host: loading WASM addons, their events, filters, functions, secrets, trust, health, upgrades and plugin hooks.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod addon_events;
pub mod addon_ext;
pub mod addon_filters;
pub mod addon_functions;
pub mod addon_health;
pub mod addon_power;
pub mod addon_secrets;
pub mod addon_trust;
pub mod addon_upgrade;
pub mod addons;
pub mod plugin_hooks;
pub mod wasm_addons;
