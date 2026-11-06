//! PeltyCMS server binary.
//!
//! Usage:
//!   pelty-server [serve] [--console | --no-console] [--no-setup | --yes]
//!                                        run the HTTP server (runs migrations first);
//!                                        the interactive console (`help` lists its
//!                                        commands) runs when stdin is a terminal
//!   pelty-server setup                   interactive setup wizard (database, web and
//!                                        reverse proxy, company, first admin, SMTP,
//!                                        captcha, advanced); saves .env and eula.txt,
//!                                        applies settings, offers to start the server
//!   pelty-server setup --yes [--accept-eula] [--local-pg] [--db-url <url>] [--bind <addr>]
//!                [--public-url <url>] [--admin-email <email>] [--admin-password <pw>]
//!                [--admin-name <name>]
//!                                        the same without questions: defaults, flags,
//!                                        then PELTY_ADMIN_EMAIL / PELTY_ADMIN_PASSWORD;
//!                                        `--local-pg` downloads and starts PostgreSQL
//!                                        under data/pg (no Docker)
//!   pelty-server pg start|stop|status|download
//!                                        manage the optional local PostgreSQL sidecar
//!   pelty-server migrate                 run migrations and exit
//!   pelty-server cron run [<task> | --schedule-only]
//!                                        one scheduler pass, then run queued jobs
//!                                        (for system cron with PELTY_SCHEDULER=false)
//!   pelty-server admin create <email> [--name <display name>]
//!                                        create an admin; password from
//!                                        PELTY_ADMIN_PASSWORD or first line of stdin
//!   pelty-server import whmcs --file whmcs.sql [--cc-hash <hash>] [--dry-run]
//!                                        import a WHMCS SQL export (mysqldump)
//!   pelty-server import whmcs --dsn mysql://user:pass@host/db [--dry-run]
//!                                        import a live WHMCS database (feature `whmcs-mysql`)
//!   pelty-server import blesta|paymenter --file export.json [--dry-run]
//!                                        import a Blesta / Paymenter JSON export
//!
//! On every start the current folder is bootstrapped first: a missing `.env`
//! is written with commented defaults and a new encryption key, missing data
//! folders are created. `.env` values apply unless the environment sets them.
//! `serve` then requires `eula=true` in `eula.txt` (written with `eula=false`
//! on first start) or `PELTY_EULA=true`, and exits before touching the
//! database otherwise. In a terminal, while `.env` has `PELTY_SETUP=pending`
//! (fresh install), `serve` runs the setup wizard first and then starts;
//! `--no-setup` skips it and `--yes` runs it without questions. Without a
//! terminal (Docker, systemd, CI) nothing is asked; in a container a hint
//! points at `docker run -it ... pelty-server setup`.
mod bootstrap;
mod console;
mod embedded;
mod eula;
mod local_pg;
mod logging;
mod setup;
mod update;
mod whmcs_import;

use pelty_api::AppState;
use pelty_core::config::Config;
use pelty_core::i18n::I18n;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;

