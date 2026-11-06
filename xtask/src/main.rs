//! PeltyCMS developer commands, in Rust so they work the same on Windows,
//! macOS and Linux (no make, no bash). Run `cargo xtask help`.
//!
//! `.cargo/config.toml` defines the `cargo xtask` alias (and shortcuts such as
//! `cargo dev`). The Makefile targets are thin wrappers around these commands.
#[cfg(feature = "addon-test")]
mod addon_test;
mod dev;
mod pg;
mod procs;
mod release;
mod seed;
mod tasks;
mod util;

use std::process::ExitCode;
use util::{Ctx, R};

/// Every command with its one-line help (`cargo xtask help`). pelty-docs
/// reads this table for the developer CLI page, so keep one entry per line.
const COMMANDS: &[(&str, &str)] = &[
    ("help", "Show this help"),
    ("dev", "Postgres + backend (cargo-watch if installed) + Vite on :5173; stops a prior dev run first; Ctrl+C stops all"),
    ("stop", "Stop a leftover backend/Vite from a previous cargo xtask dev (pid file in target/); also the dev PostgreSQL on Windows or with --pg"),
    ("backend", "Backend only (cargo run, or cargo watch if installed)"),
    ("frontend", "Frontend only (Vite dev server, proxies /api to the backend)"),
    ("db", "Start the dev Postgres if needed, create the DATABASE_URL database and migrate"),
    ("db-reset", "Drop and recreate the DATABASE_URL database, then migrate"),
    ("pg", "Dev PostgreSQL: pg start, pg stop or pg status (installed binaries, Docker, or a download)"),
    ("seed", "Load demo data into a running server (PELTY_URL, ADMIN_EMAIL, ADMIN_PASSWORD, CLIENT_PASSWORD)"),
    ("seed-extra", "More demo data for newer features (run after seed; same env)"),
    ("build", "Release build: addon zips, docs, frontend, server binary"),
    ("release", "release <version> [--local | --targets a,b] [--publish] [--assemble]: dry run builds + packages dist/<version>/ (archives, SHA256SUMS + signature, manifest.json, changelog); --assemble only re-sums existing archives (CI); --publish tags and creates the GitHub Release"),
    ("release-keygen", "release-keygen [path]: new ed25519 release signing key (default ~/.config/peltycms/release-signing.key); prints the public key for crates/pelty-server/release-key.pub"),
    ("docs", "Generate the user docs (target/docs) and developer docs (target/docs/developers), with coverage report"),
    ("addons", "Build every WASM addon into extensions/addons/dist/*.zip (or only: addons <dir>...)"),
    ("addon", "Scaffold a new WASM addon: addon <id> --kind <kind> (gateway, provisioning, registrar, captcha, sso, integration)"),
    ("addon-test", "Offline WASM conformance: cargo run -p xtask --features addon-test -- addon-test <package.zip> <fixture.json> [--report report.json]"),
    ("test", "Rust workspace tests (needs TEST_DATABASE_URL; extra args go to cargo test) + frontend tests"),
    ("lint", "clippy + TypeScript check"),
    ("docker", "Build the Docker image (peltycms:latest)"),
    ("clean", "Remove build outputs (target/, frontend/dist, addon zips)"),
    ("env", "Print DATABASE_URL and PELTY_* as the dev commands see them (.env + environment)"),
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, rest) = match args.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => ("help", &[][..]),
    };
    match dispatch(cmd, rest) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cmd: &str, args: &[String]) -> R {
    match cmd {
        "help" | "-h" | "--help" => {
            help();
            Ok(())
        }
        "dev" => dev::dev(&Ctx::with_dotenv()?),
        "stop" => dev::stop(&Ctx::with_dotenv()?, args),
        "backend" => dev::backend(&Ctx::with_dotenv()?),
        "frontend" => dev::frontend(&Ctx::with_dotenv()?),
        "db" => dev::db(&Ctx::with_dotenv()?),
        "db-reset" => dev::db_reset(&Ctx::with_dotenv()?),
        "pg" => pg::cmd(
            &Ctx::with_dotenv()?,
            args.first().map(String::as_str).unwrap_or("start"),
        ),
        "seed" => seed::seed(&Ctx::with_dotenv()?),
        "seed-extra" => seed::seed_extra(&Ctx::with_dotenv()?),
        "docs" => docs(&Ctx::plain()),
        "build" => tasks::build(&Ctx::plain()),
        "release" => release::release(&Ctx::plain(), args),
        "release-keygen" => release::keygen(&Ctx::plain(), args),
        "addons" => tasks::addons(&Ctx::plain(), args),
        "addon" => tasks::addon(&Ctx::plain(), args),
        #[cfg(feature = "addon-test")]
        "addon-test" => addon_test::run(args),
        #[cfg(not(feature = "addon-test"))]
        "addon-test" => Err("enable the harness: cargo run -p xtask --features addon-test -- addon-test <package.zip> <fixture.json>".into()),
        "test" => tasks::test(&Ctx::plain(), args),
        "lint" => tasks::lint(&Ctx::plain()),
        "docker" => tasks::docker(&Ctx::plain()),
        "clean" => tasks::clean(&Ctx::plain()),
        "env" => {
            let ctx = Ctx::with_dotenv()?;
            let mut keys: Vec<String> = std::env::vars()
                .map(|(k, _)| k)
                .chain(ctx.extra.keys().cloned())
                .collect();
            keys.sort();
            keys.dedup();
            for k in keys
                .iter()
                .filter(|k| *k == "DATABASE_URL" || k.starts_with("PELTY_"))
            {
                println!("{k}={}", ctx.get(k).unwrap_or_default());
            }
            Ok(())
        }
        other => {
            help();
            Err(format!("unknown command `{other}` {args:?}"))
        }
    }
}

fn help() {
    println!("PeltyCMS developer commands: cargo xtask <command> [args]\n");
    for (name, about) in COMMANDS {
        println!("  cargo xtask {name:<10} {about}");
    }
    println!("\nShortcuts (.cargo/config.toml): cargo dev, cargo stop, cargo addons, cargo seed");
}

/// Docs share the same profile as `xtask build` (`release-fast`) so pelty-api
/// is not compiled twice under different LTO settings.
fn docs(ctx: &Ctx) -> R {
    util::run(
        ctx.cargo().args([
            "run",
            "--profile",
            "release-fast",
            "-p",
            "pelty-docs",
            "--",
            "target/docs",
        ]),
    )
}
