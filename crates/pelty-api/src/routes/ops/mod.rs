//! Admin operations HTTP routes: dashboard/admin, admin tools, reports, exports, CSV import, bulk and mass actions.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod admin;
pub mod admin_ops;
pub mod admin_tools;
pub mod reports;
pub mod exports;
pub mod csv_import;
pub mod bulk;
pub mod mass;
