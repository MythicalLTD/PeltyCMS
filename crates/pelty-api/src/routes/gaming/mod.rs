//! Optional domain-pack routes (gaming store, digital products, creator
//! marketplace). Every router here is gated by [`modules::gate`]: when the
//! owning module is off the routes answer 404 as if they did not exist.
//! Covered by the `modules_flow` integration tests and pelty-docs route inventory tests.

pub mod delivery;
pub mod games;
pub mod modules;
pub mod rcon;
