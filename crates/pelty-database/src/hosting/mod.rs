//! Service repositories: active services, upgrades/downgrades, provisioning state, server import and SSL checks.
//!
//! Re-exported at the crate root. Covered by the services/provisioning integration tests.

pub mod module_ext;
pub mod provisioning;
pub mod server_import;
pub mod service_changes;
pub mod services;
pub mod services_service;
pub mod ssl_checks;
pub use services_service::HostingService;
