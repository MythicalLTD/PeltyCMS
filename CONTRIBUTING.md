# Contributing to PeltyCMS

Thanks for helping. PeltyCMS is open source under the
[GNU AGPL v3 or later](./LICENSE).

## Before you start

- **Bugs and ideas:** open an issue on
  [MythicalLTD/PeltyCMS](https://github.com/MythicalLTD/PeltyCMS/issues).
- **Security issues:** do not open a public issue; e-mail legal@peltycms.com.
- **Addons:** addons can live in their own repository under any license (see
  [LICENSE-ADDON-EXCEPTION.md](./LICENSE-ADDON-EXCEPTION.md)). Start from
  `extensions/examples/addon-guide` (MIT).

## Contributor License Agreement (required)

Code, documentation and translation contributions need the
[Contributor License Agreement](./CLA.md). It lets MythicalSystems Studios LLC
keep PeltyCMS open source **and** offer it under commercial terms (PeltyCMS
Premium, commercial licenses), which pays for development. You keep your
copyright. Sign it by adding the CLA line from `CLA.md` to your first pull
request. Pull requests without it cannot be merged.

## Development

```bash
cargo dev                       # server + UI (see README)
cargo test --workspace          # Rust tests (needs PostgreSQL, see docs)
cd frontend && pnpm test        # frontend tests
cargo deny check licenses       # dependency licenses must stay AGPL-compatible
```

- Keep features optional (module or addon toggles) and add tests.
- New UI strings go into all six files in `locales/`.
- New dependencies must use a license listed in `deny.toml`.
- Write commit messages in the conventional style (`feat:`, `fix:`, `docs:`).
