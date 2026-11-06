//! Optional local PostgreSQL sidecar (`PELTY_LOCAL_PG=true`).
//! Downloads once into `PELTY_PG_HOME` (default `data/pg`) and starts it on
//! serve when DATABASE_URL points at this machine and nothing listens.
use pelty_pg::{Auth, Config, DbUrl};
use std::path::Path;

/// Env / `.env` says local Postgres should be managed.
pub fn enabled() -> bool {
    pelty_pg::local_enabled()
}

fn config(root: &Path, auth: Auth) -> Config {
    Config {
        root: root.to_path_buf(),
        home: std::env::var("PELTY_PG_HOME")
            .ok()
            .or_else(|| std::env::var("PG_HOME").ok())
            .filter(|v| !v.trim().is_empty())
            .map(std::path::PathBuf::from),
        run_as: std::env::var("PG_USER")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        pg_ver: std::env::var("PG_VER")
            .ok()
            .filter(|v| !v.trim().is_empty()),
        auth,
    }
}

fn auth_from_url(db: &DbUrl) -> Auth {
    if db.password.is_empty() {
        Auth::Trust
    } else {
        Auth::Password {
            user: db.user.clone(),
            password: db.password.clone(),
        }
    }
}

/// First-run provision for the setup wizard: generate a password, download/start,
/// return the `DATABASE_URL` (database name `peltycms`).
pub fn provision(port: u16) -> Result<String, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let password = pelty_pg::generate_password();
    let auth = Auth::Password {
        user: "pelty".into(),
        password: password.clone(),
    };
    let cfg = config(&root, auth.clone());
    let db = DbUrl {
        user: "pelty".into(),
        password,
        host: "127.0.0.1".into(),
        port,
        name: "peltycms".into(),
    };
    if !db.reachable() {
        pelty_pg::start(&cfg, &db)?;
        pelty_pg::wait_reachable(&db, std::time::Duration::from_secs(45))?;
    }
    Ok(db.to_url())
}

/// On `serve` / `migrate`: if `PELTY_LOCAL_PG` is set, ensure the sidecar is up.
pub fn ensure_for_serve(root: &Path) -> Result<(), String> {
    if !enabled() {
        return Ok(());
    }
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "PELTY_LOCAL_PG=true needs DATABASE_URL".to_string())?;
    let db = DbUrl::parse(&url)?;
    if !db.is_local() {
        return Err(format!(
            "PELTY_LOCAL_PG=true but DATABASE_URL host is {} (expected localhost)",
            db.host
        ));
    }
    let cfg = config(root, auth_from_url(&db));
    println!("[PeltyCMS] local PostgreSQL ({})", cfg.home_dir().display());
    pelty_pg::ensure(&cfg, &db)
}

/// `pelty-server pg start|stop|status|download`
pub fn cmd(root: &Path, args: &[&str]) -> Result<(), String> {
    let sub = args.first().copied().unwrap_or("");
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://pelty@127.0.0.1:5432/peltycms".into());
    let db = DbUrl::parse(&url).unwrap_or(DbUrl {
        user: "pelty".into(),
        password: String::new(),
        host: "127.0.0.1".into(),
        port: 5432,
        name: "peltycms".into(),
    });
    let cfg = config(root, auth_from_url(&db));
    match sub {
        "start" => {
            if db.reachable() {
                println!("PostgreSQL already reachable at {}:{}", db.host, db.port);
                return Ok(());
            }
            pelty_pg::ensure(&cfg, &db)?;
            println!("started ({})", cfg.data_dir().display());
            Ok(())
        }
        "stop" => {
            if pelty_pg::stop(&cfg)? {
                Ok(())
            } else {
                Err("no managed local PostgreSQL was running".into())
            }
        }
        "status" => {
            if let Err(e) = pelty_pg::status(&cfg) {
                println!(
                    "{}:{} {} ({e})",
                    db.host,
                    db.port,
                    if db.reachable() {
                        "reachable"
                    } else {
                        "not reachable"
                    }
                );
            }
            Ok(())
        }
        "download" => {
            let bin = pelty_pg::download(&cfg)?;
            println!("PostgreSQL binaries in {}", bin.display());
            Ok(())
        }
        _ => Err(
            "usage: pelty-server pg start|stop|status|download\n  \
             Requires PELTY_LOCAL_PG=true (or run start/download once) and a localhost DATABASE_URL."
                .into(),
        ),
    }
}
