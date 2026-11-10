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
