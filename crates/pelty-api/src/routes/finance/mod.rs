//! Billing HTTP routes: invoices (admin, client, PDF, e-invoice), quotes, payments, gateways, credit, coupons, tax, currencies, refunds, renewals, overdue rules and usage billing.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod billable;
pub mod billing;
pub mod coupons;
pub mod credit;
pub mod currencies;
pub mod einvoice;
pub mod gateways;
pub mod invoice_ops;
pub mod invoice_pdf;
pub mod legal_entities;
pub mod overdue;
pub mod payment_methods;
pub mod quote_pdf;
pub mod quotes;
pub mod refunds;
pub mod renewals;
pub mod tax;
pub mod usage;
