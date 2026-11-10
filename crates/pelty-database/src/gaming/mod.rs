//! Optional domain packs: gaming store (game profiles, in-game delivery,
//! server status and voting), digital products with licenses and the
//! creator marketplace. Everything here is off until an admin enables the
//! module in `feature_modules` (see [`modules`]).
//!
//! Re-exported at the crate root. Covered by the `modules_flow` integration tests.

pub mod community;
pub mod creator_codes;
pub mod creator_payouts;
pub mod delivery;
pub mod digital;
pub mod discord_roles;
pub mod games;
pub mod marketplace;
pub mod modules;
pub mod reviews;
pub mod sales;
pub mod storefront;
