# Hooks and UI slots

This page is the map of every place an addon can plug into PeltyCMS. Each
extension point is listed with what it receives, what it may change, and a
minimal manifest snippet. The full generated lists live in
[Events](events.md), [Hooks and filters](hooks.md) and [Widgets and slots](widgets.md);
this guide explains when to use which.

| Need | Use | Runs |
| --- | --- | --- |
| React to something that happened (order placed, ticket opened, payment failed) | **Event hook** (`capabilities.events`) | After commit, async, retried |
| Change a value before PeltyCMS uses or shows it | **Filter** (`capabilities.filters`) | Inline, fail-open |
| Add a sidebar entry | **Nav menu** (`ui.menus[]`, slots `admin.nav` / `client.nav`) | Browser |
| Add a stat card to a dashboard | **Widget** (`ui.widgets[]`, `admin.dashboard` / `client.dashboard`) | Browser |
| Add UI inside an existing page | **Render slot** (`ui.slots[]`) | Browser |
| Change the invoice PDF text | Filter **`invoice.pdf`** | Server, on PDF render |

## Event hooks

Subscribe in `addon.json`:

```json
{ "capabilities": { "events": ["order.created", "payment.failed", "ticket.opened"] } }
```

The addon's `on-event` export receives `{name, payload}`. Delivery is
at-least-once through the job queue: make handlers idempotent (the payload
carries the record id). Every event and its payload fields are in
[Events](events.md). Staff can also route the same events to Discord, Slack,
Telegram and Teams without an addon (Admin → Notifications).

## Filters

```json
{ "capabilities": { "filters": [{ "name": "invoice.pdf", "priority": 50 }] } }
```

The host calls op `filter` with `{filter, value, context}` and keeps your
`{value}` only when it has the same shape. Filters fail open: an error or
timeout keeps the original value. Money keys never change unless the
addon holds `money.modify` and the filter allows it.

### `invoice.pdf`

- **value**: `{notes: [string], footer: [string]}`
- **context**: `invoice_id`, `customer_id`, `locale`
- **limits**: 20 notes, 6 footer lines, 300 characters each, control
  characters stripped. Lines, totals and amounts are never passed.

Example reply that adds payment instructions:

```json
{ "value": { "notes": ["Pay by bank transfer to RO49 AAAA 1B31 0075 9384 0000"], "footer": ["Thank you for your business"] } }
```

## Nav menus and widgets

```json
{
  "ui": {
    "menus": [{ "slot": "client.nav", "label": "plugin.my-addon.menu", "path": "/p/my-addon/home" }],
    "widgets": [{ "slot": "admin.dashboard", "label": "plugin.my-addon.widget", "path": "/api/v1/addons/my-addon/stats" }]
  }
}
```

A widget endpoint returns `{value, label?, href?}`. Clients can hide or
reorder dashboard widgets themselves; yours appears as one more card.

## Render slots

A slot contribution is a JSON node tree (or an approved iframe) rendered in
place. `{{ctx.*}}` reads the context listed next to each slot.

| Slot | Where | Context |
| --- | --- | --- |
| `admin.dashboard` / `client.dashboard` | Dashboards | — |
| `client.home` | Client home | — |
| `admin.customer.detail` | Admin client page | `customer_id` |
| `admin.service.detail` | Admin service page | `service_id` |
| `admin.order.detail` | Admin order page | `order_id` |
| `admin.invoice.detail` | Admin invoice page | `invoice_id` |
| `admin.ticket.reply` | Above the staff reply box | `ticket_id` |
| `client.ticket.reply` | Above the client reply box | `ticket_id` |
| `client.checkout.step` | Checkout, before the payment method | `currency`, `gateway` |
| `client.invoice.detail` | Client invoice, under related documents | `invoice_id` |
| `client.service.detail` | Client service page, at the bottom | `service_id` |

Every page also exposes generic positions (`<area>.<route>.before`,
`.after`, `.toolbar`, `.tabs`, `.row-actions`, `.detail-panel`) and layout
positions (`<area>.layout.header|sidebar|footer|banner`); see
[Addon UI](ui-guide.md).

Example: a canned-answer hint above the staff ticket reply box.

```json
{
  "ui": {
    "slots": [{
      "slot": "admin.ticket.reply",
      "data": { "hint": { "get": "/api/v1/addons/my-addon/tickets/{{ctx.ticket_id}}/hint" } },
      "body": { "type": "alert", "title": "plugin.my-addon.suggested", "text": "{{data.hint.text}}" }
    }]
  }
}
```

Example: an extra checkout step (terms checkbox, survey, upsell).

```json
{
  "ui": {
    "slots": [{
      "slot": "client.checkout.step",
      "body": { "type": "card", "title": "plugin.my-addon.upsell", "children": [{ "type": "text", "text": "plugin.my-addon.upsell_text" }] }
    }]
  }
}
```

A checkout slot is display only: prices still come from the database at
order time. To discount an order, use the `checkout.discounts` filter.
