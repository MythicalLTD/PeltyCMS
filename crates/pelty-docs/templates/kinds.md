# Gateways, provisioning and registrars

These addon kinds use the same WASM contract as other addons. Each one implements a fixed set of ops, and the SDK provides a macro so you only implement a Rust trait. The traits below are copied from the source.

## Payment gateways

Use `pelty_addon_sdk::gateway_addon!`. The host-side trait:

{{item:crates/pelty-payments/src/lib.rs:pub trait PaymentGateway}}

## Provisioning modules

Use `pelty_addon_sdk::provisioning_addon!`:

{{item:crates/pelty-provisioning/src/lib.rs:pub trait ProvisioningModule}}

## Domain registrars

Use `pelty_addon_sdk::registrar_addon!`:

{{item:crates/pelty-provisioning/src/registrar.rs:pub trait RegistrarModule}}

## SDK macros

{{item:extensions/addons/sdk/src/lib.rs:macro_rules! provisioning_addon}}
