# Events and hooks

Reference: [Events index](events.md) (every event, payload and emit site), [Cancel and modify](cancellable.md) (everything you can stop or change) and [Hook points index](hook-points.md).

PeltyCMS emits a **domain event** after something has happened, for example `invoice.paid`. Before a few sensitive actions it also runs a **pre-event**, for example `order.before_place`, which a hook can cancel or modify. Every name, payload field and flag is listed in the [event registry](events.md). That registry is checked by a test that scans the source for every emitted event.

## Who can receive events

- **WASM addons** list domain events in `capabilities.events`. The host calls `on-event(name, payload)` after the action has been committed.
- **WASM hooks** list event names in `capabilities.hooks`. These are pre-events, lifecycle phases such as `before:PUT /api/v1/client/profile`, or custom events. The host calls op `hook`.
- **Webhook plugins** declare `hooks` in their plugin manifest. The host sends each event as a signed HTTPS POST to `webhook_url`.

## Listening

{{snippet:listen}}

## Cancelling and modifying

{{snippet:hooks}}

A hook replies with one of the following:

- `{"action":"continue"}`: let it through.
- `{"action":"modify","payload":{…}}`: replace the payload. Money fields can only change for addons holding `money.modify`.
- `{"action":"cancel","reason":"…"}`: stop the action.
- `{"action":"uncancel"}`: undo an earlier cancellation. This only works with `ignore_cancelled: true`.

## What you can hook

Pre-events (cancel and/or modify, synchronous) cover the main write paths:

- **Accounts:** `login.before`, `client.before_register`, `user.before_password_reset`, `customer.before_create`, `customer.before_update` (any profile field).
- **Sales and billing:** `order.before_place`, `order.before_approve`, `order.before_cancel`, `order.before_delete`, `invoice.before_create`, `quote.before_accept`, `credit.before_adjust`.
- **Services and domains:** `service.before_action`, `service.before_change`, `service.before_cancellation_request` (retention flows), `domain.before_order`, `domain.before_nameservers`.
- **Automation:** `billing.before_overdue_run` (postpone the daily overdue run, e.g. on holidays or during a payment outage).
- **Support:** `ticket.before_open` (triage: subject, body, priority, department), `ticket.before_reply` (redaction, signatures).
- **Mail:** `email.before_send` rewrites or drops any non-security transactional email in the worker.
- **Privacy:** `gdpr.before_erase` (legal holds).

Domain events (after the commit) cover customers, contacts, users and staff roles, MFA, API keys, KYC, GDPR, orders, invoices (merge, split, duplicate), payments, credit and gift cards, services (activation, plan changes, cancellation requests), domains (orders, nameservers, DNS, contacts, renewals), the catalog and coupons, quotes, tickets (opened, updated, assigned, notes, closed), email delivery, scheduled jobs (`automation.task_completed`/`automation.task_failed`) and the addon lifecycle itself. Any HTTP route can also be hooked with a lifecycle phase (below).

## Budgets and isolation

Each hook and mixin may set `timeout_ms` (at most 5000) and `fuel`. The host interrupts the WASM store when either runs out, so a stuck hook costs at most its deadline, not the sandbox limit. Failures follow `fail_open`. Every call is counted per addon: `GET /api/v1/admin/addons/health` (all) and `GET /api/v1/admin/addons/{id}/health` return calls, failures, timeouts, skipped calls and latency. After `PELTY_ADDON_BREAKER_FAILS` consecutive failures (default 5) the addon's hooks are skipped for `PELTY_ADDON_BREAKER_COOLDOWN_SECS` (default 60), then one trial call is let through. `POST /api/v1/admin/addons/{id}/health/reset` clears it.

## Filters

Events tell you something happened, and hooks let you veto or rewrite a request. A **filter** rewrites a *value* the host is about to show or send. Filter points are the public catalog, the client dashboard, a rendered email, a client's invoice, service and order views, and the list of addon pages and slots sent to the browser.

```json
"capabilities": { "filters": [ { "name": "email.content", "priority": 10 } ] }
```

```rust
"filter" => {
    let mut v = input["value"].clone();
    v["text"] = json!(format!("{}\n\n-- Sent with love", v["text"].as_str().unwrap_or("")));
    json!({ "value": v })
}
```

The reply must keep the value's JSON schema. Arrays may shrink or be reordered. Money keys follow the `money.modify` rules. Filters never break the host: on any error the value passes through unchanged. See the filter table on the Hooks and mixins page.

### Prices in checkout and invoicing

The `catalog.public` filter changes what a visitor *sees*; checkout always re-prices from the database. To change what a customer *pays*, subscribe to `checkout.discounts` (needs `money.modify`). Before the order transaction the host quotes every cart line, passes `{lines: [{id, product_id, cycle, amount, discount: "0", label: ""}]}` through the filter, and reads back each line's `discount`. Inside the transaction the line is priced again and the discount is clamped to what is left after coupon, bundle and client-group discounts. It is added to the first invoice as a separate negative line (`<label>, <product>`). Every changed value is audited (`addon.money_modified`), and the order gets an `order.addon_discounts` audit entry. Discounts only: prices never go up, and recurring renewals are not affected.

