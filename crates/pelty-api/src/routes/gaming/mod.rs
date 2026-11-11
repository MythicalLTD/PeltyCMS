//! Optional domain-pack routes (gaming store, digital products, creator
//! marketplace). Every router here is gated by [`modules::gate`]: when the
//! owning module is off the routes answer 404 as if they did not exist.
//! Covered by the `modules_flow` integration tests and pelty-docs route inventory tests.

pub mod community;
pub mod creator_codes;
pub mod creator_payouts;
pub mod creator_tools;
pub mod delivery;
pub mod digital;
pub mod discord_roles;
pub mod games;
pub mod marketplace;
pub mod modules;
pub mod payouts;
pub mod query;
pub mod rcon;
pub mod reviews;
pub mod sales;
pub mod storefront;

/// `{"items": [...]}` list envelope used by the module APIs.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct Items<T> {
    pub items: Vec<T>,
}
