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
/// return the `DATABASE_URL` (database name `peltycms`). The port moves when
/// something else already listens there, and a cluster left by an earlier
/// attempt gets the new password (see [`pelty_pg::provision`]).
pub fn provision(port: u16) -> Result<String, String> {
    let root = std::env::current_dir().map_err(|e| e.to_string())?;
    let auth = Auth::Password {
        user: "pelty".into(),
        password: pelty_pg::generate_password(),
    };
    let cfg = config(&root, auth);
    pelty_pg::provision(&cfg, port).map(|db| db.to_url())
}

/// On `serve` / `migrate`: if `PELTY_LOCAL_PG` is set, ensure the sidecar is up.
/// Returns the new `DATABASE_URL` when the managed cluster had to move to a
/// free port (another program holds the configured one); it is saved to `.env`.
pub fn ensure_for_serve(root: &Path) -> Result<Option<String>, String> {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !enabled() || DONE.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(None);
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
    crate::logging::say(&format!(
        "[PeltyCMS] local PostgreSQL ({})",
        cfg.home_dir().display()
    ));
    let moved = pelty_pg::ensure_managed(&cfg, &db)?;
    DONE.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(new) = &moved {
        let mut up = std::collections::BTreeMap::new();
        up.insert("DATABASE_URL".to_string(), new.clone());
        match crate::setup::write_env(root, &up) {
            Ok(()) => crate::logging::say(&format!(
                "[PeltyCMS] saved the new local PostgreSQL port in {}",
                root.join(crate::bootstrap::ENV_FILE).display()
            )),
            Err(e) => crate::logging::say(&format!(
                "[PeltyCMS] warning: could not save the new DATABASE_URL to .env: {e}"
            )),
        }
    }
    Ok(moved)
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
            if !db.is_local() {
                return Err(format!("DATABASE_URL host is {} (expected localhost)", db.host));
            }
            if let Some(url) = pelty_pg::ensure_managed(&cfg, &db)? {
                let port = DbUrl::parse(&url).map(|d| d.port).unwrap_or(db.port);
                println!(
                    "port {} is used by another program; started on port {port} (update DATABASE_URL)",
                    db.port
                );
            }
            println!("running ({})", cfg.data_dir().display());
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

#[cfg(test)]
mod tests {
    use pelty_pg::{Auth, Config, DbUrl};
    use sqlx::Connection;

    async fn login(db: &DbUrl, name: &str) -> Result<sqlx::PgConnection, sqlx::Error> {
        sqlx::PgConnection::connect(&db.with_db(name)).await
    }

    /// The Windows fresh-install failure, on real PostgreSQL binaries:
    /// `PELTY_PG_TEST_DIST=/path/to/dist` (with `bin/initdb`, `pg_ctl`, `postgres`).
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn managed_pg_survives_busy_port_and_stale_cluster() {
        let Ok(dist) = std::env::var("PELTY_PG_TEST_DIST") else {
            eprintln!("skipped: set PELTY_PG_TEST_DIST");
            return;
        };
        let home =
            std::env::temp_dir().join(format!("pelty-pg-it-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(&dist, home.join("dist")).unwrap();
        let cfg = |pw: &str| Config {
            root: home.clone(),
            home: Some(home.clone()),
            run_as: None,
            pg_ver: None,
            auth: Auth::Password {
                user: "pelty".into(),
                password: pw.into(),
            },
        };
        // Another program owns the wizard's port (on the user's PC: an editor
        // port forward on 5432). The old code handed out a fresh password for it.
        let squatter = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy = squatter.local_addr().unwrap().port();
        let db1 =
            tokio::task::block_in_place(|| pelty_pg::provision(&cfg("first-pass"), busy)).unwrap();
        assert_ne!(
            db1.port, busy,
            "must not reuse a port owned by someone else"
        );
        let mut c = login(&db1, "postgres").await.expect("first password works");
        sqlx::query("CREATE DATABASE peltycms")
            .execute(&mut c)
            .await
            .unwrap();
        drop(c);

        // Setup run again: the cluster from the first attempt (now with a
        // database) keeps its data and gets the new password.
        let db2 =
            tokio::task::block_in_place(|| pelty_pg::provision(&cfg("second-pass"), busy)).unwrap();
        assert!(
            login(&db2, "peltycms").await.is_ok(),
            "new password + data kept"
        );
        let mut old = db2.clone();
        old.password = "first-pass".into();
        assert!(login(&old, "postgres").await.is_err());

        // Serve with a stale .env password: re-applied when the sidecar starts.
        tokio::task::block_in_place(|| pelty_pg::stop(&cfg("x"))).unwrap();
        let mut db3 = db2.clone();
        db3.password = "third-pass".into();
        let moved =
            tokio::task::block_in_place(|| pelty_pg::ensure_managed(&cfg("third-pass"), &db3))
                .unwrap();
        assert_eq!(moved, None);
        assert!(login(&db3, "peltycms").await.is_ok());
        // Already running: a no-op.
        assert_eq!(
            tokio::task::block_in_place(|| pelty_pg::ensure_managed(&cfg("third-pass"), &db3))
                .unwrap(),
            None
        );

        // Serve while something else took the saved port: move, report the URL.
        tokio::task::block_in_place(|| pelty_pg::stop(&cfg("x"))).unwrap();
        let squatter2 = std::net::TcpListener::bind(("127.0.0.1", db3.port)).unwrap();
        let moved =
            tokio::task::block_in_place(|| pelty_pg::ensure_managed(&cfg("third-pass"), &db3))
                .unwrap()
                .expect("port moved");
        let db4 = DbUrl::parse(&moved).unwrap();
        assert_ne!(db4.port, db3.port);
        assert!(login(&db4, "peltycms").await.is_ok());

        tokio::task::block_in_place(|| pelty_pg::stop(&cfg("x"))).unwrap();
        drop((squatter, squatter2));
        let _ = std::fs::remove_dir_all(&home);
    }

    /// A first attempt that died right after `initdb` (no databases yet)
    /// whose password cannot be reset is re-created instead of failing.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn empty_stale_cluster_is_recreated_when_reset_fails() {
        let Ok(dist) = std::env::var("PELTY_PG_TEST_DIST") else {
            return;
        };
        let home =
            std::env::temp_dir().join(format!("pelty-pg-it-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&home).unwrap();
        std::os::unix::fs::symlink(&dist, home.join("dist")).unwrap();
        let cfg = |pw: &str| Config {
            root: home.clone(),
            home: Some(home.clone()),
            run_as: None,
            pg_ver: None,
            auth: Auth::Password {
                user: "pelty".into(),
                password: pw.into(),
            },
        };
        let port = pelty_pg::free_port(41000).unwrap();
        tokio::task::block_in_place(|| pelty_pg::provision(&cfg("one"), port)).unwrap();
        tokio::task::block_in_place(|| pelty_pg::stop(&cfg("x"))).unwrap();
        // Another live process (not an ancestor) "holds" the data dir, so
        // single-user mode refuses and the password cannot be reset.
        let mut holder = std::process::Command::new("sleep")
            .arg("60")
            .spawn()
            .unwrap();
        let pid = home.join("data").join("postmaster.pid");
        std::fs::write(&pid, format!("{}\n", holder.id())).unwrap();
        let reset =
            pelty_pg::set_password(&cfg("two"), &home.join("dist").join("bin"), "pelty", "two");
        assert!(reset.is_err(), "the lock must block single-user mode");
        let db = tokio::task::block_in_place(|| pelty_pg::provision(&cfg("two"), port)).unwrap();
        let _ = holder.kill();
        let _ = holder.wait();
        assert!(
            login(&db, "postgres").await.is_ok(),
            "re-created with the new password"
        );
        tokio::task::block_in_place(|| pelty_pg::stop(&cfg("x"))).unwrap();
        let _ = std::fs::remove_dir_all(&home);
    }
}
