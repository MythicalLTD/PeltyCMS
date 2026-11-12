# Optional modules (gaming, digital products, marketplace, reviews, community, store look)

PeltyCMS is a hosting and billing panel out of the box. The same install can also run a game server store (Minecraft, FiveM/RedM, Rust and others, like Tebex or CraftingStore), sell digital products with licenses, or host a creator marketplace (like BuiltByBit). Each of these is an **optional module**: nothing changes until you switch it on, and you can combine them freely.

## Switching modules on and off

Go to Admin → System → **Modules** (`settings.manage`). Each card is one module with a switch.

- A fresh install has every module **off**. The client area, store and admin look exactly like a classic hosting panel.
- Some modules need another one first (shown as **Requires**). For example, in-game delivery needs game profiles, and the creator marketplace needs digital products.
- You cannot switch off a module that another enabled module needs (shown as **Needed by**). Switch that one off first.
- A disabled module hides its admin pages, client pages and navigation, and its API answers `404 Not Found` as if it did not exist. Each module registers its own pages and API paths for this, so the rule applies to every module listed below as it ships. Data is kept, so switching it back on restores everything.

Every change is written to the audit log as `module.updated`.

| Module | Pack | Requires |
| --- | --- | --- |
| `game_profiles` | Gaming store | |
| `game_delivery` | Gaming store | `game_profiles` |
| `server_community` | Gaming store | `game_profiles` |
| `gaming_storefront` | Gaming store | |
| `digital_products` | Digital products | |
| `creator_marketplace` | Creator marketplace | `digital_products` |
| `tier_upgrades` | Commerce | |
| `bundle_ownership` | Commerce | |
| `guest_checkout` | Gaming store | `game_profiles` |

## API

- `GET /api/v1/public/modules` (no login) returns `{"enabled": [...]}`, the ids of the enabled modules.
- `GET /api/v1/admin/modules` lists every module with `enabled`, `requires`, `required_by` and its `settings` object.
- `PUT /api/v1/admin/modules/{id}` with `{"enabled": true, "settings": {...}}` toggles a module. `settings` is optional; leave it out to keep the current settings. Answers `409` when a dependency rule blocks the change and `404` for an unknown module.

## Game profiles and game servers (`game_profiles`)

Supported games: Minecraft Java and Bedrock, FiveM and RedM, Rust, Garry's Mod, ARK, Counter-Strike 2, Unturned, and **Other game** for anything else. Each game defines the player identity fields a customer can link:

| Field | Used for | Format |
| --- | --- | --- |
| `username` | Player name (every game) | 1-32 of letters, digits, `_`, `-`, `.` (no spaces or command characters) |
| `uuid` | Minecraft Java UUID | 32 hex digits, stored dashed |
| `xuid` | Minecraft Bedrock Xbox ID | 15-17 digits |
| `steamid` | Rust, Garry's Mod, ARK, CS2, FiveM | SteamID64 (`7656119...`, 17 digits) |
| `license` | FiveM / RedM Rockstar license | 40 hex digits, with or without `license:` |
| `discord_id` | Discord user ID | 17-20 digits |
| `fivem` | FiveM / RedM Cfx.re account id | digits, with or without `fivem:` (stored without the prefix) |

Values are validated before they are saved, so a linked identity can never inject extra console commands.

**Game servers** live in Admin → Products → **Game servers** (`servers.manage`). For each server set the game, name, store slug, public address and how commands reach it:

- **RCON**: PeltyCMS connects to the server's RCON port (Minecraft `enable-rcon`, Rust `+rcon.web 0`, Source games `rcon_password`). The RCON password is encrypted at rest with `PELTY_ENCRYPTION_KEY` and never returned by the API; leave the field empty when editing to keep it.
- **Server plugin (agent)**: a plugin or resource on the game server polls PeltyCMS for commands (FiveM/RedM resource, Bedrock or any game without RCON). Click **Issue agent token** and paste the token into the plugin config; it is shown once and rotating it revokes the old one.
- **None**: the server only has a store and a status card.

**Store packages** picks which catalog products the server's store sells, in display order. The public store of a server is `GET /api/v1/public/game/servers/{slug}`.

Customers link their identities in the client area under Account → **Game accounts**. Staff can see them with `GET /api/v1/admin/game/customers/{id}/accounts`.

**Filled for the player, asked when missing.** Players rarely know their in-game IDs, so the store fills what it can:

- **Sign-in fills the IDs it proves.** Login with Steam writes the SteamID64 (`steamid`), Login with Discord the Discord user id (`discord_id`) and Login with FiveM the Cfx.re account id (`fivem`) onto the player's game accounts, for every game with an enabled store that has that field. Only empty fields are filled; an ID the player already saved is never replaced by a sign-in. No sign-in gives the Rockstar `license:`.
- **Checkout asks for the rest.** On the store checkout a signed-in player sees a **Player details** step: the IDs already on file are shown as filled, and an input appears for each ID the package's delivery commands need (for example `{license}` in `add_principal identifier.license:{license} group.vip`) that is still missing. On FiveM / RedM the Rockstar licence is always asked when missing. The checkout is refused with `error.game_identity_required` until a needed ID is given; what the player types is saved to their game accounts, so the next purchase needs nothing. Guests are asked for the same needed IDs. Gifted lines are skipped (the recipient's name is used).
- `GET /api/v1/client/game/checkout-identity/{cart_id}` returns, per game of the cart, each field with the known value and whether the delivery needs it. `POST /api/v1/client/checkout` takes the typed IDs as `"identity": {"fivem": {"license": "license:…"}}`.

**Package checkout fields.** Like Tebex custom fields: open **Checkout fields** on a package (Admin → Products, or **Store packages** on a game server) and add text, long text, number, dropdown or checkbox fields, required or optional, with an optional format (regex). A text or number field can be **filled from a player ID** (player name, Rockstar licence, SteamID64, Discord id, Cfx.re id, Minecraft UUID or XUID): it is pre-filled from the buyer's game account, the server fills it too if left empty, and a value the buyer types is saved to the game account when it had none. Answers are stored on the order line and the service like any custom field.

### API

