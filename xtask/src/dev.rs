//! `cargo xtask dev|stop|backend|frontend|db|db-reset`: the old scripts/dev.sh.
use crate::util::{self, Ctx, R};
use crate::{pg, procs};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
use std::process::Command;
use std::time::{Duration, Instant};

const VITE_PORT: u16 = 5173;

fn bind(ctx: &Ctx) -> String {
    ctx.get("PELTY_BIND")
        .unwrap_or_else(|| "127.0.0.1:8080".into())
}

fn bind_port(ctx: &Ctx) -> u16 {
    bind(ctx)
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .unwrap_or(8080)
}

/// Vite listens on localhost only when the API is loopback; else on all interfaces.
fn vite_host(ctx: &Ctx) -> String {
    if let Some(h) = ctx.get("PELTY_VITE_HOST") {
        return h;
    }
    match bind(ctx)
        .rsplit_once(':')
        .map(|(h, _)| h.to_string())
        .unwrap_or_default()
        .as_str()
    {
        "127.0.0.1" | "localhost" => "127.0.0.1".into(),
        _ => "0.0.0.0".into(),
    }
}

/// True when something accepts connections on the port, or it cannot be bound.
fn port_busy(host: &str, port: u16) -> bool {
    let local = SocketAddr::from(([127, 0, 0, 1], port));
    if TcpStream::connect_timeout(&local, Duration::from_millis(300)).is_ok() {
        return true;
    }
    let addr = (host, port)
        .to_socket_addrs()
        .ok()
        .and_then(|mut a| a.next())
        .unwrap_or(local);
    TcpListener::bind(addr).is_err()
}

fn require_port_free(host: &str, port: u16, label: &str) -> R {
    // A just-stopped process may need a moment to release the port.
    let deadline = Instant::now() + Duration::from_secs(5);
    while port_busy(host, port) {
        if Instant::now() > deadline {
            let hint = if label == "backend" {
                "Set PELTY_BIND to a free address (e.g. 127.0.0.1:8787) in .env and match PELTY_PUBLIC_URL."
            } else {
                "Free the port (or set PELTY_VITE_HOST / stop the other Vite)."
            };
            return Err(format!(
                "{label} port {port} is already in use by another program.\n  {hint}"
            ));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Ok(())
}

/// Stops a backend/Vite left over from an earlier `cargo xtask dev` in this checkout.
pub fn stop_prior(ctx: &Ctx) {
    let stopped = procs::stop_recorded(&ctx.root);
    if !stopped.is_empty() {
        println!(
            "stopped prior PeltyCMS dev process(es): {}",
            stopped
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(" ")
        );
    }
}

/// `cargo watch` when installed (restarts on changes), else `cargo run`.
/// No server console: the terminal is shared with Vite and Ctrl+C must reach xtask.
fn backend_cmd(ctx: &Ctx) -> Command {
    let mut c = ctx.cargo();
    if util::succeeds(ctx.cargo().args(["watch", "--version"])) {
        c.args([
            "watch",
            "-w",
            "crates",
            "-w",
            "migrations",
            "-w",
            "locales",
            "-x",
            "run -p pelty-server -- serve --no-console --no-setup",
        ]);
    } else {
        c.args([
            "run",
            "-p",
            "pelty-server",
            "--",
            "serve",
            "--no-console",
            "--no-setup",
        ]);
    }
    // Developers of PeltyCMS itself: no eula.txt prompt in the checkout.
    if ctx.get("PELTY_EULA").is_none() {
        c.env("PELTY_EULA", "true");
    }
    c
}

fn vite_cmd(ctx: &Ctx) -> R<Command> {
    let mut c = ctx.pnpm()?;
    c.arg("--dir")
        .arg(ctx.root.join("frontend"))
        .args(["exec", "vite", "--host", &vite_host(ctx)]);
    // Vite must reach the API on loopback even when PELTY_BIND is 0.0.0.0.
    c.env("PELTY_API", format!("http://127.0.0.1:{}", bind_port(ctx)));
    Ok(c)
}

/// The address to print for the UI: localhost, or this machine's LAN address
/// when Vite listens on all interfaces (no packets are sent).
fn ui_host(vite_host: &str) -> String {
    if vite_host == "127.0.0.1" || vite_host == "localhost" {
        return "localhost".into();
    }
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()))
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

pub fn dev(ctx: &Ctx) -> R {
    stop_prior(ctx);
    require_port_free(
        &bind(ctx)
            .rsplit_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_default(),
        bind_port(ctx),
        "backend",
    )?;
    require_port_free(&vite_host(ctx), VITE_PORT, "frontend")?;
    pg::ensure_server(ctx)?;
    ctx.frontend_deps()?;
    procs::catch_interrupts();
    let vh = vite_host(ctx);
    println!(
        "PeltyCMS UI → http://{}:{VITE_PORT}  (API :{}, vite --host {vh})  Ctrl+C stops both",
        ui_host(&vh),
        bind_port(ctx)
    );
    let mut vite = procs::spawn(&mut vite_cmd(ctx)?)?;
    // Fail fast if pnpm/vite died immediately (version mismatch, missing deps, ...).
    std::thread::sleep(Duration::from_millis(600));
    if let Ok(Some(st)) = vite.try_wait() {
        return Err(format!("Vite failed to start ({st}, see above)"));
    }
    let backend = match procs::spawn(&mut backend_cmd(ctx)) {
        Ok(b) => b,
        Err(e) => {
            procs::stop_children([&mut vite]);
            return Err(e);
        }
    };
    procs::supervise(&ctx.root, vec![("vite", vite), ("backend", backend)])
}

pub fn backend(ctx: &Ctx) -> R {
    stop_prior(ctx);
    require_port_free(
        &bind(ctx)
            .rsplit_once(':')
            .map(|(h, _)| h.to_string())
            .unwrap_or_default(),
        bind_port(ctx),
        "backend",
    )?;
    procs::catch_interrupts();
    let b = procs::spawn(&mut backend_cmd(ctx))?;
    procs::supervise(&ctx.root, vec![("backend", b)])
}

pub fn frontend(ctx: &Ctx) -> R {
    stop_prior(ctx);
    require_port_free(&vite_host(ctx), VITE_PORT, "frontend")?;
    ctx.frontend_deps()?;
    procs::catch_interrupts();
    let v = procs::spawn(&mut vite_cmd(ctx)?)?;
    procs::supervise(&ctx.root, vec![("vite", v)])
}

/// `cargo xtask stop [--pg]`. On Windows the dev PostgreSQL started by xtask
/// is stopped too (it runs hidden in the background there); elsewhere it keeps
/// running for the next `cargo dev` unless `--pg` is given.
pub fn stop(ctx: &Ctx, args: &[String]) -> R {
    stop_prior(ctx);
    println!("dev processes stopped");
    if cfg!(windows) || args.iter().any(|a| a == "--pg") {
        pg::stop_managed(ctx)?;
    }
    Ok(())
}

fn migrate(ctx: &Ctx) -> R {
    util::run(
        ctx.cargo()
            .args(["run", "-q", "-p", "pelty-server", "--", "migrate"]),
    )
}

/// Server up, database created (by `pelty-server migrate`) and migrated.
pub fn db(ctx: &Ctx) -> R {
    let db = pg::ensure_server(ctx)?;
    migrate(ctx)?;
    println!("database {} ready", db.name);
    Ok(())
}

pub fn db_reset(ctx: &Ctx) -> R {
    let db = pg::ensure_server(ctx)?;
    pg::reset(ctx, &db)?;
    migrate(ctx)?;
    println!("database {} recreated", db.name);
    Ok(())
}
