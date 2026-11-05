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
