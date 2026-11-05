//! Client HTTP routes: customers, workspaces, groups, client extras, KYC, GDPR, fraud, banned emails, affiliates, avatars and the client account.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod customers;
pub mod customer_workspace;
pub mod client_groups;
pub mod client_group_rules;
pub mod client_extras;
pub mod kyc;
pub mod gdpr;
pub mod fraud;
pub mod banned_emails;
pub mod affiliates;
pub mod affiliate_extras;
pub mod avatars;
pub mod account;