| Method | Path | Who |
| --- | --- | --- |
| GET | `/api/v1/public/game/games` | public |
| GET | `/api/v1/public/game/servers` | public |
| GET | `/api/v1/public/game/servers/{slug}` | public |
| GET, POST | `/api/v1/client/game/accounts` | client |
| DELETE | `/api/v1/client/game/accounts/{id}` | client |
| GET, POST | `/api/v1/admin/game/servers` | `servers.manage` |
| GET, PUT, DELETE | `/api/v1/admin/game/servers/{id}` | `servers.manage` |
| PUT | `/api/v1/admin/game/servers/{id}/products` | `servers.manage` |
| POST | `/api/v1/admin/game/servers/{id}/agent-token` | `servers.manage` |
| GET | `/api/v1/admin/game/customers/{id}/accounts` | `customers.manage` |

## In-game delivery (`game_delivery`)

Requires **Game profiles**. Open **Products → In-game delivery**.

- **Commands** run when a package is bought (`purchase`), renewed (`renewal`), expires or is suspended/cancelled (`expiry`), refunded (`refund`) or charged back (`chargeback`). Set them per package or globally for every package. Each command can target one server or every server whose store sells the package, can wait until the player is online, and can be delayed. Servers with transport `none` never receive commands. For "wait until online" over RCON, the queue first asks the server for its player list (`list` on Minecraft, `status` elsewhere) and holds the command, without using an attempt, until the player's name appears. A delivery that is claimed but never answered fails after 10 attempts. After an RCON error, the remaining commands for that server are put back in the queue rather than sent on the same connection.
- **Placeholders**: `{username}`, `{uuid}`, `{uuid_nodash}`, `{xuid}`, `{steamid}`, `{license}`, `{discord_id}`, `{fivem}`, `{package}`, `{package_id}`, `{service_id}`, `{order_id}`, `{customer_id}`, `{amount}`, `{currency}`, `{server}`. Unknown placeholders are rejected when you save.
- **Missing game account**: the store checkout asks for an identity a command needs (see *Filled for the player, asked when missing* above). For orders placed another way (staff orders, the client order form), when a command needs an identity the customer has not linked, the delivery waits (`Waiting for game account`) and is sent automatically as soon as they link it on **Game accounts**.
- **Queue**: every command is a row with status, attempts and the last error. Failures are retried with backoff (2, 4, 8 … up to 60 minutes, 10 attempts). Retry or cancel rows by hand, or resend a service's commands.
- **Transports**: RCON servers are processed by the worker every minute (or **Run queue now**). Agent servers (FiveM, RedM, any plugin) poll the API with their agent token:

| Call | Body | Result |
| --- | --- | --- |
| `POST /api/v1/game-agent/poll` | `{"online": ["<identity>", …], "limit": 50}` | Commands to run now; each is leased for 2 minutes. Online-only commands are returned only for listed players. |
| `POST /api/v1/game-agent/ack` | `{"delivered": [id…], "failed": [{"id": …, "error": "…"}]}` | Marks results; failures are retried. |

Send the token as `Authorization: Bearer pga_…`. Every poll/ack is scoped to the token's server.

## Digital products (`digital_products`)

Open **Products → Digital products** and pick a product under **Make a product digital**.

