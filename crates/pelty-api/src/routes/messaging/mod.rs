//! Messaging HTTP routes: email templates and log, SMTP, campaigns, link tracking, webhook endpoints and inbound webhooks.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod email_templates;
pub mod email_log;
pub mod smtp;
pub mod campaigns;
pub mod link_tracking;
pub mod webhook_endpoints;
pub mod webhooks;
