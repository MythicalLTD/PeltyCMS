//! Commerce repositories: products, bundles, order forms, the public storefront and orders (placing and approval).
//!
//! Re-exported at the crate root. Covered by the catalog/storefront/order integration tests.

pub mod catalog;
pub mod bundles;
pub mod order_forms;
pub mod storefront;
pub mod orders;
pub mod order_approval;
pub mod orders_service;
pub use orders_service::OrderService;
