//! Settings HTTP routes: settings, general settings, automation, site/branding, i18n and the setup wizard.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod automation;
pub mod general_settings;
pub mod geo;
pub mod i18n;
pub mod license;
pub mod settings;
pub mod setup_wizard;
pub mod site;
