//! Netopia Payments (RO) redirect checkout gateway.
pub mod netopia;

#[cfg(all(target_family = "wasm", feature = "export"))]
pelty_addon_sdk::gateway_addon!(netopia::descriptor, netopia::factory);