fn main() -> ExitCode {
    let mut console_flag = None;
    let mut skip_setup = false;
    let mut yes = false;
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    let headless = match setup::Headless::take_flags(&mut raw) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let args: Vec<String> = raw
        .into_iter()
        .filter(|a| match a.as_str() {
            "--console" => {
                console_flag = Some(true);
                false
            }
            "--no-console" => {
                console_flag = Some(false);
                false
            }
            "--no-setup" => {
                skip_setup = true;
                false
            }
            "--yes" | "-y" => {
                yes = true;
                false
            }
            _ => true,
        })
        .collect();
    if matches!(args.as_slice(), [v] if v == "--version" || v == "-V" || v == "version") {
        println!(
            "pelty-server {} ({})",
            env!("CARGO_PKG_VERSION"),
            update::TARGET
        );
        return ExitCode::SUCCESS;
    }
    let root = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("cannot read the current folder: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (report, _) = bootstrap::run(&root, &|k| std::env::var(k).ok());
    report.print();
    embedded::install();
    // Addon zips to seed: a source checkout's `extensions/addons/dist` (or
    // PELTY_ADDONS_DIST) wins; otherwise the ones compiled in with the opt-in
    // `embed-addons` feature. A default/release binary carries none, so a
    // production install starts without addons (upload them in Admin).
    let dist_set = std::env::var("PELTY_ADDONS_DIST").is_ok_and(|v| !v.trim().is_empty())
        || bootstrap::read_env_file(&root)
            .iter()
            .any(|(k, v)| k == "PELTY_ADDONS_DIST" && !v.trim().is_empty());
    if !dist_set && !root.join("extensions/addons/dist").is_dir() {
        match embedded::unpack_addons(&root) {
            // Still single-threaded here.
            Ok(Some(d)) => std::env::set_var("PELTY_ADDONS_DIST", d),
            Ok(None) => {}
            Err(e) => println!("[PeltyCMS] warning: could not unpack the bundled addons: {e}"),
        }
    }
    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    let serves = matches!(argv.as_slice(), [] | ["serve"]);
    let setup_cmd = argv.as_slice() == ["setup"];
    let interactive = {
        use std::io::IsTerminal;
        std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
    };
    let mut plan = None;
    let pending = setup::pending(&root);
    if yes && (setup_cmd || (serves && pending)) {
        match run_headless(&root, &headless) {
            Ok(p) => plan = Some(p),
            Err(code) => return code,
        }
    } else if serves && pending && !skip_setup && in_container() {
        // A container never opens the wizard by itself; it is one command away.
        println!("[PeltyCMS] Not configured yet: run `docker run -it ... pelty-server setup` (or `docker compose run --rm app pelty-server setup`), or set the PELTY_* variables.");
    } else if setup_cmd || (serves && !skip_setup && interactive && pending) {
        if !interactive {
            eprintln!(
                "pelty-server setup needs an interactive terminal (edit .env and eula.txt instead)"
            );
            return ExitCode::FAILURE;
        }
        match run_wizard(&root) {
            Ok(mut p) => {
                p.ask_start = setup_cmd;
                plan = Some(p)
            }
            Err(code) => return code,
        }
    }
    if serves
        && interactive
        && !yes
        && !skip_setup
        && bootstrap::read_env_file(&root)
            .iter()
            .all(|(k, _)| k != setup::MARKER)
        && root.join(bootstrap::ENV_FILE).exists()
    {
        println!("[PeltyCMS] Tip: `pelty-server setup` walks through the database, web, proxy, admin and email settings.");
    }
    // Still single-threaded: export `.env` values the environment does not set
    // (an empty variable, e.g. `FOO=` from docker compose, counts as unset).
    for (k, v) in bootstrap::read_env_file(&root) {
        if std::env::var(&k)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            std::env::set_var(k, v);
        }
    }
    if serves {
        // `PELTY_EULA=true` accepts the EULA without editing eula.txt
        // (containers, CI); `serve` refuses to start until it is accepted.
        if let eula::Gate::Refused { created } =
            // `PELTY_EULA=true` accepts the EULA without editing eula.txt (containers and CI).
            eula::check(&root, std::env::var("PELTY_EULA").ok().as_deref())
        {
            println!("{}", eula::refusal_message(&root));
            if created {
                println!(
                    "First start: also review {} (DATABASE_URL, PELTY_BIND, PELTY_PUBLIC_URL).",
                    root.join(bootstrap::ENV_FILE).display()
                );
            }
            return ExitCode::FAILURE;
        }
    }
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    rt.block_on(run(args, console_flag, plan))
}

