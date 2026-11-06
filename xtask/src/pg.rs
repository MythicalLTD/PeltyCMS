//! Development PostgreSQL. A reachable DATABASE_URL is used as is (any local
//! install, Docker container or remote server). When it points at this
//! machine and nothing listens, a dev server is started on its port, trying in order:
//! 1. Native / downloaded binaries via `pelty-pg` (`PG_HOME`, default `data/pg`);
//! 2. Docker (`postgres:16`, container `peltycms-dev-postgres`, trust auth);
//! 3. Linux as root with apt: `apt-get install postgresql`;
//! 4. Zonky download (again via `pelty-pg`).
//!
//! `PELTY_DEV_PG=native|docker` forces one of the first two.
use crate::util::{self, which, Ctx, R};
use pelty_pg::{Auth, Config as PgConfig, DbUrl};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub const CONTAINER: &str = "peltycms-dev-postgres";
const VOLUME: &str = "peltycms-dev-pgdata";

fn pg_cfg(ctx: &Ctx) -> PgConfig {
    PgConfig {
        root: ctx.root.clone(),
        home: ctx
            .get("PG_HOME")
            .or_else(|| ctx.get("PELTY_PG_HOME"))
            .map(PathBuf::from),
        run_as: ctx.get("PG_USER"),
        pg_ver: ctx.get("PG_VER"),
        auth: Auth::Trust,
    }
}

pub fn db_url(ctx: &Ctx) -> R<DbUrl> {
    DbUrl::parse(
        &ctx.get("DATABASE_URL")
            .ok_or("DATABASE_URL is not set (add it to .env)")?,
    )
}