```rust
"filter" if input["filter"] == "checkout.discounts" => {
    let mut v = input["value"].clone();
    for l in v["lines"].as_array_mut().into_iter().flatten() {
        l["discount"] = json!("2.50");
        l["label"] = json!("Loyalty");
    }
    json!({ "value": v })
}
```

Manual staff invoices go through the `invoice.before_create` pre-event, where `money.modify` addons may rewrite `unit_price`. Recurring renewal invoices are generated in bulk by a single SQL statement per batch inside the billing worker, so they have no per-invoice price hook. An addon that cares about renewals listens to `invoice.created` (`renewal: true`). Keeping addons out of that batch keeps it fast and atomic, and one slow addon can never hold up every customer's invoice run.

Your addon can offer filter points of its own (ABI 0.4). Declare them in `capabilities.filter_points` (`"my-addon:greeting"`), then run them with `pelty_addon_sdk::apply_filter("my-addon:greeting", &value, &context)`. Other addons subscribe exactly like to a core filter point. Your own subscription to your own point is skipped, so applying a point from inside a filter cannot loop.

## Core-function mixins

A route mixin wraps an HTTP endpoint. A **core-function mixin** wraps a host function directly, wherever it is called from: give the mixin the target `fn <name>` (for example `fn checkout.place`, `fn cart.view`, `fn order.admin_detail`, `fn ticket.open`, `fn mail.enqueue`, `fn service.provision` or `fn domain.register`). The last three are *input-only*: only `before` and `around` mixins are called, and only to rewrite the input (email locale and variables; the module config and custom fields of one provisioning call; nameservers and registry fields before registration).

```json
"capabilities": { "mixins": [ { "target": "fn ticket.open", "at": "around", "timeout_ms": 500 } ] }
```

```rust
fn mixin(m: &op::Mixin) -> Result<Value, String> {
    match (m.function.as_deref(), m.phase.as_str()) {
        (Some("ticket.open"), "before") => {
            let mut i = m.input.clone();
            if i["subject"].as_str().unwrap_or("").contains("invoice") {
                i["department_id"] = json!(BILLING_DEPARTMENT);
            }
            Ok(reply::fn_input(Some(i), json!({ "routed": true })))
        }
        _ => Ok(reply::mixin_keep()),
    }
}
```

Since ABI 0.4 an addon can also ask which operation it is serving: `pelty_addon_sdk::context()` (WIT `context.current`) returns e.g. `{"kind": "mixin", "target": "fn ticket.open", "at": "around", "priority": 0}`, or the hook event and phase, or the filter name.

`before` rewrites the input, `after` the output, `around` does both and passes `state` from one phase to the other. Identity values (customer, cart, order) arrive in a read-only `context`. The same rules as filters apply: same JSON schema, no money changes, fail open.

## Scheduled jobs

An addon can run work on a schedule: list the op under `capabilities.jobs` in `addon.json`, for example `"jobs": [{ "op": "cleanup", "every_secs": 3600 }]` (at least 60 seconds; `0` means the job only runs when queued). The scheduler queues one `addon.job` per window and the addon's `cleanup` op is called with the same fuel, deadline and failure limits as any other op. Each scheduled job appears beside the built-in tasks as `<addon-id>:<op>` (Admin > Automation, console `cron list`): it can be run now, disabled or re-enabled, and its history is kept. A disabled or uninstalled addon's jobs stop being queued.

### Cron jobs (ABI 0.5)

For calendar schedules, declare cron jobs instead: set `"abi": "0.5.0"` and list them under `capabilities.cron`, for example `"cron": [{ "name": "nightly", "schedule": "0 3 * * *", "description": "Nightly cleanup" }]`. Schedules are standard five-field expressions (minute, hour, day of month, month, day of week) in the company time zone; at most 20 per addon, and names may not clash with `capabilities.jobs` ops. Cron jobs need the `cron` permission, so the admin approves it at install time like the other dangerous capabilities.

On each matching minute the host queues one `addon.job` and calls the component's `on-cron(job)` export (interface `pelty:addon/scheduled`, world `pelty-addon-cron`). With the SDK, enable its `cron` feature (`pelty-addon-sdk = { ..., features = ["cron"] }`) and implement `Addon::cron(job)`; `integration_addon!` then exports `on-cron` too, and the JSON you return is stored as the run's result. Returning an error marks the run failed, with the usual retries.

The manifest schedule is only the default: the admin can change it under **Admin > Automation** (with a preview of the next runs) or with `cron schedule <addon-id>:<job> "<expr>"` in the console, and `cron schedule <task> default` restores it. Missed runs are not caught up after downtime.

An addon can also add jobs at runtime with `power::cron_register(name, schedule, description)` and remove them with `power::cron_unregister(name)` (at most 20; re-registering replaces the schedule). Runtime-only addons declare `"cron_runtime": true` to get the `cron` permission. Registrations are stored in the database and seen by every instance within a minute. Uninstalling an addon removes its runtime jobs and any edited schedules.

Components built for ABI 0.2 to 0.4 keep loading unchanged; `capabilities.jobs` works as before.

## Lifecycle phases

{{phases}}

## Custom events

An addon can emit `<its-own-id>:<event>` if the name is listed in `capabilities.emits`. The `before` phase runs synchronously through every listener, in priority order. Other addons subscribe with `before:<id>:<event>` (and `after:` or `finished:`) in their `hooks`. An addon cannot emit an event in another addon's namespace.

{{snippet:emit}}