/// Run the interactive wizard, save `.env` / `eula.txt`, and return the
/// database work for after the migrations.
fn run_wizard(root: &std::path::Path) -> Result<setup::Plan, ExitCode> {
    let file: std::collections::HashMap<String, String> =
        bootstrap::read_env_file(root).into_iter().collect();
    let cur = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| file.get(k).cloned())
    };
    let eula_ok = eula::env_accepts(std::env::var("PELTY_EULA").ok().as_deref())
        || std::fs::read_to_string(root.join(eula::FILE))
            .map(|t| eula::accepted_in(&t))
            .unwrap_or(false);
    let answers = {
        let mut svc = match setup::term::Live::new() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("cannot start the setup checks: {e}");
                return Err(ExitCode::FAILURE);
            }
        };
        let locales = cur("PELTY_LOCALES_DIR").unwrap_or_else(|| "locales".into());
        let dist =
            root.join(cur("PELTY_ADDONS_DIST").unwrap_or_else(|| "extensions/addons/dist".into()));
        let mut ctx = setup::Ctx {
            cur: &cur,
            eula_accepted: eula_ok,
            languages: setup::languages(&root.join(locales)),
            currencies: vec![],
            captcha: setup::captcha_providers(&dist),
            gateways: setup::gateway_choices(&dist),
        };
        setup::wizard(&mut setup::term::Term::default(), &mut svc, &mut ctx)
        // `svc` (and its runtime threads) ends here, before set_var below.
    };
    let a = match answers {
        Ok(a) => a,
        Err(setup::Error::Declined) => {
            println!("{}", eula::refusal_message(root));
            return Err(ExitCode::FAILURE);
        }
        Err(setup::Error::Cancelled) => {
            println!("Setup cancelled; nothing was saved.");
            return Err(ExitCode::FAILURE);
        }
        Err(setup::Error::Io(e)) => {
            eprintln!("setup aborted: {e}; nothing was saved.");
            return Err(ExitCode::FAILURE);
        }
    };
    save_answers(root, a)
}

/// `--yes`: defaults plus flags / environment, no questions.
fn run_headless(root: &std::path::Path, h: &setup::Headless) -> Result<setup::Plan, ExitCode> {
    let file: std::collections::HashMap<String, String> =
        bootstrap::read_env_file(root).into_iter().collect();
    let cur = |k: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.trim().is_empty())
            .or_else(|| file.get(k).cloned())
    };
    let eula_ok = eula::env_accepts(std::env::var("PELTY_EULA").ok().as_deref())
        || std::fs::read_to_string(root.join(eula::FILE))
            .map(|t| eula::accepted_in(&t))
            .unwrap_or(false);
    let res = match setup::term::Live::new() {
        Ok(mut svc) => setup::headless(&mut svc, &cur, eula_ok, h),
        Err(e) => Err(format!("cannot start the setup checks: {e}")),
    };
    match res {
        Ok(a) => save_answers(root, a),
        Err(e) => {
            eprintln!("setup --yes: {e}");
            Err(ExitCode::FAILURE)
        }
    }
}

/// True inside a container (`/.dockerenv` or `PELTY_DOCKER=1`).
fn in_container() -> bool {
    // `PELTY_DOCKER=1` (or `true`) marks a container when `/.dockerenv` is absent.
    let marked = std::env::var("PELTY_DOCKER").is_ok_and(|v| v == "1" || v == "true");
    std::path::Path::new("/.dockerenv").exists() || marked
}

fn save_answers(root: &std::path::Path, a: setup::Answers) -> Result<setup::Plan, ExitCode> {
    if let Err(e) = setup::write_env(root, &a.env).and_then(|_| setup::accept_eula(root)) {
        eprintln!("cannot save the configuration: {e}");
        return Err(ExitCode::FAILURE);
    }
    println!(
        "[PeltyCMS] saved {} and {}",
        root.join(bootstrap::ENV_FILE).display(),
        root.join(eula::FILE).display()
    );
    // The answers apply to this run even where the environment says otherwise.
    for (k, v) in &a.env {
        if std::env::var(k).is_ok_and(|old| !old.trim().is_empty() && old != *v) {
            println!("[PeltyCMS] note: {k} is also set in the environment, which overrides .env on later starts");
        }
        std::env::set_var(k, v);
    }
    Ok(a.plan)
}

