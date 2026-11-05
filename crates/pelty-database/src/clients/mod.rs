//! Client repositories: customers, workspaces, groups, KYC, GDPR, fraud checks and affiliates.
//!
//! Re-exported at the crate root. Covered by the clients/customers integration tests.

pub mod customers;
pub mod customer_workspace;
pub mod client_extras;
pub mod client_groups;
pub mod client_group_rules;
pub mod inactive_clients;
pub mod kyc;
pub mod gdpr;
pub mod fraud;
pub mod banned_emails;
pub mod affiliates;
pub mod affiliate_extras;
pub mod avatars;
pub mod customers_service;
pub use customers_service::ClientService;
