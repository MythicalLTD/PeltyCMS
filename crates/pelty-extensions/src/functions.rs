//! Core functions addons can wrap with mixins (`capabilities.mixins` with
//! target `fn <name>`). A `before`/`around` mixin may rewrite the function's
//! `input`, an `after`/`around` mixin its `output`; both must keep the JSON
//! schema (see `same_schema`) and never change money values. Identity values
//! (who, which record) are passed in a read-only `context`. Mixins fail open:
//! a failing or invalid reply leaves the value unchanged.
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct FunctionSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// Shape of `input` (`null` when the function takes nothing rewritable).
    pub input: &'static str,
    /// Shape of `output` (`none`: input-only, `after` mixins are not called).
    pub output: &'static str,
    /// Keys of the read-only `context`.
    pub context: &'static [&'static str],
    pub since: &'static str,
}

const fn spec(
    name: &'static str,
    description: &'static str,
    input: &'static str,
    output: &'static str,
    context: &'static [&'static str],
) -> FunctionSpec {
    FunctionSpec {
        name,
        description,
        input,
        output,
        context,
        since: "0.4.0",
    }
}

pub const FUNCTIONS: &[FunctionSpec] = &[
    spec("checkout.place", "Turning a cart into an order and invoice (`POST /api/v1/client/checkout`). Rewrite the custom order field values or the billing country before the order is placed; read or relabel the result.", "`{fields: {line_index: {field_id: value}}, country}`", "`{order, invoice}`", &["customer_id", "cart_id"]),
    spec("cart.view", "Building a store cart with its price preview (`GET`/`PUT /api/v1/store/carts/{id}`).", "`null`", "`{cart, preview, error}`", &["cart_id", "customer_id"]),
    spec("order.admin_detail", "Building the staff order detail (`GET /api/v1/admin/orders/{id}` and order actions).", "`null`", "`{order, items, customer, invoice, services, ...}`", &["order_id", "customer_id"]),
    spec("mail.enqueue", "Queueing a transactional email (every template an operator can switch off; account-security mails are never passed). Rewrite the locale or the values of existing template variables (keys cannot be added or removed).", "`{locale, vars: {name: value}}`", "none", &["template", "to"]),
    spec("service.provision", "A provisioning module call for a service (create, suspend, unsuspend, terminate, …) from the queue. Rewrite the product's module config or the service's custom fields for this call only (e.g. pick a node, derive a hostname). Stored values stay unchanged.", "`{product_config, custom_fields}`", "none", &["service_id", "action", "module"]),
    spec("domain.register", "Registering a paid domain at its registrar (not transfers). Rewrite the nameservers or the registry extra fields before the registrar is called.", "`{nameservers: [...], extra_fields: {...}}`", "none", &["domain_id", "name", "registrar", "years"]),
    spec("ticket.open", "Opening a support ticket from the client area (`POST /api/v1/client/tickets`). Rewrite the subject, department or priority before the ticket is created (e.g. auto-routing).", "The ticket request body", "The created ticket", &["customer_id"]),
];

pub fn spec_of(name: &str) -> Option<&'static FunctionSpec> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

pub fn is_function(name: &str) -> bool {
    spec_of(name).is_some()
}

/// `fn <name>` mixin target → function name.
pub fn target_function(target: &str) -> Option<&str> {
    target.strip_prefix("fn ").filter(|n| is_function(n))
}

#[cfg(test)]
mod tests {
    #[test]
    fn names_unique_and_targets() {
        let mut n: Vec<_> = super::FUNCTIONS.iter().map(|f| f.name).collect();
        n.sort();
        n.dedup();
        assert_eq!(n.len(), super::FUNCTIONS.len());
        assert_eq!(super::target_function("fn cart.view"), Some("cart.view"));
        assert_eq!(super::target_function("fn nope"), None);
        assert_eq!(super::target_function("GET /api/v1/x"), None);
    }
}
