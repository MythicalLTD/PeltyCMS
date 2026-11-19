# PeltyCMS Addon Exception 1.0

> **Draft.** This additional permission has not been reviewed by a lawyer yet.
> Have qualified counsel (Austria/EU) review it before you rely on it.

PeltyCMS is licensed under the GNU Affero General Public License, version 3 or
(at your option) any later version ([`LICENSE`](./LICENSE),
SPDX `AGPL-3.0-or-later`).

As an **additional permission under section 7 of the GNU AGPL version 3**, the
copyright holders of PeltyCMS give you permission to create, use, convey and
sell **Addons** under license terms of your choice, including proprietary
terms, without those Addons becoming subject to the GNU AGPL, provided that:

1. the Addon interacts with PeltyCMS only through the **Addon Interfaces**; and
2. the Addon does not include, copy or modify code from PeltyCMS other than the
   **Addon SDK** (MIT licensed on its own) and the **Addon Runtime Libraries**
   compiled into the Addon as dependencies of the Addon SDK, unmodified.

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
  `extensions/addons/reg-common` compiles them into an Addon. They stay under
  the GNU AGPL as part of PeltyCMS; this exception only lets you ship them
  inside an Addon under the Addon's own terms, the way a compiler runtime
  library exception works.

## Scope

- This permission covers Addons only. **Modified versions of PeltyCMS itself**
  (the server, API, database layer, admin and client UI) remain under the GNU
  AGPL, including its section 13 duty to offer the source of a modified
  version to users who interact with it over a network.
- Combining PeltyCMS with an Addon does not change the license of either.
- When you convey a modified version of PeltyCMS you may keep this additional
  permission or remove it, as allowed by section 7 of the GNU AGPL.
