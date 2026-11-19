# First-run setup

After the first sign-in the admin dashboard shows a **Finish setting up your store** checklist. It is computed from the database, so a step turns done as soon as the setting exists. Required steps keep the checklist on the dashboard until they are done; you can hide it with **Hide from dashboard** (Admin → Getting started brings it back).

Each step opens its settings page:

- License and EULA: `/admin/getting-started`
- Company: `/admin/general-settings`
- Currency: `/admin/currencies`
- Outgoing email: `/admin/mail-settings`
- Payment gateway: `/admin/gateways`
- First product: `/admin/products`
- Tax (optional): `/admin/tax`
- Domains (optional): `/admin/domains`
- Provisioning server (optional): `/admin/servers`

The first required step is **License and EULA**. Accepting records that you read the license notice for this deployment. PeltyCMS is free and open source under the GNU AGPL v3 or later: no client cap and no feature lock. If you modify PeltyCMS and serve it over a network, offer your users the source (AGPL section 13). Paid extras are optional (PeltyCMS Premium, see `COMMERCIAL.md` and Admin → Premium). See `LICENSE`, `EULA.md` and `COMMERCIAL.md` in the install, or Admin → Getting started.

{{wizard}}

## Company and general settings

Admin → System & settings → **General settings** holds the company name and contact email (used on invoices and emails), the system URLs, maintenance mode, localisation, ordering and support defaults, and custom head/body code. The sections are:

{{general}}

Each field is listed in the developer reference [Settings keys](developers/settings-keys.md#general-settings).

## Outgoing email

Admin → System & settings → **Mail (SMTP)** sets the SMTP server. Until it is configured clients receive no invoices or notifications. See [Email and notifications](admin-email.md).

## Next steps

- Create products: [Products and orders](admin-catalog.md).
- Enable a payment gateway: [Billing and payments](admin-billing.md).
- Sell domains: [Domains and registrars](admin-domains.md).
- Provision hosting automatically: [Servers and provisioning](admin-provisioning.md).
- Lock down staff accounts: [Staff security](admin-security.md).
