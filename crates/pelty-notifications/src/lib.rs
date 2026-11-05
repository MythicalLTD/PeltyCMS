//! Outbound email: a tiny template renderer, the `Mailer` trait and its
//! SMTP / log / in-memory implementations. Delivery runs through the job
//! queue (see `pelty-api::worker`), never inline in a request.
pub mod mailer;
pub mod template;

pub use mailer::{Email, LogMailer, Mailer, MemoryMailer, SmtpMailer, SmtpSecurity};
pub use template::{render, render_html, TemplateDef, TEMPLATES};