/// Makes sure the DATABASE_URL server is up (the database itself is created
/// by `pelty-server migrate` / `serve`).
pub fn ensure_server(ctx: &Ctx) -> R<DbUrl> {
    let db = db_url(ctx)?;
    if db.reachable() {
        return Ok(db);
    }
    if !db.is_local() {
        return Err(format!(
            "cannot reach PostgreSQL at {}:{} (DATABASE_URL)",
            db.host, db.port
        ));
    }
    start(ctx, &db)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !db.reachable() {
        if Instant::now() > deadline {
            return Err(format!("PostgreSQL did not come up on port {}", db.port));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(db)
}

pub fn docker_ok() -> bool {
    which("docker").is_some() && util::succeeds(Command::new("docker").arg("info"))
}

/// Starts a dev server for DATABASE_URL's port.
pub fn start(ctx: &Ctx, db: &DbUrl) -> R {
    let mode = ctx.get("PELTY_DEV_PG").unwrap_or_default();
    let cfg = pg_cfg(ctx);
    if mode != "docker" {
        if pelty_pg::find_bin(&cfg).is_some() {
            return pelty_pg::start(&cfg, db);
        }
    }
    if mode != "native" && docker_ok() {
        return start_docker(db);
    }
    if mode == "docker" {
        return Err("PELTY_DEV_PG=docker but Docker is not running".into());
    }
    if cfg!(target_os = "linux") && is_root() && which("apt-get").is_some() {
        eprintln!("PostgreSQL not found; installing: apt-get install -y postgresql");
        let install = || {
            Command::new("apt-get")
                .args(["install", "-y", "postgresql"])
                .env("DEBIAN_FRONTEND", "noninteractive")
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        };
        if !install() {
            let _ = Command::new("apt-get").arg("update").status();
            install();
        }
        if pelty_pg::find_bin(&cfg).is_some() {
            return pelty_pg::start(&cfg, db);
        }
    }
    pelty_pg::start(&cfg, db)
        .map_err(|e| format!("{e}\n  Or start Docker Desktop, or set PELTY_DEV_PG=docker"))
}

fn is_root() -> bool {
    cfg!(unix)
        && Command::new("id")
            .arg("-u")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
            .unwrap_or(false)
}

fn docker_exists() -> bool {
    Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name=^/{CONTAINER}$"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == CONTAINER)
        .unwrap_or(false)
}

fn start_docker(db: &DbUrl) -> R {
    if docker_exists() {
        eprintln!("starting Docker container {CONTAINER}");
        util::run(
            Command::new("docker")
                .args(["start", CONTAINER])
                .stdout(std::process::Stdio::null()),
        )?;
    } else {
        eprintln!(
            "starting PostgreSQL 16 in Docker ({CONTAINER}, volume {VOLUME}) on 127.0.0.1:{}",
            db.port
        );
        let mut c = Command::new("docker");
        c.args([
            "run",
            "-d",
            "--name",
            CONTAINER,
            "-p",
            &format!("127.0.0.1:{}:5432", db.port),
        ])
        .args([
            "-e",
            &format!("POSTGRES_USER={}", db.user),
            "-e",
            &format!("POSTGRES_DB={}", db.name),
        ])
        .args(["-v", &format!("{VOLUME}:/var/lib/postgresql/data")]);
        if db.password.is_empty() {
            c.args(["-e", "POSTGRES_HOST_AUTH_METHOD=trust"]);
        } else {
            c.args(["-e", &format!("POSTGRES_PASSWORD={}", db.password)]);
        }
        c.arg("postgres:16");
        util::run(c.stdout(std::process::Stdio::null()))?;
    }
    let deadline = Instant::now() + Duration::from_secs(90);
    while !util::succeeds(Command::new("docker").args([
        "exec",
        CONTAINER,
        "pg_isready",
        "-h",
        "127.0.0.1",
        "-U",
        &db.user,
    ])) {
        if Instant::now() > deadline {
            return Err(format!(
                "PostgreSQL in {CONTAINER} did not become ready (docker logs {CONTAINER})"
            ));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// Stops the native managed server in `PG_HOME` if it is running.
pub fn stop_managed(ctx: &Ctx) -> R<bool> {
    pelty_pg::stop(&pg_cfg(ctx))
}

/// `cargo xtask pg start|stop|status`.
pub fn cmd(ctx: &Ctx, sub: &str) -> R {
    let db = db_url(ctx)?;
    match sub {
        "start" => {
            if db.reachable() {
                println!("PostgreSQL already reachable at {}:{}", db.host, db.port);
                return Ok(());
            }
            ensure_server(ctx).map(|_| ())
        }
        "stop" if stop_managed(ctx)? => Ok(()),
        "stop" | "status" => {
            let cfg = pg_cfg(ctx);
            if pelty_pg::find_bin(&cfg).is_some() && cfg.data_dir().join("PG_VERSION").exists() {
                return if sub == "status" {
                    pelty_pg::status(&cfg)
                } else {
                    pelty_pg::stop(&cfg).map(|_| ())
                };
            }
            if docker_ok() && docker_exists() {
                return if sub == "stop" {
                    util::run(Command::new("docker").args(["stop", CONTAINER]))
                } else {
                    util::run(Command::new("docker").args([
                        "ps",
                        "-a",
                        "--filter",
                        &format!("name=^/{CONTAINER}$"),
                    ]))
                };
            }
            if sub == "status" {
                println!(
                    "{}:{} {}",
                    db.host,
                    db.port,
                    if db.reachable() {
                        "reachable"
                    } else {
                        "not reachable"
                    }
                );
                return Ok(());
            }
            Err(
                "no dev PostgreSQL started by xtask was found (stop your own server yourself)"
                    .into(),
            )
        }
        _ => Err("usage: cargo xtask pg start|stop|status".into()),
    }
}

/// Drops and recreates DATABASE_URL's database (psql, or psql inside the Docker container).
pub fn reset(ctx: &Ctx, db: &DbUrl) -> R {
    let drop = format!("drop database if exists \"{}\" with (force)", db.name);
    let create = format!("create database \"{}\"", db.name);
    if let Some(psql) = pelty_pg::psql(&pg_cfg(ctx)) {
        return util::run(
            Command::new(psql)
                .arg(db.admin_url())
                .args(["-qc", &drop, "-c", &create]),
        );
    }
    if docker_ok() && docker_exists() {
        return util::run(Command::new("docker").args([
            "exec", CONTAINER, "psql", "-U", &db.user, "-d", "postgres", "-qc", &drop, "-c",
            &create,
        ]));
    }
    Err(
        "db-reset needs psql (PostgreSQL client tools on PATH) or the xtask Docker container"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::DbUrl;

    #[test]
    fn parses_database_urls() {
        let d = DbUrl::parse("postgres://postgres@127.0.0.1:55432/peltycms").unwrap();
        assert_eq!(
            (d.user.as_str(), d.host.as_str(), d.port, d.name.as_str()),
            ("postgres", "127.0.0.1", 55432, "peltycms")
        );
    }
}
