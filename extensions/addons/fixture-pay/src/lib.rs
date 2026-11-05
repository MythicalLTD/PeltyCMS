//! Test fixture (not bundled): the manual gateway under a unique engine id.
fn desc() -> pelty_payments::GatewayDescriptor {
    let mut d = pelty_addon_gateway_manual::manual::descriptor();
    d.id = "addontestpay";
    d.name = "Addon Test Pay";
    d
}
pelty_addon_sdk::gateway_addon!(desc, pelty_addon_gateway_manual::manual::factory);
