//! Pure billing domain logic: billing cycles, invoice math, service lifecycle.
//! No I/O here so every rule is unit-testable; SQL lives in pelty-database.
pub mod cycle;
pub mod lifecycle;
pub mod math;
pub mod proration;
pub mod tax;
pub mod vat_format;
