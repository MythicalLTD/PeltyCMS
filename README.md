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

## Licensing (how money and Free Tier work)

PeltyCMS is **source-available**, not OSI Open Source.

| Document | What it is |
|----------|------------|
| [LICENSE](./LICENSE) | Source Available License (SAL) 1.0 |
| [EULA.md](./EULA.md) | End User License Agreement (required to run) |
| [COMMERCIAL.md](./COMMERCIAL.md) | When to buy a Commercial License |

**Free in production** while you have **50 or fewer Active Clients** (customer
status `active`, not merged), and always free for non-production use.

**Above 50 Active Clients** you need a [Commercial License](./COMMERCIAL.md).
The panel keeps working and only reminds admins (Soft enforcement). Nothing is
feature-locked for overage alone.

Accept the EULA on first install (`eula.txt`, setup wizard, or `PELTY_EULA=true`).
Staff also confirm the License and EULA once in the admin area if that record
is missing.

Commercial sales: https://peltycms.com/pricing · sales@peltycms.com

> These legal files are product drafts for MythicalSystems Studios LLC. Have a
> lawyer review them before you publish a release or take payment.

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
saved. Headless:

```bash
pelty-server setup --yes --accept-eula \
  --db-url postgres://pelty:secret@localhost/pelty \
  --public-url http://localhost:8080 \
  --admin-email you@example.com
```

See `crates/pelty-docs` / the built `/docs` site for install details
(`admin-install`).

## Repository layout

```
crates/           Rust workspace (api, server, billing, wasm, …)
frontend/         Admin and client UI
extensions/       Addon packages and examples
migrations/       SQL migrations
locales/          UI translations
LICENSE EULA.md COMMERCIAL.md
```

## Contributing

Bug reports and addon work are welcome. By contributing you agree your patches
are licensed to MythicalSystems Studios LLC under the same License terms as the
rest of the repository (or under terms we publish for contributors).

## Trademark

PeltyCMS and MythicalSystems names and logos are trademarks of MythicalSystems
Studios LLC. Use them only as allowed in the License.

## Contact

- Product: https://peltycms.com
- Legal: legal@peltycms.com
- Sales: sales@peltycms.com
