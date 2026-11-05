//! Billing HTTP routes: invoices (admin, client, PDF, e-invoice), quotes, payments, gateways, credit, coupons, tax, currencies, refunds, renewals, overdue rules and usage billing.
//!
//! Each submodule exposes `router()`-style functions merged in `routes::api_router`; old paths like
//! `crate::routes::<module>` keep working through re-exports in `routes/mod.rs`.
//! Covered by the pelty-api integration tests for these domains and by pelty-docs route inventory tests.

pub mod billing;
pub mod invoice_ops;
pub mod invoice_pdf;
pub mod einvoice;
pub mod quotes;
pub mod quote_pdf;
pub mod billable;
pub mod credit;
pub mod coupons;
pub mod currencies;
pub mod gateways;
pub mod payment_methods;
pub mod refunds;
pub mod renewals;
pub mod overdue;
pub mod tax;
pub mod usage;
pub mod legal_entities;