- **Versions**: upload each release (zip, gzip, PDF, text; same size limit as Downloads) with a version label and changelog. Customers who own the product see every version on **Licenses & files**; the public changelog is at `GET /api/v1/public/digital/products/{id}/changelog`.
- **Secure downloads**: the download button creates a one-off link (`/api/v1/public/digital/download/{token}`) that expires after the product's link lifetime (default 15 minutes) and allows 5 uses. Ownership is checked again when the link is used, so refunded or revoked customers cannot reuse old links.
- **License keys**: with licensing on, every purchase issues a key (`XXXXX-XXXXX-XXXXX-XXXXX-XXXXX`, stored encrypted). Set the activation limit (0 = unlimited), an optional validity in days, and what an activation is bound to: domain, IP and/or hardware ID. Staff can issue keys by hand, suspend, revoke and reset activations; customers can copy their key and reset their own activations.
- **Lifecycle**: suspending the service suspends its license, unsuspending reactivates it, and termination, cancellation, refund or chargeback revokes it.
- **External assets (FiveM escrow)**: set an asset id and each purchase records a grant (with the customer's linked FiveM license when Game profiles is on) and fires `digital.asset_granted`. Cfx.re has no public API for granting escrow assets, so fulfil grants with a webhook/integration or mark them granted by hand; refunds fire `digital.asset_revoked`.

### License API

Call it from the software you sell. Only the fields the product binds are needed.

| Call | Body | Result |
| --- | --- | --- |
| `POST /api/v1/public/licenses/validate` | `{"key": "…", "product_id": "…", "domain": "shop.example", "hwid": "…", "activate": true}` | `{"valid": true, "activations": 1, "max_activations": 3, "expires_at": …}` or `{"valid": false, "reason": "…"}` |
| `POST /api/v1/public/licenses/deactivate` | same identity fields | `{"removed": true}` frees the slot |

Reasons: `not_found`, `wrong_product`, `suspended`, `revoked`, `expired`, `domain_required`, `ip_required`, `hwid_required`, `activation_limit`, `not_activated` (with `"activate": false`), `license_disabled` (the product's digital settings were removed or licensing is switched off). A license whose service was terminated, cancelled or marked fraud is revoked at the next check, even if the revocation event was missed. The IP defaults to the caller's address. Repeated unknown keys from one address are rate limited.

## Creator marketplace (`creator_marketplace`)

When this module is on, customers can become **creators** and sell their own products (plugins, FiveM resources, maps, configs) through your store. You take a commission on each sale. Manage it under **Admin → Products → Creator marketplace**. Customers find it under **Billing → Sell as a creator**.

**Modes.** The Settings tab switches between:

- **Multi-vendor.** Customers apply as creators and submit listings.
- **Admin-only.** Only staff sell. Existing creators keep their dashboard, earnings and payouts, but cannot add new listings, and new applications are refused with `error.market_closed`.

**Settings.** Settings are stored on the module itself:

| Setting | Meaning |
|---|---|
| `mode` | `admin_only` or `multi_vendor` |
| `commission_percent` | Default store commission (a per-creator override is set when you approve a creator) |
| `hold_days` | Days a sale stays on hold before it can be paid out (refund/chargeback window) |
| `min_payout` | Smallest payout a creator can request |
| `auto_approve_creators` / `auto_approve_listings` | Skip manual review |
| `payout_schedule` | `manual` (creators request), `weekly` or `monthly`: the whole available balance is paid out automatically on that day (store timezone) |
| `payout_weekday` / `payout_monthday` | Day for weekly (1 = Monday … 7 = Sunday) or monthly (1–28) payouts |
| `payout_approval` | On (default): payouts wait for staff to **Send** them. Off: requests and scheduled payouts go to Stripe/PayPal right away |
| `payout_methods` | Methods creators may choose (`manual`, `stripe_connect`, `paypal`) |
| `reverse_transfers_on_refund` | On (default): a refund or chargeback on an already paid-out sale pulls the money back from the creator's Stripe transfers |

**Flow.**

1. A customer applies with a display name, URL name and payout details. Staff approve, reject or suspend the creator.
2. The creator submits a listing (name, category, description, price, billing cycle) and uploads versions. A listing becomes a hidden catalog product until staff approve it. After approval the product goes live and works with digital products (versioned downloads and license keys) if that module is on.
3. When an invoice is paid, a `sale` ledger entry is written for each line that belongs to a creator. The entry holds the gross amount, the commission and the net amount, and it is available after `hold_days`.
4. Refunds and disputes (`order.refunded`, `invoice.refunded`, `payment.disputed`) write a matching negative `refund` entry, so the balance can go negative and is recovered from later sales.
5. A creator requests a payout from the available balance, and the amount is debited right away. Staff then **Send** it through the provider, or mark it paid, rejected or failed. Rejected or failed payouts are credited back.

**How money flows (separate charges and transfers).** Buyers always pay *your* store through your normal payment gateways. PeltyCMS does not use Stripe destination charges: a cart can mix several creators and your own products, and buyers may pay with any gateway, so each sale is credited to the creator's ledger (minus commission) and paid out later from your platform balance. Creators get paid by one of three methods, chosen in **Billing → Sell as a creator → Payouts** (staff can override and lock it per creator under **Creators**):

- **Manual** (bank transfer or anything else). **Send** moves the payout to *processing*; you pay outside PeltyCMS and then mark it *paid*.
- **Stripe Connect (Express).** The creator clicks **Connect with Stripe** and finishes Stripe's hosted onboarding (account links with return and refresh URLs back to the dashboard). The connected account id and its status (`onboarding`, `restricted`, `enabled`, `disconnected`) are stored on the creator and kept current by the `account.updated` webhook. Sending a payout creates a Stripe **transfer** from your platform balance to the connected account (idempotent per payout). Stripe then pays out to the creator's bank on the creator's own Stripe schedule; the creator can open their Express dashboard from PeltyCMS. A reversed transfer credits the creator's balance back; a `payout.failed` on the creator's account is shown on the payout and mailed to the creator (the money stays in their Stripe balance).
- **PayPal Payouts.** The creator saves a PayPal email. Sending creates a PayPal payout batch (one item per payout, idempotent per batch). Item status arrives by webhook and is also polled hourly; `SUCCESS` marks the payout paid, and `FAILED`, `RETURNED`, `BLOCKED`, `REFUNDED`, `REVERSED`, `DENIED` or `CANCELED` mark it failed and credit the amount back. `UNCLAIMED` stays processing until PayPal returns it (after 30 days).

**Refunds after a payout.** When a refund or chargeback pushes a creator's balance below zero and `reverse_transfers_on_refund` is on, PeltyCMS reverses the creator's most recent paid Stripe transfers for up to the missing amount and books a `transfer_reversal` ledger entry. PayPal payouts cannot be pulled back; the negative balance is then recovered from later sales.

**Jobs.** `creator_payouts.sync` runs hourly (retries unsent PayPal batches, polls PayPal, and sends requested Stripe and PayPal payouts when approval is off). `creator_payouts.auto` runs once on the scheduled day and creates a payout for every creator whose available balance is at least `min_payout`. **Run now** on the Payouts tab does the same on demand.

**Going live.** Gateway credentials are read from environment variables on the server (never stored in the database or shown in the UI). Test keys work out of the box; live keys are refused unless `PELTY_CREATOR_PAYOUTS_LIVE=true`.

| Variable | Meaning |
|---|---|
| `PELTY_STRIPE_CONNECT_SECRET_KEY` | Secret key of your Stripe platform account with Connect enabled (`sk_test_…` or `sk_live_…`; restricted keys need Connect accounts, account links and transfers write access). `PELTY_STRIPE_CONNECT_KEY` is still read as a fallback |
| `PELTY_STRIPE_CONNECT_WEBHOOK_SECRET` | Signing secret (`whsec_…`) of a Connect webhook endpoint |
| `PELTY_PAYPAL_PAYOUTS_CLIENT_ID` / `PELTY_PAYPAL_PAYOUTS_CLIENT_SECRET` | REST app credentials; the PayPal account must have Payouts enabled |
| `PELTY_PAYPAL_PAYOUTS_MODE` | `sandbox` (default) or `live` |
| `PELTY_PAYPAL_PAYOUTS_WEBHOOK_ID` | Id of the webhook you create in that PayPal app (used to verify webhook signatures) |
| `PELTY_CREATOR_PAYOUTS_LIVE` | `true` to allow live Stripe keys and PayPal live mode |

Webhooks to create (the exact URLs are shown on **Admin → Creator marketplace → Payouts**):

- Stripe: `{your site}/api/v1/webhooks/creator-payouts/stripe`, listening to events **on connected accounts** `account.updated`, `account.application.deauthorized`, `payout.failed`, and to your account's `transfer.created`, `transfer.updated`, `transfer.reversed`.
- PayPal: `{your site}/api/v1/webhooks/creator-payouts/paypal`, with `PAYMENT.PAYOUTS-ITEM.*` and `PAYMENT.PAYOUTSBATCH.*`.

Webhook events are deduplicated, so Stripe or PayPal retries are safe. Without webhooks Stripe statuses refresh when the creator clicks **Refresh status**, and PayPal relies on the hourly poll.

**Gallery and updates.** Creators add up to 12 screenshots with captions per listing: drag several image files onto the gallery (or pick them), paste an https URL, and drag rows to reorder (the arrow buttons do the same from the keyboard). The resource page shows them as a gallery after the listing image. When a creator uploads a new version, every customer who owns the product (and has not opted out of marketing mail) receives an update email linking to their downloads.

Payout administration needs the **Affiliates** permission. Creator and listing review need **Catalog**.

**Discount codes.** On **Creator → Discount codes** a creator makes codes that only work on their own listings (percentage or fixed amount, total uses, uses per customer, expiry). The creator's slug is put in front of every code (`spring` becomes `NOVA-SPRING`), so codes never clash with the store's own coupons or other creators' codes. A code that has already been used is switched off instead of deleted, so old orders keep their history. Customers enter it in the normal coupon box at checkout.

**Bundles.** On **Creator → Bundles** a creator groups 2–10 of their approved listings with a discount (0–100%). Each listing uses its first billing cycle. The bundle is shown on the page of every listing in it ("Add bundle to cart" puts the missing ones in the cart). When a store cart holds every listing of an active bundle, the best such bundle is applied automatically and shown as its own line in the cart summary. The discount is stored per order line, so creator earnings are counted after the bundle (and coupon) discount.

**Wishlists.** Signed-in customers save resources with the heart button on a resource page and see them on **Wishlist** (`/wishlist`, up to 200). Creators see how many wishlists a listing is on in its analytics.

**Messages.** "Message" on a resource page starts a conversation with the creator (up to 20 new conversations per customer per day; not with yourself). Conversations show whether the customer had already paid for the listing (*verified buyer*). Both sides use **Creator messages** (`/messages`); each new message emails the other side (template *Marketplace message*, linking to `/messages?c={id}`) and fires the `market.message_received` event, so you can also route it to Discord or Slack with a notification rule. Either side can close or reopen a conversation.

**Analytics.** **Analytics** on each listing in **Creator → Listings** shows views, paid sales, revenue (after discounts, before tax), the creator's earnings, conversion (sales ÷ views) and wishlist count for the last 7, 30 or 90 days (UTC days) as a chart. A view is counted each time the public resource page is loaded; there is no bot or repeat-visitor filtering.


### API

| Method | Path | Who |
|---|---|---|
| GET | `/api/v1/public/marketplace` | anyone (current mode) |
| GET | `/api/v1/public/creators`, `/api/v1/public/creators/{slug}` | anyone |
| GET | `/api/v1/public/marketplace/listings?q=&category=&sort=trending\|new\|top_rated\|popular&limit=` | anyone: approved listings (with creator, category, sales, rating) and the categories |
| GET | `/api/v1/public/marketplace/listings/{product}` | anyone: one resource page (listing, creator profile, more by the creator, active bundles that include it); counts a view |
| GET/POST/PUT | `/api/v1/client/creator` | dashboard / apply / update profile |
| GET/POST | `/api/v1/client/creator/listings` | the creator |
| POST | `/api/v1/client/creator/listings/{id}/listed` | unlist or relist |
| POST | `/api/v1/client/creator/listings/{id}/versions` | the creator |
| GET | `/api/v1/client/creator/ledger` | the creator |
| GET/POST | `/api/v1/client/creator/payouts` | the creator |
| GET | `/api/v1/admin/creators`, POST `/api/v1/admin/creators/{id}/status` | staff (Catalog) |
| GET | `/api/v1/admin/creator-listings`, POST `/api/v1/admin/creator-listings/{id}/review` | staff (Catalog) |
| PUT | `/api/v1/client/creator/payout-settings` | the creator: method and PayPal email |
| POST | `/api/v1/client/creator/stripe/onboard`, `/stripe/sync`, `/stripe/dashboard` | the creator: Stripe onboarding link, status refresh, Express dashboard link |
| GET/PUT | `/api/v1/client/creator/listings/{id}/gallery` | the creator |
| GET/POST | `/api/v1/client/creator/coupons`, PUT/DELETE `/{id}` | the creator: discount codes |
| GET/POST | `/api/v1/client/creator/bundles`, PUT/DELETE `/{id}` | the creator: bundles |
| GET | `/api/v1/client/creator/listings/{id}/analytics?days=7\|30\|90&currency=` | the creator: daily views, sales, revenue, earnings, conversion |
| GET | `/api/v1/client/wishlist`, PUT/DELETE `/api/v1/client/wishlist/{product}` | signed-in customers |
| GET/POST | `/api/v1/client/market-messages` | inbox (both sides) / start a conversation (`product_id`, `subject`, `body`) |
| GET | `/api/v1/client/market-messages/{id}` | either side: thread (marks it read) |
| POST | `/api/v1/client/market-messages/{id}/reply`, `/{id}/status` | either side: reply, close (`{"open": false}`) or reopen |
| POST | `/api/v1/client/creator/media` | any signed-in customer (raw image body): avatar, banner, cover and gallery uploads |
| GET | `/api/v1/admin/creator-payouts`, POST `/{id}/send`, POST `/{id}/status` | staff (Affiliates) |
| GET | `/api/v1/admin/creator-payouts/overview` | staff (Affiliates): provider configuration, test/live, webhook URLs |
| POST | `/api/v1/admin/creator-payouts/run`, `/send-batch` | staff (Affiliates): create due payouts, send, sync PayPal |
| PUT | `/api/v1/admin/creators/{id}/payout` | staff (Affiliates): set and lock a creator's method |
| POST | `/api/v1/webhooks/creator-payouts/stripe`, `/paypal` | Stripe / PayPal (signed) |

Events: `creator.applied`, `creator.payout_requested`, `creator.payout_paid`, `creator.payout_failed`, `market.message_received`.

## Reviews and Q&A (`product_reviews`)

When this module is on, every active store product page shows:

- the star rating with a 5-to-1 histogram,
- approved reviews, newest first, with verified buyers listed first,
- a **Questions & answers** section.

Signed-in customers can write one review per product. Editing a review sends it back to moderation and removes the reply. Customers can also ask questions.

A review carries a **Verified purchase** badge when the customer bought the product, meaning any one of these exists:

- a service for the product that is not pending,
- a paid order line,
- a license.

Moderate reviews and questions under **Admin → Products → Reviews & Q&A**. Approve or reject them, reply to reviews publicly, and answer questions. Answering a question also publishes it. When the creator marketplace is on, the creator who owns a product can reply to its reviews and answer its questions from the **Reviews** tab of the creator dashboard. Creators cannot approve or reject anything.

Settings on the module:

| Setting | Default | Meaning |
|---|---|---|
| `require_approval` | `true` | New reviews and questions wait for staff approval |
| `verified_only` | `false` | Only buyers may review (others get `error.review_purchase_required`) |
| `questions` | `true` | Product Q&A is open |

Each customer can have at most 5 unanswered questions per product. Public responses show the author as "First L." and never include customer ids or emails.

### API

| Method | Path | Who |
|---|---|---|
| GET | `/api/v1/public/reviews/products/{id}?limit=&offset=` | anyone: summary, approved reviews |
| GET | `/api/v1/public/reviews/products/{id}/questions` | anyone |
| GET | `/api/v1/client/reviews` | your reviews |
| GET/POST | `/api/v1/client/reviews/products/{id}` | eligibility / create or edit your review |
| DELETE | `/api/v1/client/reviews/{id}` | your review |
| POST | `/api/v1/client/reviews/products/{id}/questions` | ask |
| GET | `/api/v1/client/reviews/creator` | approved creators: reviews and questions on their products |
| POST | `/api/v1/client/reviews/creator/reviews/{id}/reply`, `/creator/questions/{id}/answer` | owning creator |
| GET | `/api/v1/admin/reviews`, `/api/v1/admin/reviews/questions` (`?status=`) | staff (Catalog) |
| POST | `/api/v1/admin/reviews/{id}/status`, `/{id}/reply` | staff (Catalog) |
| POST | `/api/v1/admin/reviews/questions/{id}/status`, `/questions/{id}/answer` | staff (Catalog) |

Events: `review.submitted`, `review.approved`, `question.asked`, `question.answered`.

## Server status, voting and leaderboards (`server_community`)

Needs `game_profiles`. Switch it on and a public **Community** page appears at `/community` with:

- **Live server status** for every game server that has a public address: online/offline, players and slots, version, MOTD and a sample of player names. A background job checks each server once a minute (Minecraft Java ping, Minecraft Bedrock ping, Source A2S for Rust/ARK/CS-style games, and FiveM/RedM `info.json` + `players.json`). Each check times out after a few seconds; the last error is shown to admins under **Community → Servers**, and **Check now** runs a check immediately.
- **Vote links** to server lists. Put `{username}` in the vote page URL and the page fills in the name the player types.
- **Top voters** for the current month and, if you switch it on, **top supporters** (paid invoices this month in the goal currency). Supporters are hidden by default; when shown, they appear by their game account **player name** (never an ID such as a UUID or Steam ID), or first name and last initial. Amounts appear only if **Show donation amounts** is on. Emails never appear.
- An optional **monthly goal** bar ("server costs"), based on paid invoices this month in the goal currency. Set the goal to `0` to hide it.

### Vote rewards

Under **Community → Vote sites**, add each server list with its vote URL, an optional reward command (for example `give {username} diamond 1`), the game server that runs it (or all), whether the player must be online, and a cooldown (0–168 hours, default 24).

Click **Issue token** to get the callback URL to paste into the server list's settings:

```
https://your-panel/api/v1/public/community/vote/<site id>?username={username}&token=pgv_…
```

The list calls it when someone votes. The callback accepts `GET` or `POST`, and reads the name from `username`, `user`, `player` or `name` and the token from `token`, `key`, `secret` or the `X-Vote-Token` header. Only a hash of the token is stored; rotating it stops the old one. A wrong token gets `403`, and calls with a wrong token are rate-limited per IP. A repeat vote by the same name inside the cooldown gets `409 error.vote_cooldown` and no reward. If the reward cannot be queued, the vote is undone so the site can retry. Vote names with spaces are only rewarded on Bedrock servers (quoted as one argument); elsewhere the vote counts but no command is queued, so a name can never add command arguments. Caller IPs are cleared after 30 days by housekeeping; votes stay for the leaderboards.

Rewards go through the **in-game delivery** queue (`game_delivery` must be on; trigger `vote`), so they retry, wait for the player when "must be online" is set, and show up under **Game deliveries**. Commands can use `{username}`, `{player}`, `{site}`, `{server}` (unknown placeholders and line breaks are refused) and, when the name matches a customer's game account, that customer's identity variables. Each accepted vote emits the `community.voted` event for extensions and webhooks.

### API

- `GET /api/v1/public/community` (no login): `servers`, `sites`, `top_voters`, `top_donors` (`null` when hidden), `goal` (`null` when unset).
- `GET|POST /api/v1/public/community/vote/{site}`: the vote callback above.
- `GET /api/v1/admin/community`: sites, the last 100 votes, statuses and check errors.
- `POST /api/v1/admin/community/refresh`, `POST /api/v1/admin/community/sites`, `PUT|DELETE /api/v1/admin/community/sites/{id}`, `POST /api/v1/admin/community/sites/{id}/token`.

Admin routes need the **Manage servers** permission. Settings (`goal_amount`, `goal_currency`, `goal_label`, `leaderboard_size` 1–50, `show_donors`, `show_donor_amounts`) are saved with `PUT /api/v1/admin/modules/server_community`.

Limits: the status check needs the server's query port to be reachable from the panel (it uses the server's **query port** when set, otherwise the game port, otherwise the game's default port). Server lists that only send a signed callback with their own scheme (for example Votifier) are not supported yet; use their HTTP/webhook callback.

## Store look (`gaming_storefront`)

Changes how the game store looks when **Settings → Website → Store mode** is **Game store**. Switch the module on, then open **Store look**:

- **Banner**: a title, subtitle and background image (upload or drop a file, or use an `https://` URL or a path on your site such as `/uploads/banner.png`).
- **Accent color**: recolors buttons, focus rings and charts on the public site (theme default, emerald, violet, amber, rose, sky).
- **Package layout**: a dense list or cards.
- **Sidebar** on the left or right, with up to 12 widgets in the order you choose:
  - **Text box**: rules, about, contact. Shown as plain text (no HTML).
  - **Top customer**: the biggest spender today, this week, month, year or all time. Counts paid invoices in the default currency. Shown by game account player name (never an ID) or first name and last initial; the amount only if you switch it on.
  - **Recent payments** (1–10): who bought which packages, by product name only (no invoice details, no emails).
  - **Featured package**: one active product with its image and a link. Hidden products are skipped.
  - **Server status** and **Monthly goal**: from the community module (`server_community`, above); they only appear while `server_community` is on.
  - **Discord button**: a `https://discord.gg/…` or `https://discord.com/invite/…` link.

Store-wide look (used when the public site is in store mode, below):

- **Accent preset**: the public store, marketplace, support pages, basket and client area always use the PeltyCMS panel design (the active theme's colours, cards, buttons, badges, font, radius and light/dark mode). *Match PeltyCMS* (the default; saved as `panel`, the old value `default` means the same) keeps the panel accent, or your Website brand colour when one is set. *Minecraft* (green), *FiveM* (orange), *Rust* (red) and *Monochrome* change only the accent colour. The accent color above wins over the preset. The staff area never changes.
- **Server in the banner**: which game server the homepage banner shows (default: the first enabled one). Visitors see its address with a copy button (`connect host:port` for FiveM and RedM, the port filled in from the game's default when you left it empty) and, while `server_community` is on, the live player count.
- **Server logo** (`https://` or `/path`; empty uses the site logo) and a **Discord invite** for the banner button and the store menu.

**Announcement and categories.** An announcement bar above the store (plain text up to 300 characters, an optional `https://` or `/path` link, and an *info*, *sale* or *warning* style). Each category can override the package layout (list or cards); categories without an override use the store layout.

**Package rules** (Store look → Package rules), per package:

- **Per-player limit**: how many times one customer may buy it, ever or per rolling day / 7 days / 30 days.
- **Limit for everyone**: how many may be sold in total in the same kinds of period (a limited run, e.g. "100 founder ranks"). Shown on the store as "Only N left" and "Sold out".
- Pending and paid orders count towards both limits; cancelled and fraud orders do not. The check runs inside the order transaction with the rule locked, so two checkouts cannot both take the last one.
- **Required packages**: the buyer must already own one of them (an active or suspended service, or a paid order for a one-time package), or have one in the same cart.
- **Pay what you want**: for one-time prices, the listed price is the minimum and the buyer may enter more (up to 100× the price, at most 100,000, two decimals). Coupons, sales and creator codes apply to the amount they chose.
- **Can be bought as a gift** (on by default): the buyer ticks *Buy as a gift* on the package page and enters the recipient's in-game name. The buyer pays and gets the invoice; delivery commands use the recipient's name for `{username}` and `{player}`. Commands that need anything else (a UUID, a FiveM license) are marked failed with "gift: missing …" instead of waiting, so they are never sent to the buyer's own account. The per-player limit counts the buyer, not the recipient.

**Store ban list** (Store look → Store ban list): in-game names and emails that cannot buy. Checkout is refused when the buyer's email, any of their linked game names, or a gift recipient is on the list (`error.store_banned`, which does not say which entry matched). Values are stored in lower case.

**Currency.** With the module on, a first-time visitor who has not picked a currency gets the currency of their browser's region (for example `en-US` → USD) when the store sells in it; otherwise the store default.

### API

| Method | Path | Who |
|---|---|---|
| GET | `/api/v1/public/storefront/package-rules` | anyone: limits left, requirements, pay what you want, gifting (signed-in customers also get their own remaining limit and whether they meet the requirement) |
| GET | `/api/v1/admin/storefront/package-rules` | staff (Catalog) |
| PUT | `/api/v1/admin/storefront/package-rules/{product}` | staff (Catalog): a rule with every default removes it |
| GET/POST | `/api/v1/admin/storefront/bans`, DELETE `/{id}` | staff (Catalog) |

Order lines (store carts, `/api/v1/client/orders`, headless API) accept `pay_amount` (string) and `gift_to` (in-game name). Both are refused while the module is off.

With the module off, the store keeps its plain look, rules and bans are not applied, and these endpoints return 404.

## Public site look (Settings → Website)

**Public site look** decides what visitors see on `/`, in the header menu and in the footer:

| Setting | Result |
|---|---|
| Automatic (default) | Game server store when `gaming_storefront` is on, else the creator marketplace when `creator_marketplace` is on, else the hosting landing. |
| Hosting landing | Always the hosting homepage (your Website pages) and your own menu. |
| Game server store | `/` is the store homepage: big server banner (logo, name, copyable address, live players, Discord), quick links (game account, vote, leaderboards, support), category tabs, package cards with images, prices, sale badges and **Add to cart**, and the sidebar widgets (featured package, recent payments, top customer, goal). The menu becomes Store, Vote, Leaderboards (with `server_community`), Marketplace (when that module is on), Discord and Support, plus the cart. `/store` always uses the game store layout. |
| Creator marketplace | `/` is the marketplace: search, categories (your product groups that hold approved listings), Trending (most sales in the last 30 days), New and Top rated (with `product_reviews`) shelves. Resource pages are at `/marketplace/r/{product}` (image gallery, description, versions and changelog with `digital_products`, reviews and Q&A with `product_reviews`, Buy box, creator card). Creator profiles are at `/creators/{slug}`; `/creators` lists them. The menu becomes Explore, Creators, Store (when the game store is on), Sell (the creator dashboard) and Support. |

When both modules are on, pick which one is the homepage; the other stays in the menu. Choosing a look whose module is off falls back to Automatic.

In every look, signed-in customers land on the store or marketplace homepage (the client area is a click away) and the client area uses the same colours. **Support** (`/support`) gathers the knowledgebase (with search), **Open a ticket**, your tickets, live chat (when it is switched on), network status, announcements and the contact form; it is in the menu and a Support column is added to the footer. Every public page shows *Powered by PeltyCMS* in the footer.

Players sign in with their PeltyCMS account (or **Login with FiveM** / **Login with Steam** when the `sso-cfx` / `sso-steam` addons are on, see Admin → Security → Single sign-on) and link their game identity (Minecraft name, FiveM license, Steam ID, Discord ID) under **Game accounts**. A Cfx.re sign-in does not provide the FiveM `license:` identifier; delivery still uses the one entered under Game accounts.

### API

- `GET /api/v1/public/storefront` (no login): the banner, accent, theme, logo, Discord link, the banner `server` (`name`, `slug`, `game`, `address`, `status` with `online`/`players`/`max_players` or `null`), layout, sidebar side and the widgets with their live data. Widgets with nothing to show (an unknown product, community off) are left out.
- `GET|PUT /api/v1/admin/storefront` (**Manage settings**): read or replace the settings. Invalid values return `422 error.storefront_invalid`.

## Image uploads

Every image field in the store (store banner and server logo, product and package images, creator avatar, profile banner, listing cover and gallery) accepts a dropped or picked file as well as a URL.

- **Formats**: PNG, JPEG, WebP and GIF, up to 2 MB (`PELTY_UPLOAD_AVATAR_MB`). The file type must match its content; SVG and other formats are refused.
- **Processing**: every upload is decoded and saved again as PNG, so hidden payloads and metadata are dropped. Images larger than 2048 px on the long edge are scaled down, and a 480 px thumbnail is kept at `/api/v1/media/{id}/thumb`. An animated GIF keeps only its first frame.
- **Storage**: files are written to the media folder on the server's disk (`PELTY_MEDIA_DIR`, default `data/media`) and served publicly at `/api/v1/media/{id}`. There is no S3 or other object-storage backend; to share files between several app servers, mount the same folder on each.
- **Creator limits**: customers (creators and applicants) can upload 60 images an hour and keep at most 1,000 images or 500 MB. Staff uploads (`/api/v1/admin/media`, needs the Settings permission) are not limited.

## Timed sales (`timed_sales`)

Part of the **Commerce** pack. Run a time-limited percentage discount on
chosen products or whole product groups, such as a weekend sale or a launch
discount, without making a coupon.

- **Admin → Sales** (`/admin/sales`, needs *Manage catalog*): name, percent
  (above 0 and up to 95), start and end time, and the products and/or groups
  it covers (1–500 in total). A sale can be switched off without deleting it.
  Every change is written to the audit log (`sale.created`, `sale.updated`,
  `sale.deleted`).
- **Only running sales apply**: enabled, already started and not yet ended.
  Scheduled sales show nothing until their start time.
- **No stacking between sales**: when several sales cover one product, the
  highest percent wins. A sale does stack with a coupon, a bundle or a client
  group discount. It is applied after them and never takes a line below zero.
- **First invoice only**: the sale discounts the order's first invoice. Later
  renewals are charged the normal price.
- **Store**: the game store shows the old price struck through, a −X% badge
  and a banner with a countdown to the end of the sale. The cart and checkout
  show a separate *Sale* row.
- Turning the module off stops every sale at once. Saved sales are kept and
  apply again when it is turned back on.

## Creator codes (`creator_codes`)

Part of the **Commerce** pack. It builds on the affiliate program: an affiliate
picks a short code, such as a streamer's name, and buyers type it at checkout to
support them.

- **Affiliates** pick or rename their code on their affiliate page. A code is
  3–24 characters (letters, digits, `_`, `-`), is not case-sensitive, and must
  be unique. Joining the affiliate program comes first.
- **Admin → Creator codes** (`/admin/creator-codes`, needs *Manage
  affiliates*) lists every code with its order count. Here you can give buyers
  an optional discount (0–50 %, off by default) or switch a code off. Changes
  are audited (`creator_code.updated`; affiliates' own changes are
  `creator_code.set`).
- **Checkout**: the cart has a *Creator code* field. An unknown or switched-off
  code is reported in the cart and refused at checkout. Buyers can't use their
  own code. The discount applies to the order's first invoice after coupons,
  bundles, client-group discounts and sales, and never takes a line below zero.
  The order is recorded against the code (`order.creator_code` in the audit
  log).
- **Commission**: when that first invoice is paid, the creator earns their usual
  affiliate commission: base rate, tiers and per-product rates, exactly like a
  referral. For that invoice it replaces the buyer's signup referrer, so only
  one commission is paid. Renewals keep following the normal referral rules.
- Turning the module off hides the field and the pages. Codes entered from then
  on are ignored rather than refused. Orders already placed with a code still
  pay their commission when their invoice is paid.

## Tier upgrades (`tier_upgrades`)

Rank ladders like VIP < VIP+ < MVP. Under **Admin → Marketing → Tier upgrades**, put the packages of one ladder into a **tier group**, lowest first (a package can be in one group only). A player who owns a lower tier then pays less for a higher one:

- **Credit mode**: *What they paid* credits the amount actually paid for the owned tier (after discounts, scaled down by partial refunds) plus any upgrade credit that tier carried, so VIP → VIP+ → MVP is cumulative and never counts VIP twice. *Price difference* credits the owned tier's current list price, so the upgrade costs the difference.
- **Minimum price**: the upgrade never costs less than this (in the default currency, converted at the currency rate); 0 allows free upgrades.
- Only the best owned tier counts; one owned tier credits one upgrade at a time (also when two upgrades are in one cart or checked out at once).
- **Owned** means an active service from a paid, accepted order that is not refunded, cancelled or marked fraud. Expired, terminated or cancelled subscriptions do not count. Refunding or cancelling an upgrade releases its credit back to the lower tier without crediting both.
- **Gifts** use the recipient's tiers: everything gifted to that player name plus the tiers of customers who linked that name under Game profiles. The buyer's own tiers do not count for a gift.
- **Pricing order**: the credit is taken off the package price first; coupons, bundles, client-group discounts, timed sales and creator codes then apply to the upgrade price, followed by tax. Gift cards and account credit pay the invoice as usual. The invoice shows an *Upgrade credit: VIP+ (from VIP)* line.
- **Old tier**: *Cancel the old subscription* (default on) cancels a recurring lower tier when the upgrade order is accepted so it stops billing. *Run removal commands* queues the old tier's in-game expiry commands (Game delivery) when the upgrade is delivered.
- Store pages show an *Upgrade* badge, the reduced price and the credit in the cart.

Limitations: the credit is based on the owned tier's first invoice (renewals are not added); refunding an upgrade does not reactivate a subscription it cancelled.

### API

| Method | Path | Who |
| --- | --- | --- |
| GET | `/api/v1/public/tier-upgrades?currency=EUR&gift_to=Name` | anyone: tier groups; `credits[product_id] = {amount, from_name}` for the signed-in customer (or the gift recipient) |
| GET/POST | `/api/v1/admin/tier-groups`, GET/PUT/DELETE `/{id}` | staff with catalog access |

## Bundle ownership discount (`bundle_ownership`)

Like BuiltByBit: a buyer who already owns some items of a bundle can still buy the bundle and pays only for the rest. Configure it under **Admin → Products → Bundles** once the module is on:

- **Owned items are worth**: *their share of the bundle price* (the rest keeps the bundle percent, e.g. 3 of 5 items at 20% off) or *their full list price* (the rest costs the bundle price minus the list price of what is owned).
- **Minimum price**: the extra discount for owned items never takes the rest below this percent of its list price (0 = no floor). The bundle's own percent always applies, even when it is deeper than the floor.
- **Owned** means an active service of the product or an active, unexpired license, for the signed-in customer. Guests see normal bundle prices.
- The bundle offer on a resource page shows *You already own 2 of 5*, marks owned items and adds only the rest to the cart; the cart and the invoice line (*Bundle discount: Mega (2 of 5 already owned)*) say the same.
- A buyer who owns every item cannot buy that bundle again: the offer is disabled and a cart or order with all of it is refused with `error.bundle_all_owned`.
- The discount is applied like a normal bundle discount (after coupons, before group discounts, timed sales and creator codes, then tax).

Limitation: the hosting order form (Billing → Order) still asks for every bundle item before it offers the bundle; a claimed `bundle_id` on `POST /api/v1/client/orders` does accept owned items.

### API

| Method | Path | Who |
| --- | --- | --- |
| GET | `/api/v1/public/bundle-ownership` | anyone: `{mode, min_percent, owned}` (`owned` = product ids the signed-in customer owns, else empty) |
| PUT | `/api/v1/admin/modules/bundle_ownership` | `settings.manage`: `{"enabled": true, "settings": {"mode": "share" or "list", "min_percent": "0"}}` |

## Guest checkout (`guest_checkout`)

Like Tebex: on stores that allow it, a buyer checks out with only a player name (or game ID) and an email, without creating an account first.

- **Per store**: turn on *Allow guest checkout* on the store under **Admin → Game servers**. A cart can be bought as a guest when every package in it is sold on an enabled store with guest checkout on, for the same game. Other carts need a normal account.
- **What the buyer enters**: the game's identity fields (Minecraft Java: the player name; FiveM / RedM: player name, Rockstar/Cfx `license:` or SteamID64; other games: the player name or Steam ID) and an **email**. The email is required: receipts, invoices and the claim link are sent there, and it is the only way back into the order.
- **Checks**: Minecraft Java names are looked up with the Mojang API (*Check Minecraft names with Mojang*, on by default), so unknown names are refused and the correct spelling and UUID are saved. Other identifiers get a format check (a FiveM license is 40 hex characters, a SteamID64 is 17 digits starting with 7656119). The captcha for *checkout*, banned email domains, the store ban list (email and player name) and fraud screening apply as for signed-in buyers.
- **Shadow account**: the first guest order for an email creates a customer account keyed by that email with an unusable random password, links the player, opens a session so the buyer can pay right away, and emails a *set your password* link (valid 30 days) that claims the account. The order, invoice, delivery commands (`{username}`, `{uuid}`, ...) and receipts work like any order.
- **Same email again**: while the account is unclaimed, a new guest order is added to it but no session is opened; the buyer gets the link by email, sets a password and pays from the client area. A different player for that email is refused (`error.guest_identity_mismatch`). Once a password is set (claim link or *Forgot password*), or for any normal account, the buyer must sign in (`error.guest_account_exists`).
- **Rate limits**: guest checkouts per IP per hour (default 10) and per email per hour (default 5), under **Admin → Game servers → Guest checkout**.

Limitations: guest checkout is for gaming stores (packages on a game server); the reused-account path places the order before the buyer proves the email, so an unpaid order can appear in an unclaimed guest account (rate limited, never delivered until paid).

### API

| Method | Path | Who |
| --- | --- | --- |
| GET | `/api/v1/store/guest-checkout/{cart_id}` | anyone: `{available, game, fields, verify_minecraft}` |
| POST | `/api/v1/store/guest-checkout` | anyone (captcha form `checkout`): `{cart_id, email, identity: {username: "Notch"}, fields?, country?, locale?}` → `{order, invoice, session, csrf_token}`; sets the session cookie only for a new email |
| PUT | `/api/v1/admin/modules/guest_checkout` | `settings.manage`: `{"enabled": true, "settings": {"verify_minecraft": true, "ip_per_hour": 10, "email_per_hour": 5}}` |
| PUT | `/api/v1/admin/game/servers/{id}` | `guest_checkout: true` turns it on for that store |

Event: `store.guest_checkout` (`customer_id`, `order_id`, `game`, `new_account`). Mail template: `guest_claim` (`player`, `order`, `link`, `expires_days`).

## Discord roles (`discord_roles`)

Part of the **Gaming** pack. It gives buyers roles on your Discord server for
what they bought, such as a VIP role for a VIP rank, and takes them away again.

- **Setup** (Admin → Discord roles, needs *Manage settings*):
  - Create a Discord bot and invite it to your server with the *Manage Roles*
    permission.
  - Put the bot's role **above** every role it should give.
  - Enter the server (guild) ID and the bot token. The token is stored
    encrypted with the encryption key and is never shown again. Leave the field
    empty to keep it.
- **Rules**: up to 10 role IDs per product, and whether roles are removed when
  the service expires (suspended, terminated or cancelled). Refunds and
  chargebacks always remove them.
- **Who gets roles**: buyers are matched by the Discord account linked to
  their login (Discord sign-in). A buyer who hasn't linked one sees their
  changes as *Waiting*; these continue on their own once the account is linked.
- **Keeping roles**: a role is not removed while the same buyer has another
  active service that gives it.
- **Queue**: every change is listed with its status.
  - Rate limits and Discord outages are retried with back-off, up to 8 attempts.
  - A member who isn't in the server yet is retried every hour.
  - A missing permission, or a role above the bot, fails with the reason shown.
  - Failed, skipped and waiting changes can be retried by hand, and *Run now*
    processes the queue immediately.
  - Every setting and rule change is audited.
- Turning the module off stops new changes from being queued and hides the
  page. Roles already given stay on Discord.

## Gaming reports

Admin → Reports → *Report library* has a **Gaming & creators** group. Each
report shows only while its module is on. Like every library report, each one
covers whole months and exports to CSV.

| Report | Module | What it shows |
| --- | --- | --- |
| Sales by game server | `game_profiles` | Paid revenue (excluding tax, after every discount) and items for each server's store packages. A new order's paid lines are shared over its items by price, including invoices merged or split from the order's invoice; renewals count by service line. A package sold on two servers counts for both. |
| In-game deliveries | `game_delivery` | Delivered, failed, pending and waiting-for-player-link commands for each server, by when they were queued. |
| Creator code usage | `creator_codes` | Orders placed with each code, and how much of their invoices has been paid. |
| Marketplace sales by creator | `creator_marketplace` | Gross, commission, net and refunds from each creator's ledger. |

Amounts are excluding tax, in each order's currency.
