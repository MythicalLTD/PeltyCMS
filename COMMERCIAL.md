# PeltyCMS: open source, Premium and commercial licenses

**Licensor:** MythicalSystems Studios LLC
**Product:** PeltyCMS

This page summarises the licensing model. The binding texts are the
[GNU AGPL](./LICENSE), the [Addon Exception](./LICENSE-ADDON-EXCEPTION.md)
and, for Premium, Part B of [`EULA.md`](./EULA.md) (also published at
https://www.mythicalsystems.org/peltycms-premium).

## The short version

| | Free (open source) | PeltyCMS Premium | Commercial license |
|---|---|---|---|
| License of the code | GNU AGPL v3 or later | GNU AGPL v3 or later | Custom terms instead of the AGPL |
| Production use, any number of clients | Yes | Yes | Yes |
| All core features | Yes | Yes | Yes |
| White-label (hide "Powered by PeltyCMS") | Edit the code yourself | One switch in Admin → Premium | As agreed |
| Priority support | Community | Yes | As agreed |
| Higher limits on MythicalSystems services | No | Yes | As agreed |
| Keep modifications closed while serving users over a network | No (AGPL section 13) | No (AGPL section 13) | Yes |
| Price | Free | Monthly, per installation | On request |

There is **no client cap** and **no feature lock** in the open source edition.

## Open source (GNU AGPL v3 or later)

PeltyCMS is free software. You can use, study, change and share it under the
[GNU Affero General Public License](./LICENSE). The most important rule: if
you run a **modified** PeltyCMS and let people use it over a network, you must
offer them the source code of your modified version (AGPL section 13).

Addons, themes and plugins that use only the addon interfaces may use any
license, including proprietary ones. See the
[Addon Exception](./LICENSE-ADDON-EXCEPTION.md). The addon SDK, the frame SDK,
the theme SDK, the examples and the bundled themes are MIT licensed.

## PeltyCMS Premium

Premium is an optional monthly subscription **per installation**, sold through
the MyMythical platform (https://my.mythicalsystems.org). The default price is
EUR 9 per installation per month plus VAT/taxes (added at checkout); the
current price is shown on the Premium page in MyMythical.

Premium includes:

- **White-label:** hide "Powered by PeltyCMS" on the public site with one switch.
- **Priority support** through MyMythical support tickets.
- **Higher limits** on MythicalSystems services used by your installation
  (marketplace/registry API rate limits, open support tickets).

How it works:

1. Turn on the **PeltyCMS Premium** module in Admin → Modules.
2. Link the installation to your MyMythical team in Admin → Marketplace
   (MythicCloud link).
3. Subscribe in MyMythical (team → PeltyCMS Premium), one unit per installation.
4. Admin → Premium → **Check now**. The installation receives a signed,
   short-lived entitlement and refreshes it every day.

**Soft verification.** If PeltyCMS cannot reach MyMythical or the entitlement
has expired, Premium features **keep working** and staff see a warning. Premium
switches off only when MyMythical answers that the installation has no active
Premium subscription (for example after you cancel).

The Premium module is off by default. While it is off, PeltyCMS does not
contact MyMythical for Premium.

Consumers in the EU/EEA have a 14-day right of withdrawal; the instructions
and a model withdrawal form are in Part B of [`EULA.md`](./EULA.md). Cancel
any time, with effect at the end of the paid period.

## PeltyCMS Serverless (managed hosting)

Don't want to run a server? **Serverless** is PeltyCMS hosted by us: your own
virtual server (Akamai/Linode) in the region you pick, the official PeltyCMS
Docker image, PostgreSQL, automatic HTTPS for your domain, nightly snapshots
of your database and uploads and **Premium included**. Ordered and billed
monthly through MyMythical (team → PeltyCMS Serverless).

| Plan | Memory / CPU / disk / traffic | Price per month (plus VAT/taxes) |
|---|---|---|
| Serverless 1 GB | 1 GB / 1 vCPU / 25 GB / 1 TB | EUR 8.99 |
| Serverless 2 GB | 2 GB / 1 vCPU / 50 GB / 2 TB | EUR 18.99 |
| Serverless 4 GB | 4 GB / 2 vCPU / 80 GB / 4 TB | EUR 37.99 |

Checkout runs on Tebex, which adds the VAT or sales tax of your country on
top; Sao Paulo and Jakarta cost 40% and 20% more. A free trial on the 1 GB
plan may be offered (one per team). Hosting is best effort with no SLA; terms
are in [EULA.md](./EULA.md) Part B, section B9.

## Commercial license

If the AGPL does not fit you, for example because you want to ship a modified
PeltyCMS (or a product built on it) without publishing your changes, you can
buy a commercial license that replaces the AGPL for your organisation.
Contact sales@peltycms.com.

## Contact

sales@peltycms.com · legal@peltycms.com · Premium orders and withdrawal:
support@mythical.ltd · https://peltycms.com
