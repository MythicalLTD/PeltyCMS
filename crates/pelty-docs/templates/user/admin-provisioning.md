# Servers and provisioning

Provisioning creates, suspends, unsuspends and terminates accounts on control panels (cPanel, Plesk, DirectAdmin, Pterodactyl, Proxmox and others) when orders are paid, invoices go overdue or services are cancelled.

## Set up

- Install the provisioning addon for your panel (Admin → Addons & integrations → **Addons**).
- Add the panel in Admin → Products & services → **Servers**. API keys are write-only: you can replace them but never read them back.
- Edit a product, choose the module and fill the product fields the module asks for.

Each module's server and product fields are listed on its page in the developer docs (linked below).

## Watch it run

- **Module queue** lists services whose last provisioning action **failed** (`provision_status = failed`). Retry re-enqueues the failed action. In-flight jobs are not listed here; use **Jobs** for the live queue.
- **Module log** records every call to a panel (success and failure). If a log row cannot be written, the server logs an error so failures are not silent.
- On a service, staff can run module actions (create, suspend, unsuspend, terminate, change package, plus module-specific custom actions such as **change password**) and see the status.
- Transient panel errors retry; configuration and most 4xx-style panel rejections are permanent and move the service to failed after the last attempt. Staff with `services.manage` get a mail when an action dies.

Clients can sign in to their panel from the service page with a one-time token (single sign-on) when the module supports it:

- **cPanel / WHM** creates a real session token via the API.
- **DirectAdmin** and **Plesk** use a configured `sso_url` template that must include `{token}` (and typically `{user}`). Leave it empty to hide SSO for that server.

## Custom-field kit

Every product and game store package has **Checkout fields** (Admin → Products → a product → **Checkout fields**, or Game servers → **Store packages** → **Checkout fields**). The same kit feeds the checkout form, the provisioning module and in-game delivery commands.

- **Field types**: text, long text, number, dropdown, checkbox, e-mail and password. Each field can be required or optional, have a help text and (text, long text, e-mail, password) a format regex. Dropdowns need at least one choice. Admin-only fields are never shown to the buyer.
- **Presets**: one click adds a ready set of fields; fields the package already has (same key, or the same player ID) are skipped, so applying a preset twice adds nothing. Presets for the games your store sells are listed first as **Suggested**; the others stay available.
  - **FiveM / RedM player**: Rockstar licence (required), Cfx.re id, SteamID64, Discord id, player name.
  - **Minecraft: Java player**: username (required), UUID, Discord id.
  - **Minecraft: Bedrock player**: gamertag (required), XUID, Discord id.
  - **Steam game player** (Rust, Garry's Mod, ARK, CS2, Unturned): player name, SteamID64 (required), Discord id.
  - **Account login**: account username, account password, account e-mail, for a package that creates an in-game or panel account. Combine it with a game preset.
  After applying, rename, reorder (arrows), make fields required or optional, delete or add more.
- **Keys**: every field has a key (`license`, `account_password`, `rank`...). It is filled from the player ID or the name when left empty, can be changed, must be `a-z`, digits and `_`, and is unique per package. Fields declared by the product's module keep the module's key.
- **What the module needs**: when the product uses a provisioning module that lists the kit keys it reads, the editor shows them with an **Add missing** button.
- **Checkout**: the buyer sees the fields with the right input (e-mail keyboard, hidden password), player IDs filled from their game accounts, and the server checks required fields, e-mail format, numbers, dropdown choices and the regex. Signed-in and guest checkouts behave the same.

**Passwords.** A password is collected once at checkout. It is stored encrypted with `PELTY_ENCRYPTION_KEY` (set it; without a key the value is stored as typed but still hidden), never returned by the API (service views show `********`), never sent to the dynamic checkout form logic, and never written to logs or the delivery queue. A password is not a player ID and is never saved on the game account.

**How modules read the kit.** On create, suspend, unsuspend, terminate, change package and custom actions the module gets `ServiceCtx.custom_fields` with every field of the product under its key (and under its field id), passwords decrypted for that call only. In Rust: `svc.field("license")`, `svc.field("account_password")`. Webhook and HTTPS-agent modules can put them in a body template as `{{custom.license}}` or `{{custom.account_password}}`. A module can:

- declare its own fields with `checkout_fields()` (kinds `text`, `textarea`, `number`, `select`, `checkbox`, `email`, `password`); they are created on the product when the module is chosen and keep the module's key; `checkout_config()` can still filter them per selection;
- list the kit keys it reads with `uses_fields()` (WASM descriptor: `"uses_fields": [{"key": "license", "name": "Rockstar licence", "kind": "text", "identity_key": "license", "required": true}]`); staff then add the missing ones in one click.

Modules must use password values for the remote call only and never log or store them.

## Shared hosting panels (cPanel, DirectAdmin, Plesk)

These three cover the daily shared-hosting lifecycle: create, suspend, unsuspend, terminate, change package, and change password (custom action).

Not supported yet on these modules:

- Disk / bandwidth **usage sync** back into PeltyCMS (usage stays empty).
- Panel **reseller** account catalogs (sell reseller packages as first-class products).
- Automatic account **transfer** between servers (re-create on the target, then terminate the source).

Prefer testing create → change password → suspend → unsuspend → change package → terminate on a staging server before going live.

## Provisioning modules

{{addons:provisioning}}

Panel addresses that resolve to link-local addresses are refused unless `PELTY_PROVISION_ALLOW_LINK_LOCAL` is set; keep it unset in production.
