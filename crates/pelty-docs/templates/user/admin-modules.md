# Optional modules (gaming, digital products, marketplace)

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

- **Commands** run when a package is bought (`purchase`), renewed (`renewal`), expires or is suspended/cancelled (`expiry`), refunded (`refund`) or charged back (`chargeback`). Set them per package or globally for every package. Each command can target one server or every server whose store sells the package, can wait until the player is online, and can be delayed.
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

Reasons: `not_found`, `wrong_product`, `suspended`, `revoked`, `expired`, `domain_required`, `ip_required`, `hwid_required`, `activation_limit`, `not_activated` (with `"activate": false`). The IP defaults to the caller's address. Repeated unknown keys from one address are rate limited.
