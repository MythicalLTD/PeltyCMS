# Addon UI

Addons add UI without shipping JavaScript to the main app. There are three ways to do it:

- **Native pages** (`ui.pages[].body`): a JSON tree of shadcn components that the frontend renders. The allowed node types and their fields are in the [UI schema](ui.md).
- **Iframe pages** (`ui.pages[].frame`): a sandboxed HTML file from the addon's `assets/`. This needs `"iframe"` in `capabilities.ui`, and the admin must approve it explicitly at install.
- **Slots** (`ui.slots[]`): small contributions in named places of existing pages, such as a toolbar, the dashboard or the layout banner.

## Example

{{file:extensions/examples/addon-guide/addon.json:json}}

Strings that start with `plugin.<addon-id>.` are translated from the manifest's `i18n` map. `{{data.<name>}}` reads a data source declared in `data`, and `{{row.<field>}}` reads the current table row.

## Slots and positions

Every page and its exact slot names are listed in [Widgets and slots](widgets.md).

{{slots}}

## Replaceable sections

Detail pages mark their main blocks as sections. A slot named `<page>.before:<section>` or `<page>.after:<section>` adds content around a section; `<page>.replace:<section>` replaces it (needs the approved `ui.override` capability; the first replacement wins). Every section gets the route context plus the record ids listed below.

- Admin invoice (`invoice_id`, `status`): `lines`, `record-payment`, `payments`, `related`, `refunds`, `notes`.
- Admin order (`order_id`, `status`): `summary`, `fraud`, `notes`.
- Admin client (`customer_id`, `status`): `stats`, and one `tab-<name>` per tab (`tab-summary`, `tab-contacts`, `tab-services`, `tab-domains`, `tab-invoices`, `tab-transactions`, `tab-tickets`, `tab-notes`, `tab-emails`, `tab-log`).
- Admin general and support settings: one `tab-<name>` per tab (for example `tab-custom_code`, `tab-sla`, `tab-mailboxes`).
- Client domain (`domain_id`, `status`): `overview`, `nameservers`, `addons`, `dns`, `whois`.
- Client service management (`service_id`, `status`): `change-plan`, `cancellation`.
- Dashboards, the client invoice and the client order status page also have sections (`stats`, `services`, `upcoming`, `activity`, `quick-actions`, `revenue`, `items`, `lines`, `summary`, `progress`).

Header actions (pay, refund, approve, delete) are never replaceable, so an addon cannot hide or fake a money action.

## Iframe pages and the frame SDK

An iframe page runs in a sandbox: an opaque origin, no cookies, and `connect-src 'none'`. Its only channel to the panel is `postMessage`, and the panel checks every message. `extensions/frame-sdk/pelty-frame.js` wraps that protocol. Copy it into your addon's `assets/` and load it with `<script src="pelty-frame.js"></script>`; it has no dependencies.

- `Pelty.ready(cb)`: `cb(init)` once the panel sent `pelty:init`: `locale`, `ctx` (page context plus `route` and `area`), `viewer`, `theme`, `dark`, `token`. Theme variables, dark mode and `lang` are applied for you.
- `Pelty.request(path, {method, body})`: Calls the API through the panel. It resolves with the JSON body and rejects with `{status, error}`. Only your addon's own routes (`/api/v1/addons/<id>/…`) and the prefixes approved in `capabilities.bridge` are allowed.
- `Pelty.on(pattern, cb)`: Subscribes to the panel's event bus (`"cart.*"`, `"other-addon:*"`, `"*"`). Returns an unsubscribe function.
- `Pelty.emit(name, payload)`: Publishes `<your-id>:<name>` to the panel and to other frames.
- `Pelty.toast(text, tone)`: Shows a toast in the panel (`"success"` or `"error"`).
- `Pelty.navigate(to)`: Navigates the panel within the same area (client or admin).
- `Pelty.resize(height?)`: Reports the frame height. It runs automatically whenever the layout changes.
- `Pelty.filter(name, fn, priority?)`: Takes part in a panel filter (`"page.title"`, `"nav.items"`) while the frame is mounted. `fn(value, ctx)` returns the new value or a Promise of it, and gets a copy of the value. Returns a function that stops filtering.

Frame filters are asynchronous. The panel runs its own filters first, then each frame filter by priority (lower first). Each frame filter gets about 300 ms. A timeout, an error or a result of the wrong shape (a title over 200 characters, nav items without `to` and `label`, more than 50 items) is skipped and the previous value is kept. The panel shows the unfiltered value until the frame answers. A frame may register at most 8 filter names. On the wire, the frame sends `pelty:filter_register {name, priority}`; the panel sends `pelty:filter {id, name, value, ctx}` and the frame answers `pelty:filter_result {id, value}`.

The example addon's frame page uses it:

{{file:extensions/examples/addon-guide/assets/ui.html:html}}

`frontend/src/lib/addons/frame-sdk.test.ts` runs the SDK against the panel's message parser, so the SDK and the host stay in sync.
