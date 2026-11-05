//! Razorpay gateway addon: Payment links, signed webhooks, refunds.
//! The whole gateway lives in [`razorpay`]; this file only exports it to the host.
pub mod razorpay;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(razorpay::descriptor, razorpay::factory);
