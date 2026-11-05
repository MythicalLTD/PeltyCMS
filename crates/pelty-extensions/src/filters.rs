//! Filter points: values the host passes through addon `filters` before
//! using them (WordPress-style `apply_filters`). Each listener gets
//! `{filter, value, context}` and replies `{value}`; the host keeps the
//! JSON schema of the value (see `same_schema`), applies the money rules
//! and always fails open (a failing filter leaves the value unchanged).
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FilterSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// Shape of `value`.
    pub value: &'static str,
    /// Keys of the read-only `context`.
    pub context: &'static [&'static str],
    /// Rules the host applies to the filtered value, beyond keeping its schema.
    pub rules: &'static str,
    pub since: &'static str,
}

const fn spec(
    name: &'static str,
    description: &'static str,
    value: &'static str,
    context: &'static [&'static str],
    rules: &'static str,
) -> FilterSpec {
    FilterSpec {
        name,
        description,
        value,
        context,
        rules,
        since: "0.4.0",
    }
}

pub const FILTERS: &[FilterSpec] = &[
    spec("catalog.public", "The public store catalog (`GET /api/v1/catalog`) before it is returned: hide, reorder or relabel groups and products.", "`{groups: [...], products: [...]}` as returned by the endpoint", &[], "Display only: checkout always prices from the database. Price keys change only with `money.modify` (audited)."),
    spec("client.dashboard", "The client area dashboard summary before it is returned.", "The `ClientDashboard` object", &["customer_id"], "Money keys change only with `money.modify`."),
    spec("email.content", "A rendered transactional email in the mail worker, after `email.before_send`. Security mails (password reset, verification, invites, …) are never filtered.", "`{subject, text}`", &["template", "locale", "to"], "`subject` newlines become spaces; HTML is regenerated from `text`."),
    spec("ui.contributions", "Addon pages and slot contributions sent to the browser (`GET /api/v1/plugins/ui`): hide, reorder or re-rank other addons' UI.", "`{pages: [...], slots: [...]}`", &["staff"], "Only removal, reordering and the `order` of slot entries are kept: every returned entry must match an original one by `plugin` + `id` (pages) or `plugin` + `slot` (slots); anything else is ignored."),
    spec("invoice.view", "A client's invoice detail before it is returned (`GET /api/v1/client/invoices/{id}`): rewrite notes or line descriptions for display.", "The invoice detail object", &["customer_id", "invoice_id"], "Display only. An output that changes any money key is ignored as a whole (stored invoices are immutable)."),
    spec("service.view", "A client's service detail before it is returned (`GET /api/v1/client/services/{id}`): relabel, add display notes, hide addons.", "The service object with `addons`", &["customer_id", "service_id"], "Display only. An output that changes any money key is ignored as a whole."),
    spec("order.view", "A client's order detail before it is returned (`GET /api/v1/client/orders/{id}`).", "`{order, items, invoice, services}`", &["customer_id", "order_id"], "Display only. An output that changes any money key is ignored as a whole."),
    spec("checkout.discounts", "Addon discounts for an order about to be placed (store checkout and client orders), computed before the order transaction. Set a line's `discount` (and `label`) to take that amount off the first invoice.", "`{lines: [{id: cart_index, product_id, cycle, amount, discount: \"0\", label: \"\"}]}`", &["customer_id", "currency"], "Requires `money.modify`; every change is audited, and the placed order is audited with the applied discounts. Only `discount` is read: it must be ≥ 0 and is clamped to what is left of the line after coupon, bundle and group discounts, re-priced inside the transaction. Discounts apply to the first invoice only; prices never increase."),
];

pub fn spec_of(name: &str) -> Option<&'static FilterSpec> {
    FILTERS.iter().find(|f| f.name == name)
}

pub fn is_filter(name: &str) -> bool {
    spec_of(name).is_some()
}
