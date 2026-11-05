//! GoCardless gateway addon: Direct Debit via Billing Requests (mandate +
//! first payment), signed webhooks, refunds, off-session mandate charges.
pub mod gocardless;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(gocardless::descriptor, gocardless::factory);
