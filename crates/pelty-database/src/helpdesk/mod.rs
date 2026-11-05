//! Support repositories: tickets and their statuses/SLA/macros/attachments, live chat, satisfaction, network status, announcements, downloads and projects.
//!
//! Re-exported at the crate root. Covered by the support/tickets integration tests.

pub mod downloads;
pub mod live_chat;
pub(crate) mod live_chat_ext;
pub mod network_status;
pub mod noticeboard;
pub mod projects;
pub mod satisfaction;
pub mod support;
pub mod support_ext;
pub mod ticket_attachments;
pub mod ticket_macros;
pub mod ticket_sla;
pub mod ticket_statuses;
pub mod tickets;
pub mod tickets_service;
pub use tickets_service::TicketService;
