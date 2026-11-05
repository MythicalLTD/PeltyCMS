//! Support HTTP routes: tickets (statuses, SLA, macros, attachments, AI), live chat, network status, downloads, knowledge content and projects.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod support;
pub mod support_ext;
pub mod tickets;
pub mod ticket_statuses;
pub mod ticket_sla;
pub mod ticket_macros;
pub mod ticket_attachments;
pub mod ticket_ai;
pub mod live_chat;
pub mod network_status;
pub mod downloads;
pub mod content;
pub mod projects;
