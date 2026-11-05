//! Commerce repositories: products, bundles, order forms, the public storefront and orders (placing and approval).
//!
//! Re-exported at the crate root. Covered by the catalog/storefront/order integration tests.

pub mod bundles;
pub mod catalog;
pub mod order_approval;
pub mod order_forms;
pub mod orders;
pub mod orders_service;
pub mod storefront;
pub use orders_service::OrderService;
