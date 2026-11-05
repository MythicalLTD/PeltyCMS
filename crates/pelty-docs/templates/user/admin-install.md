# Planned installation (not released)

**PeltyCMS is not released yet.** There is no public download, image or installer today. This page describes how installation is planned to work at release, and the details may change.

At release, PeltyCMS is planned to be one server binary, `pelty-server`, plus a PostgreSQL 16+ database. The binary serves the API, the built frontend and these docs, runs the database migrations on start and runs the background scheduler, so no separate cron or worker is needed.

## Requirements

- PostgreSQL 16 or newer.
- A domain with TLS in front of PeltyCMS (a reverse proxy such as nginx or Caddy). The server listens on plain HTTP.
- For building from source: Rust with the `wasm32-wasip2` target, Node.js 20+ (its corepack provides pnpm). No make or bash needed, so it builds on Windows too.

## Docker Compose (planned)

The repository ships a `Dockerfile` and a `docker-compose.yml`. Put the secrets in a `.env` file next to the compose file (never commit it), then start it and create the first admin:

```bash
docker compose up -d
docker compose run --rm app admin create you@example.com
```

The compose file refuses to start until `.env` contains `PELTY_EULA=true`, which confirms you accept the PeltyCMS EULA (`EULA.md`). The container keeps its own state in the `appdata` volume (`/data`): a generated `.env` with the encryption key (used when `PELTY_ENCRYPTION_KEY` is not set), `eula.txt`, installed addons and backups. Back it up with the database.

{{file:docker-compose.yml:yaml}}

The image runs `pelty-server serve`, listens on `0.0.0.0:8080` (`PELTY_BIND`), and has a health check on `/healthz`. The compose file binds it to `127.0.0.1:8080` so only your reverse proxy can reach it.

## From source

```bash
cargo xtask build   # frontend, server binary, addon zips and these docs
mkdir /srv/pelty && cd /srv/pelty
/path/to/pelty-server           # first start in a terminal: setup wizard, then the server starts
```

The first start in a terminal runs a setup wizard. It asks for:

- the License and EULA (Free Tier ≤50 Active Clients; Soft overage notices; see LICENSE, EULA.md, COMMERCIAL.md);
- the database (tested live, can be created if missing);
- the listen address, public URL, reverse proxy and HTTPS;
- company name and email, default currency, language and time zone;
- the first administrator's name and email (generated or typed password, never echoed);
- optional outgoing email with a test email;
- optional captcha (built-in or a bundled provider with its keys);
- optional advanced settings (log level, sessions, sign-in rate limit, database pool, backups).

Before saving you see a summary per section and can edit any of them. It saves `.env` and `eula.txt`, applies the settings after the migrations, prints the next steps and keeps running. Run `pelty-server setup` to change these answers later.

### Behind a reverse proxy

The wizard's proxy question sets `PELTY_TRUSTED_PROXIES`: `127.0.0.1,::1` for nginx, Caddy or Apache on the same machine, the published Cloudflare ranges for Cloudflare, or your own IPs/CIDRs. Only requests from those addresses may set `X-Forwarded-For`, `X-Real-IP`, `CF-Connecting-IP` and `X-Forwarded-Proto`; the client IP then drives rate limits, sessions and the audit log. From any other address the headers are removed. More ranges can be added in **Admin > Security**. Use a public `https://` URL and answer yes to HTTPS so cookies are marked Secure.

### Uploads, background jobs and Redis

