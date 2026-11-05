# Addons, plugins and themes

Payment gateways, provisioning modules, registrars, captcha and single sign-on providers and integrations are addons: WebAssembly packages that run sandboxed inside PeltyCMS.

## Install an addon

Admin → Addons & integrations → **Addons** lists the installed addons, anything you uploaded and, on a source checkout, the zips built by `cargo xtask addons`. Release binaries ship without addons, so on a fresh production install you start by uploading the ones you need:

- **Upload** an addon zip (WebAssembly packages up to 64 MiB, others up to 10 MiB), or install from a configured addon registry.
- **Install** it. PeltyCMS shows what the addon asks for: its permissions and its sandbox capabilities (which hosts it may call, which secrets and data it may use, which events it hooks).
- **Approve dangerous capabilities.** Some capabilities need an explicit tick before install succeeds, for example `http.any` (call any host), `http.private` (call private networks), `money.modify`, `db.read`, `db.write`, `db.sql`, `fs`, `exec`, `auth.provider`, request-wide hooks and iframes. The approval is recorded in the audit log.
- **Enable** it and fill its **settings**. Secret settings are stored encrypted.

## Official addon registry

Admin → Addons → **Marketplace** (registry) can point at an HTTPS index JSON:

```json
{ "addons": [ { "id": "…", "name": "…", "kind": "gateway", "version": "0.1.0",
  "url": "https://…/addon.zip", "sha256": "<64 hex>", "author": "…", "description": "…" } ] }
```

PeltyCMS downloads the zip, checks `sha256`, then runs the same capability review as a manual upload. Signed addons are verified against Admin → Addons → **Trust** (see [Addon signing](developers/addon-signing.md)).

A starter index for MythicalSystems official packages lives in the source tree as `extensions/addons/registry/official.example.json`. Replace the zero `sha256` placeholders with the digests of the zips from `cargo xtask addons` before publishing the index. Host the JSON and the zips on HTTPS; set the registry URL under Addons → Marketplace → Registry settings.

Disable stops the addon; uninstall removes it. Addons that change core API responses (mixins) can be switched off per addon with the mixin kill switch in the Addons page, without disabling the addon.

Every bundled addon has a reference page in the developer docs ([Bundled addons](developers/addons.md)).

## Vendor catalog (MarketConnect-style)

Admin → Products & services → **Marketplace** syncs SKUs from a vendor provisioning server (`prov-vendor` / `prov-ssl-reseller`). Import a SKU into the product catalog with a markup; PeltyCMS then sells and provisions it like any other product. Start with SSL certificates via **SSL reseller**, then add further upstream catalogs as they become available. This is the PeltyCMS counterpart to WHMCS MarketConnect — content is yours to configure, not a locked branded store.

A starter catalog (DV/wildcard SSL plus site-builder and email upsells) lives at `extensions/addons/registry/vendor-catalog.example.json`. Point your vendor module at a server that returns that shape, sync, then import SKUs with a markup.

## Regional payment gateways

Beyond Stripe/PayPal, bundled gateways include **Authorize.net** (Accept Hosted, US-centric), **Netopia Payments** (Romania, RON/EUR), and **Mollie**/Adyen-style redirects for broader EU coverage. Install them from the official registry or upload the zips from `cargo xtask addons`.

## Plugins

Admin → Addons & integrations → **Plugins** manages plugins: settings rendered from the plugin's schema, enable/disable, and the webhook delivery log for plugins that receive events.

## Themes

Admin → Website → **Themes** switches the client-area theme. A theme (`extensions/themes/<id>/theme.json`) sets design tokens such as colours and fonts plus static assets; themes cannot run JavaScript, and token values are validated so they cannot inject CSS. **Website** manages the public site pages.
