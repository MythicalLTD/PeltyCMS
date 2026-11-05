# Getting started

## Requirements

- Rust stable with the `wasm32-wasip2` target (`rustup target add wasm32-wasip2`) for building addons.
- PostgreSQL 14 or newer: an installed server, Docker (`cargo dev` starts a `postgres:16` container), or nothing at all (`cargo dev` downloads PostgreSQL 16 binaries into `data/pg`; Linux and macOS on x86_64 or arm64, Windows x64).
- Node.js 20+ with **pnpm** for the frontend (corepack, bundled with Node, provides the pinned pnpm).

Every developer command is `cargo xtask <command>` (plain Rust, so Windows, macOS and Linux behave the same; no make or bash). `cargo xtask help` lists them; the [CLI](cli.md) page has the table.

## Develop

```sh
cargo xtask addons    # build the WASM addon zips (once, and after addon changes)
cargo dev             # Postgres + backend + Vite on http://localhost:5173, Ctrl+C stops all
cargo xtask stop      # stop a dev run left behind in another terminal
```

`.env` is created from `.env.example` on first run; `DATABASE_URL` there decides which PostgreSQL is used.

**macOS** (Apple silicon or Intel): `brew install rustup node`, put `$(brew --prefix rustup)/bin` on PATH (the formula is keg-only; or use the installer from https://rustup.rs), `rustup default stable`, `rustup target add wasm32-wasip2`. PostgreSQL is optional: `brew install postgresql@16` (keg-only, found without PATH changes), Postgres.app, Docker Desktop or OrbStack; with none of them `cargo dev` downloads PostgreSQL 16 for your CPU into `data/pg`. To use a Homebrew server already running as a service, set `DATABASE_URL=postgres://<your user>@127.0.0.1:5432/peltycms` and `createdb peltycms`. Run `cargo dev` as yourself, not with `sudo`.

## Run it

```sh
cargo xtask build     # frontend, server and docs
mkdir run && cd run   # the server sets up the folder it starts in
../target/release/pelty-server        # in a terminal: setup wizard, then the server starts
```

The wizard asks, with defaults you accept with Enter:

1. The EULA.
2. The PostgreSQL connection, as a URL or as separate fields. It tests the connection, offers a retry, and can create a missing database.
3. The listen address, public URL, the reverse proxy in front (none, local nginx/Caddy/Apache, Cloudflare or a custom list) and whether HTTPS is used.
4. Company name and email, default currency, language and time zone (lists you can filter by typing).
5. The first administrator's name and email. The password is generated and printed once, or typed twice, rated and checked against the policy.
6. Optional outgoing email, with a test email.
7. Optional captcha: built-in, or a bundled provider with its keys.
8. Optional advanced settings: log level, JSON logs, session lifetimes, sign-in rate limit, database pool, backups, public docs, upload limits, scheduler or system cron (`pelty-server cron run`), daily billing hour, and Redis for multi-instance setups.

The summary is grouped by section with secrets masked; you can edit any section before saving, or cancel. It then writes `.env` (keeping comments, mode 0600) and `eula.txt`, runs the migrations, saves the settings, creates the admin, shows the next steps and starts serving. `pelty-server setup` reruns it and offers to start the server. `--no-setup` skips it.

Without questions (CI, scripts):

```sh
../target/release/pelty-server setup --yes --accept-eula --db-url postgres://postgres@localhost/pelty --admin-email admin@example.com
```

`--bind`, `--public-url`, `--admin-password` and `--admin-name` are optional (`PELTY_ADMIN_EMAIL` / `PELTY_ADMIN_PASSWORD` work too); a missing password is generated and printed once.

Without a terminal, or with `--no-setup`, configure the folder by hand:

```sh
../target/release/pelty-server --no-setup   # writes .env and eula.txt, then exits
$EDITOR .env                          # DATABASE_URL (PELTY_ENCRYPTION_KEY is already generated)
sed -i 's/eula=false/eula=true/' eula.txt   # after reading EULA.md (or PELTY_EULA=true)
PELTY_ADMIN_PASSWORD='a long strong password' ../target/release/pelty-server admin create admin@example.com
../target/release/pelty-server --no-setup   # migrations, then serve
```

The first start creates what is missing and prints each item: a commented `.env` with a new `PELTY_ENCRYPTION_KEY`, the data folders (`data/addons`, `extensions/themes`, `extensions/plugins`, `locales` with the default translations, `backups`) and `eula.txt` with `eula=false`. Nothing that exists is overwritten. `serve` exits with an error until the EULA is accepted; other sub-commands run anyway. `.env` values apply only where the environment leaves a variable unset or empty.

In a terminal, `serve` opens a console: type `help` for the commands (`status`, `addons ...`, `admin create`, `admin reset-password`, `users list`, `migrate`, `reload`, `log level`, `stop`). `--no-console` / `--console` override the terminal check. Ctrl+C stops the server; Ctrl+D closes only the console.

Every setting is an environment variable. The full list, with defaults, is in [Configuration](configuration.md). Server sub-commands are in [CLI](cli.md).

## The docs themselves

`cargo xtask docs` runs `cargo run -p pelty-docs -- target/docs` and writes HTML, Markdown and JSON. The server publishes that folder at `/docs`:

- `PELTY_DOCS_DIR` sets the folder (default `target/docs`).
- `PELTY_DOCS_PUBLIC=true` opens the docs to everyone. By default only signed-in staff can read them.

Staff open them from **Developer docs** in the admin sidebar.

## What you can extend

- **WASM addons** (`engine: "wasm"`): gateways, provisioning modules, registrars, captcha, SSO and general integrations. They run sandboxed and can only use the capabilities an admin approved. Start with [Write a plugin in Rust](plugin-rust.md).
- **Webhook plugins**: an external HTTPS service receives signed domain events. See [Events and hooks](events-guide.md).
- **Themes**: design tokens, a logo, a stylesheet or a complete frontend bundle. See [Themes](themes.md).
