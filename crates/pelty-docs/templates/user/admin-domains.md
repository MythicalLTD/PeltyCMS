# Domains and registrars

{{nav:admin:domains}}

## Selling domains

Set TLD prices in Admin → Domains (the setup checklist asks for at least one TLD price if you sell domains). Clients search and order domains from **Domain search** and the store; registrations, transfers and renewals are carried out by the domain jobs, not during the web request:

- `domains.process` (every 5 minutes) sends pending registrations and transfers to the registrar.
- `domains.daily` handles renewals and expiry.

Clients manage nameservers, contacts, DNS, auto-renew, EPP codes and domain addons themselves (see [Client guide: domains](client-domains.md)).

## Registrars

Registrars are addons of kind `registrar`. Install, then configure their API credentials in the addon settings:

{{addons:registrar}}

Registrant verification emails (where a registry needs them) are sent by PeltyCMS; clients confirm at `/verify-registrant`.
