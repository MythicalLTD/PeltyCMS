# Products and orders

{{nav:admin:products}}

## Products and pricing

Products belong to a product group, which is how the store organises them (`/store/{group}` in the client area). Each product has a status, a sort order, optional stock (`stock_enabled`, `stock_qty`, `allow_backorder`) and an optional provisioning module with its module settings.

Prices are set per currency and billing cycle. The cycles are `monthly`, `quarterly`, `semiannual`, `annual`, `biennial`, `triennial` and `one_time`. Currencies are enabled in Admin → Billing → **Currencies**; at least one must be enabled before the store can sell.

Bundles combine products, order forms give a product group its own order page, and usage metrics bill metered usage (the `usage.bill` task runs hourly).

## Orders

A client order creates an order, its services and the first invoice. Orders can be approved automatically by rules evaluated when the order is created or its invoice is paid; otherwise staff accept them in Admin → Products & services → **Orders**. Fraud screening runs on new orders: scores from the built-in rules and fraud addons add up, and Admin → Security → **Fraud** sets the accept / review / reject thresholds. A fraud check that fails is ignored, so an outage never blocks sales.

## Cancellations

Clients request cancellation from the service page (immediately or at the end of the billing period). Requests appear in Admin → Products & services → **Cancellations**.
