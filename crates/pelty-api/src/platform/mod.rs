//! Platform services: the background worker and cron, backups, updates, change log, approvals, CSV/extract helpers, developer docs, the WHMCS-compatible API, translations and static files.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod approval;
pub mod backup;
pub mod changes;
pub mod cron;
pub mod csv;
pub mod dev_docs;
pub mod extract;
pub mod static_files;
pub mod translations;
pub mod updates;
pub mod whmcs_api;
pub mod worker;
