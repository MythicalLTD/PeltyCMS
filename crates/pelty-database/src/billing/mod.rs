//! Billing repositories: invoices, payments, credit, coupons, tax, currencies and gateways.
//!
//! Re-exported at the crate root (`pelty_database::invoices` etc.) so old paths keep working.
//! Covered by the billing_* and invoice_* integration tests in pelty-api.

pub mod add_funds;
pub mod autopay;
pub mod billable;
pub mod card_expiry;
pub mod coupons;
pub mod credit;
pub mod currencies;
pub mod exchange_rates;
pub mod gift_cards;
pub mod invoice_ops;
pub mod invoice_tools;
pub mod invoices;
pub mod overdue;
pub mod quotes;
pub mod refunds;
pub mod tax;
pub mod transactions;
pub mod gateway_admin;
pub mod gateway_rules;
pub mod legal_entities;
pub mod usage;
pub mod invoices_service;
pub use invoices_service::InvoiceService;
