//! Billing engine pieces outside HTTP: overdue processing, autopay, exchange rates, e-invoices, PDF rendering and VIES VAT checks.
//!
//! Modules are re-exported at the crate root (`pelty_api::<module>`), so existing paths keep working.
//! Covered by the pelty-api unit tests in these files and the integration suite in tests/integration.

pub mod overdue;
pub mod autopay;
pub mod exchange_rates;
pub mod einvoice;
pub mod pdf;
pub mod vies;