- **Upload limits**: `PELTY_UPLOAD_ATTACHMENT_MB` (50, also the maximum), `_AVATAR_MB` (2), `_KYC_MB` (20), `_DOWNLOAD_MB` (25), `_IMPORT_MB` (5) and `_ADDON_MB` (64, also the maximum). They set the request body limit and the size check of each upload. The per-file limits in General settings and KYC settings still apply below them. Raise your proxy's limit too (nginx `client_max_body_size`).
- **Scheduler**: every instance runs queued jobs (`PELTY_WORKER_CONCURRENCY` per batch, default 10). The built-in scheduler queues the due tasks (recurring billing and invoices, overdue notices, autopay, reminders, domain renewals, ticket escalation, mail piping, exchange rates, ...) every `PELTY_SCHEDULER_INTERVAL_SECS` (60). Per-task on/off and the schedule are under **Admin > Automation**: every task can get a five-field cron expression (company time zone) with a preview of its next runs, and keeps its built-in interval until you set one; the wizard can set the daily billing hour. With `PELTY_SCHEDULER=false`, run `pelty-server cron run` from system cron every minute (`--schedule-only` queues without running). Tasks are deduplicated, so overlapping runs are harmless. `pelty-server cron run <task>` queues and runs one task now (for example `billing.recurring`); in the server console, `cron list`, `cron run <task>`, `cron enable|disable <task>`, `cron schedule <task> "<m h dom mon dow>"` (or `default`) and `cron history <task> [n]` manage the same tasks, including scheduled addon jobs and addon cron jobs (`<addon-id>:<name>`). A task whose runs fail `PELTY_CRON_MAX_FAILURES` times in a row (default 5; retries of one run count once) is disabled automatically ("tripped"): it is audited, staff with the jobs permission get an email, and it shows as tripped in Admin > Automation and `cron list` until you reset it there or with `cron reset <task>`. Runs missed while no scheduler was running are skipped; set a task to `run_once` (Admin > Automation, or `cron missed <task> run_once`) to run it once when a scheduler starts or takes over as leader.
- **Several instances**: with `PELTY_REDIS_URL`, an admin change on one instance (settings, security policy, SMTP, translations, theme, plugins, addons and gateways) is published on Redis and the other instances reload it from the database; console `cluster` lists the live instances and the scheduler leader. Addon packages must be present on every instance (shared `extensions/addons` folder or the same image).
- **Redis (optional)**: with `PELTY_REDIS_URL` (any Redis 2.6 or newer), rate-limit counters are shared by all instances and only one instance at a time (the holder of a lease renewed each pass) runs the scheduler. If Redis is unreachable each instance falls back to local counters and scheduling. The console `status` command shows the Redis health. In Docker Compose: `docker compose --profile redis up` and `PELTY_REDIS_URL=redis://redis:6379/0`.

### Without questions

```bash
pelty-server setup --yes --accept-eula --db-url postgres://pelty:secret@localhost/pelty \
  --public-url https://billing.example.com --admin-email you@example.com
```

This uses defaults for everything not given (`--bind`, `--admin-password`, `--admin-name`, or `PELTY_ADMIN_EMAIL` / `PELTY_ADMIN_PASSWORD`), creates the database if needed and prints a generated password once. To configure by hand instead, use `--no-setup` and edit the files:

```bash
/path/to/pelty-server --no-setup   # creates .env, data folders and eula.txt, then exits
# edit .env (DATABASE_URL, PELTY_PUBLIC_URL, ...), read EULA.md, set eula=true in eula.txt
/path/to/pelty-server admin create you@example.com
/path/to/pelty-server --no-setup   # serve (runs migrations first)
```

Under systemd or Docker there is no terminal, so the wizard never runs there. In a container the server prints a hint instead; run the wizard once with a terminal attached: `docker run -it --rm -v pelty:/data <image> pelty-server setup` or `docker compose run --rm app pelty-server setup`.

Like a Minecraft server, `pelty-server` sets up the folder it starts in. The bootstrap never overwrites a file (only the wizard edits `.env`, after you confirm), and every created file or folder is printed. `serve` exits until `eula.txt` says `eula=true` or `PELTY_EULA=true` is set. Started in a terminal it opens a console (`help`, `status`, `addons install <zip>`, `admin reset-password <email>`, `log level debug`, `stop`, ...). The console is off automatically under systemd or Docker, and `--no-console` turns it off explicitly.

`admin create <email> [--name "Display Name"]` reads the password from `PELTY_ADMIN_PASSWORD` or the first line of standard input. `cargo xtask help` lists every developer command; see [CLI](developers/cli.md) for all commands, including the WHMCS, Blesta and Paymenter importers.