async fn run(args: Vec<String>, console_flag: Option<bool>, plan: Option<setup::Plan>) -> ExitCode {
    let pipe_mode =
        args.first().is_some_and(|a| a == "mail") && args.get(1).is_some_and(|a| a == "pipe");
    let failure = if pipe_mode {
        ExitCode::from(75)
    } else {
        ExitCode::FAILURE
    };
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("configuration error: {e}");
            return failure;
        }
    };
    logging::init(cfg.log_json);
    let res = match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [] | ["serve"] => serve(cfg, console_flag, plan).await,
        ["setup"] => match finish_setup(&cfg, plan).await {
            Ok(true) => serve(cfg, console_flag, None).await,
            other => other.map(|_| ()),
        },
        ["pg", rest @ ..] => {
            let root = std::env::current_dir().unwrap_or_default();
            local_pg::cmd(&root, rest).map_err(|e| e.into())
        }
        ["migrate"] => setup_db(&cfg).await.map(|_| ()),
        ["admin", "create", email, rest @ ..] => match rest {
            [] => admin_create(cfg, email, "Administrator").await,
            ["--name", name @ ..] if !name.is_empty() => admin_create(cfg, email, &name.join(" ")).await,
            _ => Err("usage: pelty-server admin create <email> [--name <display name>]".into()),
        },
        ["cron", "run", rest @ ..] => cron_run(cfg, rest).await,
        ["mail", "pipe", "--department", department] => pipe_email(cfg, department).await,
        ["update", rest @ ..] => update_cmd(&cfg, rest).await,
        ["import", "whmcs", rest @ ..] => import_whmcs(&cfg, rest).await,
        ["import", src @ ("blesta" | "paymenter"), rest @ ..] => import_json(&cfg, src, rest).await,
        _ => Err("usage: pelty-server [--version | serve [--console|--no-console] [--no-setup|--yes] | setup [--yes] [--local-pg] | pg start|stop|status|download | migrate | update [--check] [--version x.y.z] [--yes] [--no-backup] | admin create <email> [--name <name>] | mail pipe --department <uuid> | cron run [<task> | --schedule-only] | import whmcs --file <whmcs.sql> | --dsn <mysql-url> [--cc-hash <hash>] [--dry-run] | import blesta|paymenter --file <export.json> [--dry-run]]".into()),
    };
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "fatal");
            eprintln!("error: {e}");
            failure
        }
    }
}

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

/// `pelty-server update ...`: the database is only needed for the saved feed
/// URL and the pre-update backup, so a check works without one.
async fn update_cmd(cfg: &Config, rest: &[&str]) -> Result<(), BoxErr> {
    let o = update::parse_args(rest)?;
    let pool = pelty_database::connect(&cfg.database_url, 2).await.ok();
    if pool.is_none() && !o.check {
        println!("[update] warning: database not reachable");
    }
    update::run(pool.as_ref(), &cfg.database_url, &o, &mut |m| {
        println!("[update] {m}")
    })
    .await?;
    Ok(())
}

/// Local MTA delivery: one bounded RFC 5322 message on stdin. Nonzero exit
/// means the MTA should defer delivery; committed Message-IDs are idempotent.
async fn pipe_email(cfg: Config, department: &str) -> Result<(), BoxErr> {
    use tokio::io::AsyncReadExt;
    let id = department.parse().map_err(|_| "invalid department UUID")?;
    let pool = pelty_database::connect(&cfg.database_url, 2).await?;
    let d = pelty_database::support_ext::department(&pool, id)
        .await?
        .ok_or("department not found")?;
    if d.email.is_empty()
        || !pelty_database::tickets::departments(&pool, true)
            .await?
            .iter()
            .any(|dep| dep.id == id)
    {
        return Err("department must be active and have an email address".into());
    }
    let mut raw = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::io::stdin()
            .take(pelty_api::mailbox::MAX_MESSAGE as u64 + 1)
            .read_to_end(&mut raw),
    )
    .await??;
    if raw.len() > pelty_api::mailbox::MAX_MESSAGE {
        return Err("message too large".into());
    }
    let text = std::str::from_utf8(&raw)?;
    let i18n = I18n::load(&cfg.locales_dir, &cfg.default_locale)?;
    let state = AppState::new(pool, cfg, i18n);
    let stats = pelty_api::pipe::process_messages_in_department(
        &state,
        vec![pelty_api::mailbox::parse_message(text, &d.email)],
        id,
    )
    .await?;
    eprintln!(
        "opened={} replied={} rejected={} duplicate={} spam={}",
        stats.opened, stats.replied, stats.rejected, stats.duplicate, stats.spam
    );
    Ok(())
}

