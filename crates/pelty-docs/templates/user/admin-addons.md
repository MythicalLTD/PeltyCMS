# Addons, plugins and themes

Payment gateways, provisioning modules, registrars, captcha and single sign-on providers and integrations are addons: WebAssembly packages that run sandboxed inside PeltyCMS.

## Install an addon

Admin → Addons & integrations → **Addons** lists the installed addons, anything you uploaded and, on a source checkout, the zips built by `cargo xtask addons`. Release binaries ship without addons, so on a fresh production install you start by uploading the ones you need:

- **Upload** an addon zip (WebAssembly packages up to 64 MiB, others up to 10 MiB), or install from a configured addon registry.
- **Install** it. PeltyCMS shows what the addon asks for: its permissions and its sandbox capabilities (which hosts it may call, which secrets and data it may use, which events it hooks).
- **Approve dangerous capabilities.** Some capabilities need an explicit tick before install succeeds, for example `http.any` (call any host), `http.private` (call private networks), `money.modify`, `db.read`, `db.write`, `db.sql`, `fs`, `exec`, `auth.provider`, request-wide hooks and iframes. The approval is recorded in the audit log.
- **Enable** it and fill its **settings**. Secret settings are stored encrypted.

Disable stops the addon; uninstall removes it. Addons that change core API responses (mixins) can be switched off per addon with the mixin kill switch in the Addons page, without disabling the addon.

Every bundled addon has a reference page in the developer docs ([Bundled addons](developers/addons.md)).

## Plugins

Admin → Addons & integrations → **Plugins** manages plugins: settings rendered from the plugin's schema, enable/disable, and the webhook delivery log for plugins that receive events.

## Themes

Admin → Website → **Themes** switches the client-area theme. A theme (`extensions/themes/<id>/theme.json`) sets design tokens such as colours and fonts plus static assets; themes cannot run JavaScript, and token values are validated so they cannot inject CSS. **Website** manages the public site pages.
