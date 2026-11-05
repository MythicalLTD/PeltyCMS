//! Hosting backends: provisioning, domains and registrant verification, SSL probing and the cluster coordinator.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod provision;
pub mod domains;
pub mod registrant_verify;
pub mod ssl_probe;
pub mod cluster;
