//! Test fixture (not bundled): a provisioning addon with a unique engine id
//! so the addon lifecycle tests never toggle engines other tests rely on.
use pelty_addon_provisioning_pterodactyl::pterodactyl;
pelty_addon_sdk::provisioning_addon!(pterodactyl::AppPanel { id: "addontestmod", name: "Addon Test", accept: "application/json" });
