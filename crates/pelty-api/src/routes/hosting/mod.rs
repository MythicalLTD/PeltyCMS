//! Hosting HTTP routes: provisioning, server modules, service upgrades/downgrades and SSL.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod provisioning;
pub mod modules_ext;
pub mod service_changes;
pub mod ssl;
