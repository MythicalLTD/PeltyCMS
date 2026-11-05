//! Commerce HTTP routes: catalog, bundles, order forms, the public store and data feeds.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod catalog;
pub mod bundles;
pub mod order_forms;
pub mod store;
pub mod data_feeds;
