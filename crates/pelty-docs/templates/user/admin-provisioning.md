# Servers and provisioning

Provisioning creates, suspends, unsuspends and terminates accounts on control panels (cPanel, Plesk, Pterodactyl, Proxmox and others) when orders are paid, invoices go overdue or services are cancelled.

## Set up

- Install the provisioning addon for your panel (Admin → Addons & integrations → **Addons**).
- Add the panel in Admin → Products & services → **Servers**. API keys are write-only: you can replace them but never read them back.
- Edit a product, choose the module and fill the product fields the module asks for.

Each module's server and product fields are listed on its page in the developer docs (linked below).

## Watch it run

- **Module queue** shows provisioning actions waiting to run or that failed, and lets you retry them.
- **Module log** records every call to a panel.
- On a service, staff can run module actions (create, suspend, unsuspend, terminate, plus module-specific custom actions) and see the status.

Clients can sign in to their panel from the service page with a one-time token (single sign-on), when the module supports it.

## Provisioning modules

{{addons:provisioning}}

Panel addresses that resolve to link-local addresses are refused unless `PELTY_PROVISION_ALLOW_LINK_LOCAL` is set; keep it unset in production.
