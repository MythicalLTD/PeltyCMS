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

Values are validated before they are saved, so a linked identity can never inject extra console commands.

**Game servers** live in Admin → Products → **Game servers** (`servers.manage`). For each server set the game, name, store slug, public address and how commands reach it:

- **RCON**: PeltyCMS connects to the server's RCON port (Minecraft `enable-rcon`, Rust `+rcon.web 0`, Source games `rcon_password`). The RCON password is encrypted at rest with `PELTY_ENCRYPTION_KEY` and never returned by the API; leave the field empty when editing to keep it.
- **Server plugin (agent)**: a plugin or resource on the game server polls PeltyCMS for commands (FiveM/RedM resource, Bedrock or any game without RCON). Click **Issue agent token** and paste the token into the plugin config; it is shown once and rotating it revokes the old one.
- **None**: the server only has a store and a status card.

**Store packages** picks which catalog products the server's store sells, in display order. The public store of a server is `GET /api/v1/public/game/servers/{slug}`.

Customers link their identities in the client area under Account → **Game accounts**. Staff can see them with `GET /api/v1/admin/game/customers/{id}/accounts`.

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
- **Placeholders**: `{username}`, `{uuid}`, `{uuid_nodash}`, `{xuid}`, `{steamid}`, `{license}`, `{discord_id}`, `{package}`, `{package_id}`, `{service_id}`, `{order_id}`, `{customer_id}`, `{amount}`, `{currency}`, `{server}`. Unknown placeholders are rejected when you save.
- **Missing game account**: when a command needs an identity the customer has not linked, the delivery waits (`Waiting for game account`) and is sent automatically as soon as they link it on **Game accounts**.
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

**Flow.**

1. A customer applies with a display name, URL name and payout details. Staff approve, reject or suspend the creator.
2. The creator submits a listing (name, category, description, price, billing cycle) and uploads versions. A listing becomes a hidden catalog product until staff approve it. After approval the product goes live and works with digital products (versioned downloads and license keys) if that module is on.
3. When an invoice is paid, a `sale` ledger entry is written for each line that belongs to a creator. The entry holds the gross amount, the commission and the net amount, and it is available after `hold_days`.
4. Refunds and disputes (`order.refunded`, `invoice.refunded`, `payment.disputed`) write a matching negative `refund` entry, so the balance can go negative and is recovered from later sales.
5. A creator requests a payout from the available balance, and the amount is debited right away. Staff then **Send** it through the provider, or mark it paid, rejected or failed. Rejected or failed payouts are credited back.

**Payout providers.**

- **Manual** (bank transfer or anything else). **Send** moves the payout to *processing*; you pay outside PeltyCMS and then mark it *paid*.
- **Stripe Connect** and **PayPal Payouts** are **stubs** in this release. They return `error.payout_provider_unconfigured` until `PELTY_STRIPE_CONNECT_KEY` / `PELTY_PAYPAL_PAYOUTS_CLIENT_ID` and the creator's account details are set. Even then they return `error.payout_provider_stub`, because no money is moved automatically yet. For now, pay those creators manually and mark the payouts paid.

Payout administration needs the **Affiliates** permission. Creator and listing review need **Catalog**.

### API

| Method | Path | Who |
|---|---|---|
| GET | `/api/v1/public/marketplace` | anyone (current mode) |
| GET | `/api/v1/public/creators`, `/api/v1/public/creators/{slug}` | anyone |
| GET/POST/PUT | `/api/v1/client/creator` | dashboard / apply / update profile |
| GET/POST | `/api/v1/client/creator/listings` | the creator |
| POST | `/api/v1/client/creator/listings/{id}/listed` | unlist or relist |
| POST | `/api/v1/client/creator/listings/{id}/versions` | the creator |
| GET | `/api/v1/client/creator/ledger` | the creator |
| GET/POST | `/api/v1/client/creator/payouts` | the creator |
| GET | `/api/v1/admin/creators`, POST `/api/v1/admin/creators/{id}/status` | staff (Catalog) |
| GET | `/api/v1/admin/creator-listings`, POST `/api/v1/admin/creator-listings/{id}/review` | staff (Catalog) |
| GET | `/api/v1/admin/creator-payouts`, POST `/{id}/send`, POST `/{id}/status` | staff (Affiliates) |

Events: `creator.applied`, `creator.payout_requested`, `creator.payout_paid`.

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

- **Banner**: a title, subtitle and background image (an `https://` URL or a path on your site such as `/uploads/banner.png`).
- **Accent color**: recolors buttons and focus rings in the store (theme default, emerald, violet, amber, rose, sky).
- **Package layout**: a dense list or cards.
- **Sidebar** on the left or right, with up to 12 widgets in the order you choose:
  - **Text box**: rules, about, contact. Shown as plain text (no HTML).
  - **Top customer**: the biggest spender today, this week, month, year or all time. Counts paid invoices in the default currency. Shown by game account player name (never an ID) or first name and last initial; the amount only if you switch it on.
  - **Recent payments** (1–10): who bought which packages, by product name only (no invoice details, no emails).
  - **Featured package**: one active product with its image and a link. Hidden products are skipped.
  - **Server status** and **Monthly goal**: from the community module (`server_community`, above); they only appear while `server_community` is on.
  - **Discord button**: a `https://discord.gg/…` or `https://discord.com/invite/…` link.

With the module off, the store keeps its plain look and these endpoints return 404.

### API

- `GET /api/v1/public/storefront` (no login): the banner, accent, layout, sidebar side and the widgets with their live data. Widgets with nothing to show (an unknown product, community off) are left out.
- `GET|PUT /api/v1/admin/storefront` (**Manage settings**): read or replace the settings. Invalid values return `422 error.storefront_invalid`.

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