async fn setup_db(cfg: &Config) -> Result<pelty_database::Pool, BoxErr> {
    if local_pg::enabled() {
        let root = std::env::current_dir().unwrap_or_default();
        local_pg::ensure_for_serve(&root)?;
    }
    let pool = pelty_database::connect(&cfg.database_url, cfg.db_max_connections).await?;
    pelty_database::migrate(&pool).await?;
    tracing::info!("database connected, migrations applied");
    Ok(pool)
}

/// The addon part of the wizard's plan (needs an initialised app state):
/// captcha provider and payment gateways.
async fn apply_addons(state: &AppState, plan: &setup::Plan) {
    if let Some(c) = &plan.captcha {
        match pelty_api::captcha::configure_from_setup(
            state,
            &c.provider,
            &c.site_key,
            &c.secret_key,
        )
        .await
        {
            Ok(()) => println!(
                "[PeltyCMS] setup: captcha enabled ({}) for guests on {}",
                c.label,
                pelty_api::captcha::SETUP_FORMS.join(", ")
            ),
            Err(e) => println!(
                "[PeltyCMS] setup warning: captcha not enabled: {e} (Admin > Security > Captcha)"
            ),
        }
    }
    for g in &plan.gateways {
        let sg = pelty_api::routes::gateways::SetupGateway {
            addon: g.addon.clone(),
            secret_refs: g
                .secret_refs
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
            test_mode: g.test_mode,
        };
        match pelty_api::routes::gateways::configure_from_setup(state, &sg).await {
            Ok(m) => println!("[PeltyCMS] setup: gateway {m}"),
            Err(e) => println!(
                "[PeltyCMS] setup warning: gateway {} not enabled: {e} (Admin > Gateways)",
                g.label
            ),
        }
    }
}

/// Returns true when the user wants the server started right away.
async fn finish_setup(cfg: &Config, plan: Option<setup::Plan>) -> Result<bool, BoxErr> {
    let pool = setup_db(cfg).await?;
    let ask = plan.as_ref().is_some_and(|p| p.ask_start);
    let mut login_token = None;
    if let Some(p) = &plan {
        login_token = setup::apply(&pool, p).await;
        if p.captcha.is_some() || !p.gateways.is_empty() {
            // Addons need the app state (addon catalog, gateway registry); build one briefly.
            let i18n = I18n::load(&cfg.locales_dir, &cfg.default_locale)?;
            let state = AppState::new(pool.clone(), cfg.clone(), i18n);
            state.init().await.map_err(|e| e.to_string())?;
            apply_addons(&state, p).await;
        }
    }
    pool.close().await;
    let root = std::env::current_dir().unwrap_or_default();
    println!(
        "{}",
        setup::next_steps(
            &cfg.public_url,
            plan.as_ref()
                .and_then(|p| p.admin.as_ref())
                .map(|a| a.email.as_str()),
            login_token.as_deref(),
            &root
        )
    );
    if !ask {
        return Ok(false);
    }
    // Only offer to start when nothing else listens on the port.
    if std::net::TcpListener::bind(cfg.bind).is_err() {
        println!(
            "{} is in use (a server already running?); start PeltyCMS later with `pelty-server`.",
            cfg.bind
        );
        return Ok(false);
    }
    let start = tokio::task::block_in_place(|| setup::ask_start(&mut setup::term::Term::default()))
        .unwrap_or(false);
    Ok(start)
}

