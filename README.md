# PeltyCMS

Self-hosted billing and automation for hosting companies.

Rust API (axum, SQLx, wasmtime) and React admin/client UI (Vite, shadcn/ui).
Payment gateways, provisioning, registrars, captcha, and SSO ship as WASM
addons from **Admin → Addons**.

**Status:** under active development. Not a public release yet.

Release tags are **v-prefixed SemVer** (`v1.0.0`, `v1.0.0-rc.1`) — not bare
`1.0.0` like older MythicalDash cuts. Nightlies publish every day from
`develop` as `v0.1.0-nightly.YYYYMMDD` (plus a floating `nightly` release /
image). In **Admin → Updates** pick the Stable or Nightly channel; Docker
hosts get a pull command, binary hosts get `pelty-server update`.

```text
ghcr.io/mythicalltd/peltycms:v1.0.0
ghcr.io/mythicalltd/peltycms:latest      # stable only
ghcr.io/mythicalltd/peltycms:nightly
```

Release archives (also what `pelty-server update` downloads):

| Platform | Archive |
|----------|---------|
| Linux x64 / ARM64 | `pelty-server-<v>-{x86_64,aarch64}-unknown-linux-{gnu,musl}.tar.gz` |
| Windows x64 / ARM64 | `pelty-server-<v>-{x86_64,aarch64}-pc-windows-msvc.zip` |
| macOS Intel / Apple Silicon | `pelty-server-<v>-{x86_64,aarch64}-apple-darwin.tar.gz` |

## License

PeltyCMS is **free and open source software** under the
[GNU Affero General Public License v3.0 or later](./LICENSE)
(SPDX `AGPL-3.0-or-later`). No client cap, no feature lock.

| Document | What it is |
|----------|------------|
| [LICENSE](./LICENSE) | GNU AGPL v3 (full text) |
| [LICENSE-ADDON-EXCEPTION.md](./LICENSE-ADDON-EXCEPTION.md) | Addons, themes and plugins may use any license |
| [COMMERCIAL.md](./COMMERCIAL.md) | PeltyCMS Premium and commercial licenses |
| [EULA.md](./EULA.md) | Install notice and Premium terms (withdrawal, privacy of the license check) |
| [CONTRIBUTING.md](./CONTRIBUTING.md) / [CLA.md](./CLA.md) | How to contribute (CLA required) |
| `THIRD_PARTY_LICENSES.md` | Licenses of bundled dependencies (generated, shipped in release archives) |

The addon SDK (`extensions/addons/sdk`, `*-common`), `extensions/sdk`,
`extensions/frame-sdk`, the examples and the bundled themes are **MIT**.

**PeltyCMS Premium** (optional, monthly per installation, via
[MyMythical](https://my.mythicalsystems.org)): white-label, priority support and
higher limits. **PeltyCMS Serverless** is PeltyCMS hosted by us on its own
server, Premium included. Turn on the *PeltyCMS Premium*
module in Admin → Modules. If the licence check cannot reach MyMythical,
Premium keeps working and only shows a warning. Details in
[COMMERCIAL.md](./COMMERCIAL.md).

Accept the install notice on first start (`eula.txt`, setup wizard, or
`PELTY_EULA=true`). It confirms you read the license; it adds no conditions.

## Stack

- **Server:** `pelty-server` (API, static UI, docs, migrations, scheduler)
- **Database:** PostgreSQL 16+
- **Frontend:** `frontend/` (Vite + React)
- **Addons:** `extensions/addons/` (WASM packages)

## Quick start (development)

```bash
# PostgreSQL running, then:
cp .env.example .env   # if present; otherwise first run creates .env
cargo dev              # or: cargo xtask / pelty-server as documented in docs
```

First terminal start runs the setup wizard (License/EULA, database, URL, company,
admin, optional mail/captcha/gateways). Confirm the summary before anything is
saved. Headless (bring your own Postgres, or `--local-pg` to download one into
`data/pg` with no Docker):

```bash
pelty-server setup --yes --accept-eula \
  --db-url postgres://pelty:secret@localhost/pelty \
  --public-url http://localhost:8080 \
  --admin-email you@example.com
# production-style binary: pelty-server setup --yes --accept-eula --local-pg ...
```

Release archives ship `docs/` next to the binary. See `crates/pelty-docs` / the
built `/docs` site for install details (`admin-install`).

## Repository layout

```
crates/           Rust workspace (api, server, billing, wasm, …)
frontend/         Admin and client UI
extensions/       Addon packages and examples
migrations/       SQL migrations
locales/          UI translations
LICENSE LICENSE-ADDON-EXCEPTION.md COMMERCIAL.md EULA.md CONTRIBUTING.md CLA.md
```

## Contributing

Bug reports, pull requests and addons are welcome. Code contributions need a
signed [Contributor License Agreement](./CLA.md); see
[CONTRIBUTING.md](./CONTRIBUTING.md).

## Trademark

PeltyCMS and MythicalSystems names and logos are trademarks of MythicalSystems
Studios LLC. The AGPL does not grant trademark rights; factual references are fine.

## Contact

- Product: https://peltycms.com
- Legal: legal@peltycms.com
- Sales: sales@peltycms.com
