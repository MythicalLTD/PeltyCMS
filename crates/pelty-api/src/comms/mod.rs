//! Communication: outgoing mail, inbound mailbox/pipe, notifications, event mail, outgoing webhooks, the chat bus and support automation.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod mail;
pub mod mailbox;
pub mod pipe;
pub mod notify;
pub mod event_mail;
pub mod webhooks_out;
pub mod chat_bus;
pub mod support_ext;