async fn serve(
    cfg: Config,
    console_flag: Option<bool>,
    plan: Option<setup::Plan>,
) -> Result<(), BoxErr> {
    use std::io::IsTerminal;
    let started = std::time::Instant::now();
    let stdin_tty = std::io::stdin().is_terminal();
    let console_on = console_flag.unwrap_or(stdin_tty);
    let pool = setup_db(&cfg).await?;
    if let Some(p) = &plan {
        let login_token = setup::apply(&pool, p).await;
        let root = std::env::current_dir().unwrap_or_default();
        println!(
            "{}",
            setup::next_steps(
                &cfg.public_url,
                p.admin.as_ref().map(|a| a.email.as_str()),
                login_token.as_deref(),
                &root
            )
        );
    }
    let i18n = I18n::load(&cfg.locales_dir, &cfg.default_locale)?;
    tracing::info!(languages = ?i18n.languages(), "locales loaded");
    let bind = cfg.bind;
    let state = AppState::new(pool.clone(), cfg, i18n);
    if let Err(e) = pelty_api::translations::reload(&state).await {
        tracing::warn!(error = %e, "translation overrides not loaded");
    }
    state.init().await.map_err(|e| e.to_string())?;
    if let Some(p) = &plan {
        apply_addons(&state, p).await;
    }
    let worker = Arc::new(pelty_api::worker::spawn(state.clone()));
    pelty_api::cluster::spawn(state.clone());
    let app = pelty_api::router(state.clone());
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(%bind, "PeltyCMS listening");
    let stop = Arc::new(tokio::sync::Notify::new());
    #[cfg(unix)]
    let saved_term = stdin_tty
        .then(|| nix::sys::termios::tcgetattr(std::io::stdin()).ok())
        .flatten();
    if console_on {
        let ctx = console::Ctx {
            state,
            started,
            worker: worker.clone(),
            stop: stop.clone(),
        };
        console::start(Arc::new(ctx), stdin_tty);
    }
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown(stop))
    .await?;
    logging::set_printer(None);
    // Stopped by a signal while the console prompt had the terminal in raw mode.
    #[cfg(unix)]
    if let Some(t) = saved_term {
        let _ =
            nix::sys::termios::tcsetattr(std::io::stdin(), nix::sys::termios::SetArg::TCSANOW, &t);
    }
    worker.abort();
    pool.close().await;
    tracing::info!("shut down cleanly");
    Ok(())
}

async fn shutdown(stop: Arc<tokio::sync::Notify>) {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {}, _ = stop.notified() => {} }
}

