# Optional modules (gaming, digital products, marketplace)

PeltyCMS is a hosting and billing panel out of the box. The same install can also run a game server store (Minecraft, FiveM/RedM, Rust and others, like Tebex or CraftingStore), sell digital products with licenses, or host a creator marketplace (like BuiltByBit). Each of these is an **optional module**: nothing changes until you switch it on, and you can combine them freely.

## Switching modules on and off

Go to Admin → System → **Modules** (`settings.manage`). Each card is one module with a switch.

- A fresh install has every module **off**. The client area, store and admin look exactly like a classic hosting panel.
- Some modules need another one first (shown as **Requires**). For example, in-game delivery needs game profiles, and the creator marketplace needs digital products.
- You cannot switch off a module that another enabled module needs (shown as **Needed by**). Switch that one off first.
- A disabled module hides its admin pages, client pages and navigation, and its API answers `404 Not Found` as if it did not exist. Data is kept, so switching it back on restores everything.

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
