//! Domain repositories: registered domains, TLD pricing, defaults and registrant verification.
//!
//! Re-exported at the crate root. Covered by the domains_* integration tests.

pub mod domains;
pub mod domain_defaults;
pub mod domain_pricing;
pub mod registrant_verify;
pub mod domains_service;
pub use domains_service::DomainService;
