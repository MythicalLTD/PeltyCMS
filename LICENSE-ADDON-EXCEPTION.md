# PeltyCMS Addon Exception 1.0

**Effective date: 2026-11-19**
**Copyright holder:** MythicalSystems Studios LLC and the PeltyCMS contributors

PeltyCMS is licensed under the GNU Affero General Public License, version 3 or
(at your option) any later version ([`LICENSE`](./LICENSE),
SPDX `AGPL-3.0-or-later`). This file grants an additional permission under
section 7 of that license. SPDX identifier for the combination:
`AGPL-3.0-or-later WITH PeltyCMS-Addon-Exception-1.0` (a local identifier,
not on the SPDX list).

## Additional permission

As a special exception, the copyright holders of PeltyCMS give you permission
to create Addons and to use, copy, modify, convey and sell them under terms of
your choice, including proprietary terms, regardless of the license terms of
PeltyCMS, and to run and convey PeltyCMS together with such Addons, provided
that, for each Addon:

1. the Addon interacts with PeltyCMS only through the **Addon Interfaces**; and
2. the Addon does not contain any part of PeltyCMS other than the **Addon
   SDK** (separately MIT licensed) and the **Addon Runtime Libraries**, the
   latter only unmodified and only as compiled into the Addon as dependencies
   of the Addon SDK.

An Addon that meets these conditions is not a work based on PeltyCMS for the
purposes of the GNU AGPL, and you are not required to license it under the
GNU AGPL. You must still follow the GNU AGPL for PeltyCMS itself, and the
MIT license (keep its copyright notice) for the parts of the Addon SDK and
the license of the Addon Runtime Libraries for the parts you ship.

## Definitions

- **Addon** means a separately packaged extension for PeltyCMS (for example a
  WASM addon package `.zip` with an `addon.json` manifest, a theme, or a
  frame/plugin), together with its own assets and server-side services.
- **Addon Interfaces** means the documented ways PeltyCMS loads or talks to
  Addons: the WASM addon host ABI and its WIT/host functions, the addon
  manifest format, hooks, events, filters and mixins exposed to addons, the
  theme API, the frame SDK, and the PeltyCMS HTTP APIs.
- **Addon SDK** means the code published for building Addons, including
  `extensions/addons/sdk`, `extensions/addons/*-common`, `extensions/sdk`,
  `extensions/frame-sdk`, `extensions/examples`, `extensions/plugins` and
  `extensions/themes` (see the `LICENSE` file in each of those folders).
- **Addon Runtime Libraries** means the PeltyCMS crates `crates/pelty-core`,
  `crates/pelty-payments` and `crates/pelty-provisioning`, to the extent the
  Addon SDK (for example its `gateway` or `provisioning` features) or
  `extensions/addons/reg-common` compiles them into an Addon. When conveyed
  inside a qualifying Addon in object code form, they may be conveyed under
  the Addon's own terms; their source code remains available under the GNU
  AGPL as part of PeltyCMS.

## Scope

- This permission covers Addons only. **Modified versions of PeltyCMS itself**
  (the server, API, database layer, admin and client UI) remain under the GNU
  AGPL, including its section 13 duty to offer the source of a modified
  version to users who interact with it over a network.
- This permission does not grant any trademark rights.
- If you modify PeltyCMS, you may extend this exception to your version, but
  you are not obliged to do so. If you do not wish to do so, delete this
  exception statement from your version (GNU AGPL section 7).

This exception follows the model of the GNU GPL linking exception and the
GNU Classpath exception, adapted to PeltyCMS addons.