/// `pelty-server cron run [<task> | --schedule-only]`: one scheduler pass (due billing,
/// overdue, reminders, renewals, escalations, ...; deduplicated, so overlapping
/// with a running server is harmless), then run queued jobs until none are left
/// or 10 minutes passed. For system cron when `PELTY_SCHEDULER=false`.
async fn cron_run(cfg: Config, rest: &[&str]) -> Result<(), BoxErr> {
    let (only, task) = match rest {
        [] => (false, None),
        ["--schedule-only"] => (true, None),
        [t] if !t.starts_with('-') => (false, Some(t.to_string())),
        _ => return Err("usage: pelty-server cron run [<task> | --schedule-only]".into()),
    };
    let pool = setup_db(&cfg).await?;
    let i18n = I18n::load(&cfg.locales_dir, &cfg.default_locale)?;
    let n = cfg.worker_concurrency;
    let state = AppState::new(pool, cfg, i18n);
    state.init().await.map_err(|e| e.to_string())?;
    match &task {
        // One task now (`cron list` in the console shows the names); other
        // queued jobs run too, so its follow-up work is not left waiting.
        Some(t) => match pelty_api::routes::automation::run_task(&state, t).await {
            Ok(id) => println!(
                "[PeltyCMS] cron: queued {t}{}",
                id.map(|i| format!(" (job {i})"))
                    .unwrap_or_else(|| " (already queued this minute)".into())
            ),
            Err(pelty_core::AppError::NotFound) => {
                return Err(format!(
                    "no task {t:?}; built-in: {}",
                    pelty_api::routes::automation::TASKS
                        .iter()
                        .map(|t| t.0)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
                .into())
            }
            Err(e) => return Err(e.to_string().into()),
        },
        None => {
            pelty_api::worker::schedule_tick(&state).await;
            println!("[PeltyCMS] cron: scheduled due tasks");
        }
    }
    if only {
        return Ok(());
    }
    let worker = format!("cron-{}", std::process::id());
    let started = std::time::Instant::now();
    let mut total = 0;
    while started.elapsed() < std::time::Duration::from_secs(600) {
        let ran = pelty_api::worker::run_once(&state, &worker, n)
            .await
            .map_err(|e| e.to_string())?;
        if ran == 0 {
            break;
        }
        total += ran;
    }
    println!("[PeltyCMS] cron: ran {total} job(s)");
    Ok(())
}

async fn admin_create(cfg: Config, email: &str, name: &str) -> Result<(), BoxErr> {
    // Password for `admin create` (otherwise the first line of stdin).
    let password = match std::env::var("PELTY_ADMIN_PASSWORD") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            eprintln!("Enter admin password (min 12 chars), then press Enter:");
            let mut line = String::new();
            tokio::io::BufReader::new(tokio::io::stdin())
                .read_line(&mut line)
                .await?;
            line.trim_end_matches(['\r', '\n']).to_string()
        }
    };
    let pool = setup_db(&cfg).await?;
    let auth = pelty_auth::AuthService {
        pool,
        idle: time_minutes(1),
        absolute: time_minutes(1),
    };
    let user = create_admin(&auth, email, &password, name).await?;
    println!("admin created: {} ({})", user.email, user.id);
    Ok(())
}

/// Register a user with the `admin` role (CLI `admin create` and the console).
pub(crate) async fn create_admin(
    auth: &pelty_auth::AuthService,
    email: &str,
    password: &str,
    name: &str,
) -> Result<pelty_database::users::User, String> {
    auth.register(
        pelty_auth::RegisterInput {
            email,
            password,
            display_name: name,
        },
        "admin",
        None,
    )
    .await
    .map_err(|e| format!("could not create admin: {} ({})", e, e.code()))
}

fn time_minutes(m: i64) -> time::Duration {
    time::Duration::minutes(m)
}

async fn import_whmcs(cfg: &Config, args: &[&str]) -> Result<(), BoxErr> {
    let o = whmcs_import::parse_args(args)?;
    let pool = setup_db(cfg).await?;
    print!("{}", whmcs_import::run(&pool, cfg, &o).await?);
    Ok(())
}

/// Blesta / Paymenter: JSON export (table name -> rows), see `make docs`.
async fn import_json(cfg: &Config, source: &str, args: &[&str]) -> Result<(), BoxErr> {
    let mut file = None;
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--dry-run" => dry_run = true,
            "--file" => file = it.next().map(|s| s.to_string()),
            other => return Err(format!("unknown import option {other}").into()),
        }
    }
    let file = file.ok_or("missing --file export.json")?;
    let json = std::fs::read_to_string(&file)?;
    let (data, warnings) = match source {
        "blesta" => pelty_import::blesta::convert(&pelty_import::blesta::parse(&json)?),
        _ => pelty_import::paymenter::convert(&pelty_import::paymenter::parse(&json)?),
    };
    let pool = setup_db(cfg).await?;
    let mut report = pelty_import::run_source(&pool, &data, dry_run, source).await?;
    let mut all = warnings;
    all.append(&mut report.warnings);
    report.warnings = all;
    print!("{}", report.render());
    Ok(())
}
