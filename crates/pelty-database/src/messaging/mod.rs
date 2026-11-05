//! Messaging repositories: email templates and log, event mail, campaigns, link tracking and outgoing webhooks.
//!
//! Re-exported at the crate root. Covered by the campaigns/email/webhook integration tests.

pub mod campaigns;
pub mod email_log;
pub mod email_templates;
pub mod event_mail;
pub mod link_tracking;
pub mod plugin_webhooks;
pub mod webhook_endpoints;
