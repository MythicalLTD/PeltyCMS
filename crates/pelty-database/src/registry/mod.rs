//! Domain repositories: registered domains, TLD pricing, defaults and registrant verification.
//!
//! Re-exported at the crate root. Covered by the domains_* integration tests.

pub mod domain_defaults;
pub mod domain_pricing;
pub mod domains;
pub mod domains_service;
pub mod registrant_verify;
pub use domains_service::DomainService;