## Migrating from WHMCS

`pelty-server import whmcs` copies a WHMCS installation into PeltyCMS. It works from a plain SQL export, so no MySQL server has to be reachable from PeltyCMS:

```sh
# on the WHMCS host
mysqldump --single-transaction --default-character-set=utf8mb4 whmcs > whmcs.sql
# on the PeltyCMS host ($cc_encryption_hash is in WHMCS' configuration.php)
export WHMCS_CC_ENCRYPTION_HASH='...'
pelty-server import whmcs --file whmcs.sql --dry-run   # report only, nothing is written
pelty-server import whmcs --file whmcs.sql
```

The console has the same command (`import whmcs --file whmcs.sql --dry-run`). Prefer the SQL export path: it is what CI covers (`crates/pelty-import/tests` fixtures).

### Live MySQL (optional)

A binary built with `--features whmcs-mysql` can also read a live database:

```bash
cargo build -p pelty-server --release --features whmcs-mysql
# Dry-run against a staging WHMCS DB (nothing is written to PeltyCMS):
pelty-server import whmcs --dsn 'mysql://user:pass@127.0.0.1:3306/whmcs' \
  --cc-hash "$WHMCS_CC_ENCRYPTION_HASH" --dry-run
```

Or set `WHMCS_DSN` / `WHMCS_CC_ENCRYPTION_HASH`. The live loader uses the same column map as the dump importer. Smoke-test checklist before a production cutover:

1. `--dry-run` on a copy of production WHMCS; confirm created/skipped counts and warnings.
2. Real import into an empty PeltyCMS database; spot-check clients, services, invoices, tickets.
3. Client login with a WHMCS password; first success upgrades the hash to Argon2id.
4. Re-enter server API keys if the run was without `cc_encryption_hash` (servers import disabled).

What is imported: currencies, client groups, clients and their contacts, product groups, products and pricing, configurable options, product addons, servers, services (linked to their server, with the control-panel username), service addons, custom fields, domains, invoices with their items, transactions and refunds, client credit, ticket departments, tickets and replies, announcements, knowledgebase categories and articles, promotions (percentage and fixed-amount; other types are reported and skipped) and tax rules.

* **Dry run first.** `--dry-run` runs the whole import in a transaction and rolls it back, so its report (created / already imported counts and warnings) is exactly what the real run will do.
* **Re-runnable.** Every imported row is recorded, so running the import again only adds what is new. An invoice whose number already exists is skipped with a warning.
* **Passwords.** WHMCS login hashes (bcrypt from WHMCS 8 `tblusers`, or the old `md5(salt + password)`) are kept. Clients sign in with their WHMCS password, and the first successful login replaces the hash with Argon2id. Accounts with any other hash type must use "forgot password".
* **Server credentials.** Server passwords are decrypted with the WHMCS `cc_encryption_hash` (`--cc-hash`, or `WHMCS_CC_ENCRYPTION_HASH`, which keeps it out of the process list) and stored encrypted with `PELTY_ENCRYPTION_KEY`. An API token (`accesshash`) is preferred over the password. Without the hash or the key, servers are imported disabled: re-enter their key in Admin > Servers.
* **Card data is never imported.** Stored card numbers, bank details and `tblcreditcards` / `tblpaymethods` are not read. Clients add a payment method again.

## Required configuration

Only a few environment variables are needed to run in production:

{{env:DATABASE_URL,PELTY_ENCRYPTION_KEY,PELTY_PUBLIC_URL,PELTY_COOKIE_SECURE,PELTY_BIND}}

`PELTY_ENCRYPTION_KEY` encrypts stored secrets (server API keys, gateway and addon secrets). Keep it safe and keep it the same across restarts: secrets saved with one key cannot be read with another.

Everything else (company details, currencies, mail, gateways, tax, security policy) is configured in the admin area and stored in the database. Continue with [First-run setup](admin-setup.md).

## All environment variables

{{env}}

Generated from the code; the developer reference [Configuration](developers/configuration.md) also shows which file reads each variable.
