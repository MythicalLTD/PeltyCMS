//! Development PostgreSQL. A reachable DATABASE_URL is used as is (any local
//! install, Docker container or remote server). When it points at this
//! machine and nothing listens, a dev server is started on its port, trying in order:
//! 1. PostgreSQL binaries already installed (`/usr/lib/postgresql/*/bin`,
//!    `pg_ctl` on PATH, Homebrew `postgresql@*` (keg-only, so often not on
//!    PATH) and Postgres.app on macOS, `C:\Program Files\PostgreSQL\*\bin`
//!    on Windows),
//!    with the data dir in `PG_HOME` (default `data/pg`);
//! 2. Docker (`postgres:16`, container `peltycms-dev-postgres`, trust auth);
//! 3. Linux as root with apt: `apt-get install postgresql`;
//! 4. zonky embedded-postgres binaries (PostgreSQL `PG_VER`, Linux, macOS and
//!    Windows x64) downloaded from Maven Central into `PG_HOME/dist`.
//!
//! `PELTY_DEV_PG=native|docker` forces one of the first two.
//! initdb/postgres refuse to run as root, so as root on Linux everything runs
//! as the system user `PG_USER` (default `pelty`, created when missing); on
//! macOS run `cargo dev` as your own user.
use crate::util::{self, which, Ctx, R};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub const CONTAINER: &str = "peltycms-dev-postgres";
const VOLUME: &str = "peltycms-dev-pgdata";

/// The parts of DATABASE_URL the dev commands need.
pub struct DbUrl {
    pub user: String,
    pub password: String,
    pub host: String,
    pub port: u16,
    pub name: String,
}

impl DbUrl {
    pub fn parse(url: &str) -> R<DbUrl> {
        let rest = url
            .strip_prefix("postgres://")
            .or_else(|| url.strip_prefix("postgresql://"))
            .ok_or_else(|| format!("DATABASE_URL must start with postgres://, got {url}"))?;
        let rest = rest.split('?').next().unwrap_or_default();
        let (auth, hostpath) = match rest.rsplit_once('@') {
            Some((a, h)) => (a, h),
            None => ("", rest),
        };
        let (user, password) = auth.split_once(':').unwrap_or((auth, ""));
        let (hostport, name) = hostpath.split_once('/').unwrap_or((hostpath, ""));
        let (host, port) = match hostport.strip_prefix('[') {
            Some(v6) => {
                let (h, p) = v6.split_once(']').unwrap_or((v6, ""));
                (h.to_string(), p.trim_start_matches(':').to_string())
            }
            None => match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.to_string()),
                None => (hostport.to_string(), String::new()),
            },
        };
        Ok(DbUrl {
            user: if user.is_empty() { "postgres".into() } else { user.into() },
            password: password.into(),
            host: if host.is_empty() { "localhost".into() } else { host },
            port: port.parse().unwrap_or(5432),
            name: if name.is_empty() { "postgres".into() } else { name.into() },
        })
    }

    pub fn is_local(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
    }

    /// Same server, `postgres` maintenance database.
    pub fn admin_url(&self) -> String {
        let auth = if self.password.is_empty() { self.user.clone() } else { format!("{}:{}", self.user, self.password) };
        let host = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        format!("postgres://{auth}@{host}:{}/postgres", self.port)
    }

    pub fn reachable(&self) -> bool {
        let Ok(addrs) = (self.host.as_str(), self.port).to_socket_addrs() else { return false };
        addrs.into_iter().any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(1)).is_ok())
    }
}

pub fn db_url(ctx: &Ctx) -> R<DbUrl> {
    DbUrl::parse(&ctx.get("DATABASE_URL").ok_or("DATABASE_URL is not set (add it to .env)")?)
}

/// Makes sure the DATABASE_URL server is up (the database itself is created
/// by `pelty-server migrate` / `serve`).
pub fn ensure_server(ctx: &Ctx) -> R<DbUrl> {
    let db = db_url(ctx)?;
    if db.reachable() {
        return Ok(db);
    }
    if !db.is_local() {
        return Err(format!("cannot reach PostgreSQL at {}:{} (DATABASE_URL)", db.host, db.port));
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

fn is_root() -> bool {
    static ROOT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ROOT.get_or_init(|| {
        cfg!(unix) && Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0").unwrap_or(false)
    })
}

/// `name` on PATH or in /usr/sbin (`su` without `-` on Debian leaves it off PATH).
fn sbin(name: &str) -> PathBuf {
    which(name).unwrap_or_else(|| Path::new("/usr/sbin").join(name))
}

fn pg_home(ctx: &Ctx) -> PathBuf {
    if let Some(h) = ctx.get("PG_HOME") {
        return PathBuf::from(h);
    }
    if is_root() {
        return PathBuf::from("/var/lib/pelty-pg");
    }
    if cfg!(unix) && Path::new("/workspace/pg").is_dir() {
        return PathBuf::from("/workspace/pg");
    }
    ctx.root.join("data").join("pg")
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn has_server_bins(dir: &Path) -> bool {
    dir.join(exe("initdb")).is_file() && dir.join(exe("pg_ctl")).is_file()
}

/// Installed PostgreSQL bin directory, newest version first.
fn system_bin(ctx: &Ctx) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = vec![];
    let versioned = |base: &Path| -> Vec<PathBuf> {
        let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().split('.').next().and_then(|n| n.parse().ok()).unwrap_or(0), e.path().join("bin")))
            .collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.0));
        v.into_iter().map(|(_, p)| p).collect()
    };
    if cfg!(windows) {
        if let Some(pg_ctl) = which("pg_ctl") {
            cands.extend(pg_ctl.parent().map(Path::to_path_buf));
        }
        for var in ["ProgramFiles", "ProgramW6432"] {
            if let Some(pf) = std::env::var_os(var) {
                cands.extend(versioned(&PathBuf::from(pf).join("PostgreSQL")));
            }
        }
    } else {
        cands.extend(versioned(Path::new("/usr/lib/postgresql")));
        if let Some(pg_ctl) = which("pg_ctl") {
            cands.extend(pg_ctl.parent().map(Path::to_path_buf));
        }
        if cfg!(target_os = "macos") {
            cands.extend(mac_bins());
        }
    }
    // Downloaded on an earlier run.
    cands.push(pg_home(ctx).join("dist").join("bin"));
    cands.into_iter().find(|d| has_server_bins(d))
}

/// macOS: Homebrew kegs (`/opt/homebrew` on Apple silicon, `/usr/local` on
/// Intel; `postgresql@16` is keg-only) and Postgres.app, newest first.
fn mac_bins() -> Vec<PathBuf> {
    let mut v: Vec<(u32, PathBuf)> = vec![];
    for base in ["/opt/homebrew/opt", "/usr/local/opt"] {
        for e in std::fs::read_dir(base).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("postgresql") {
                // `postgresql` (unversioned formula) sorts after the numbered ones.
                let n = rest.strip_prefix('@').and_then(|n| n.parse().ok()).unwrap_or(1);
                v.push((n, e.path().join("bin")));
            }
        }
    }
    for e in std::fs::read_dir("/Applications/Postgres.app/Contents/Versions").into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().split('.').next().and_then(|n| n.parse().ok()).unwrap_or(0);
        v.push((n, e.path().join("bin")));
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// `psql`, for db-reset: PATH, then the server bin dirs.
pub fn psql(ctx: &Ctx) -> Option<PathBuf> {
    which("psql").or_else(|| system_bin(ctx).map(|d| d.join(exe("psql"))).filter(|p| p.is_file()))
}

pub fn docker_ok() -> bool {
    which("docker").is_some() && util::succeeds(Command::new("docker").arg("info"))
}

/// Starts a dev server for DATABASE_URL's port.
pub fn start(ctx: &Ctx, db: &DbUrl) -> R {
    let mode = ctx.get("PELTY_DEV_PG").unwrap_or_default();
    if mode != "docker" {
        if let Some(bin) = system_bin(ctx) {
            return start_native(ctx, &bin, db.port);
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
            Command::new("apt-get").args(["install", "-y", "postgresql"]).env("DEBIAN_FRONTEND", "noninteractive").status().map(|s| s.success()).unwrap_or(false)
        };
        if !install() {
            let _ = Command::new("apt-get").arg("update").status();
            install();
        }
        if let Some(bin) = system_bin(ctx) {
            return start_native(ctx, &bin, db.port);
        }
    }
    let bin = download(ctx).map_err(|e| {
        format!(
            "no PostgreSQL reachable at {}:{}, none installed, Docker not running, and downloading one failed:\n  {e}\n  \
             Fix the download (network / proxy; PG_VER picks the version), start Docker Desktop, or install PostgreSQL 16+\n  \
             (https://www.postgresql.org/download/) and set DATABASE_URL in .env to it,\n  \
             e.g. DATABASE_URL=postgres://postgres:<password>@127.0.0.1:5432/peltycms",
            db.host, db.port
        )
    })?;
    start_native(ctx, &bin, db.port)
}

/// Runs a PostgreSQL binary, as `PG_USER` when we are root.
fn as_pg(ctx: &Ctx, program: &Path) -> Command {
    if is_root() {
        let user = ctx.get("PG_USER").unwrap_or_else(|| "pelty".into());
        let mut c = Command::new(sbin("runuser"));
        c.args(["-u", &user, "--"]).arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn prepare_home(ctx: &Ctx) -> R<PathBuf> {
    let home = pg_home(ctx);
    std::fs::create_dir_all(&home).map_err(|e| format!("mkdir {}: {e}", home.display()))?;
    if is_root() {
        let user = ctx.get("PG_USER").unwrap_or_else(|| "pelty".into());
        if !util::succeeds(Command::new("id").arg(&user)) {
            util::run(Command::new(sbin("useradd")).args(["-m", "-s", "/bin/bash", &user]))?;
        }
        let _ = Command::new("chown").arg(format!("{user}:")).arg(&home).status();
    }
    Ok(home)
}

fn start_native(ctx: &Ctx, bin: &Path, port: u16) -> R {
    if is_root() && !cfg!(target_os = "linux") {
        return Err("PostgreSQL refuses to run as root; run `cargo dev` as your own user (not with sudo)".into());
    }
    let home = prepare_home(ctx)?;
    let data = home.join("data");
    if !data.join("PG_VERSION").exists() {
        let mut c = as_pg(ctx, &bin.join(exe("initdb")));
        c.arg("-D").arg(&data).args(["-U", "postgres", "-A", "trust"]);
        if cfg!(windows) {
            // The Windows default (system code page) cannot hold UTF-8 text.
            c.args(["-E", "UTF8", "--no-locale"]);
        }
        util::run(c.stdout(std::process::Stdio::null()))?;
    }
    let opts = if cfg!(windows) {
        format!("-p {port} -c listen_addresses=127.0.0.1")
    } else {
        format!("-p {port} -k /tmp -c listen_addresses=127.0.0.1")
    };
    eprintln!("starting PostgreSQL ({}) on 127.0.0.1:{port}, data in {}", bin.display(), data.display());
    let mut c = as_pg(ctx, &bin.join(exe("pg_ctl")));
    c.arg("-D").arg(&data).arg("-l").arg(home.join("pg.log")).args(["-w", "-o", &opts, "start"]);
    #[cfg(windows)]
    {
        // Own process group: the console's Ctrl+C (stopping `cargo dev`) must not
        // reach the server; `cargo xtask stop` / `pg stop` shut it down cleanly.
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    // Like the old script: "already running" is not an error, reachability is checked afterwards.
    let _ = util::status(&mut c)?;
    println!("DATABASE_URL=postgres://postgres@127.0.0.1:{port}/peltycms");
    Ok(())
}

fn docker_exists() -> bool {
    Command::new("docker")
        .args(["ps", "-a", "--filter", &format!("name=^/{CONTAINER}$"), "--format", "{{.Names}}"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == CONTAINER)
        .unwrap_or(false)
}

fn start_docker(db: &DbUrl) -> R {
    if docker_exists() {
        eprintln!("starting Docker container {CONTAINER}");
        util::run(Command::new("docker").args(["start", CONTAINER]).stdout(std::process::Stdio::null()))?;
    } else {
        eprintln!("starting PostgreSQL 16 in Docker ({CONTAINER}, volume {VOLUME}) on 127.0.0.1:{}", db.port);
        let mut c = Command::new("docker");
        c.args(["run", "-d", "--name", CONTAINER, "-p", &format!("127.0.0.1:{}:5432", db.port)])
            .args(["-e", &format!("POSTGRES_USER={}", db.user), "-e", &format!("POSTGRES_DB={}", db.name)])
            .args(["-v", &format!("{VOLUME}:/var/lib/postgresql/data")]);
        if db.password.is_empty() {
            c.args(["-e", "POSTGRES_HOST_AUTH_METHOD=trust"]);
        } else {
            c.args(["-e", &format!("POSTGRES_PASSWORD={}", db.password)]);
        }
        c.arg("postgres:16");
        util::run(c.stdout(std::process::Stdio::null()))?;
    }
    // The image's first-start init server has no TCP listener, so a TCP
    // pg_isready inside the container only succeeds on the real server.
    let deadline = Instant::now() + Duration::from_secs(90);
    while !util::succeeds(Command::new("docker").args(["exec", CONTAINER, "pg_isready", "-h", "127.0.0.1", "-U", &db.user])) {
        if Instant::now() > deadline {
            return Err(format!("PostgreSQL in {CONTAINER} did not become ready (docker logs {CONTAINER})"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// zonky embedded-postgres-binaries into `PG_HOME/dist`. The jar comes with
/// curl (shipped with Windows 10+ as curl.exe) and is unpacked with unzip, or
/// the bsdtar `tar.exe` on Windows; the `.txz` inside is unpacked in Rust.
fn download(ctx: &Ctx) -> R<PathBuf> {
    let home = prepare_home(ctx)?;
    let dist = home.join("dist");
    let bin = dist.join("bin");
    if has_server_bins(&bin) && bin.join(exe("postgres")).is_file() {
        return Ok(bin);
    }
    let os = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };
    let arch = match (os, std::env::consts::ARCH) {
        (_, "x86_64") => "amd64",
        ("linux" | "darwin", "aarch64") => "arm64v8",
        (_, a) => return Err(format!("no PostgreSQL download for {os}/{a}: install PostgreSQL 16+ or Docker")),
    };
    let ver = ctx.get("PG_VER").unwrap_or_else(|| "16.4.0".into());
    let art = format!("embedded-postgres-binaries-{os}-{arch}");
    let url = format!("https://repo1.maven.org/maven2/io/zonky/test/postgres/{art}/{ver}/{art}-{ver}.jar");
    eprintln!("PostgreSQL not found; downloading PostgreSQL {ver} binaries ({url})");
    let jar = home.join("pg.jar");
    let curl = if cfg!(windows) { system32("curl.exe") } else { PathBuf::from("curl") };
    util::run(Command::new(curl).args(["-fSL", "--retry", "2", &url, "-o"]).arg(&jar))?;
    let j = home.join("j");
    let _ = std::fs::remove_dir_all(&j);
    std::fs::create_dir_all(&j).map_err(|e| e.to_string())?;
    if cfg!(windows) {
        util::run(Command::new(system32("tar.exe")).arg("-xf").arg(&jar).arg("-C").arg(&j))?;
    } else {
        util::run(Command::new("unzip").args(["-oq"]).arg(&jar).current_dir(&j))?;
    }
    let txz = std::fs::read_dir(&j)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().is_some_and(|x| x == "txz"))
        .ok_or("no .txz in the downloaded jar")?;
    let _ = std::fs::remove_dir_all(&dist);
    std::fs::create_dir_all(&dist).map_err(|e| e.to_string())?;
    unpack_txz(&txz, &dist)?;
    let _ = std::fs::remove_dir_all(&j);
    let _ = std::fs::remove_file(&jar);
    if !has_server_bins(&bin) {
        return Err(format!("the download has no {} / {} in {}", exe("initdb"), exe("pg_ctl"), bin.display()));
    }
    if is_root() {
        let user = ctx.get("PG_USER").unwrap_or_else(|| "pelty".into());
        let _ = Command::new("chown").arg("-R").arg(format!("{user}:")).arg(&home).status();
    }
    Ok(bin)
}

/// `%SystemRoot%\System32\<name>`, so a curl/tar from Git or MSYS on PATH is not picked.
fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let p = PathBuf::from(root).join("System32").join(name);
    if p.is_file() {
        p
    } else {
        PathBuf::from(name)
    }
}

/// Unpacks a `.tar.xz` (pure Rust: no xz or tar program needed).
fn unpack_txz(txz: &Path, dest: &Path) -> R {
    let tar_path = txz.with_extension("tar");
    {
        let mut input = std::io::BufReader::new(std::fs::File::open(txz).map_err(|e| format!("{}: {e}", txz.display()))?);
        let mut out = std::io::BufWriter::new(std::fs::File::create(&tar_path).map_err(|e| format!("{}: {e}", tar_path.display()))?);
        lzma_rs::xz_decompress(&mut input, &mut out).map_err(|e| format!("{}: {e:?}", txz.display()))?;
        std::io::Write::flush(&mut out).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::open(&tar_path).map_err(|e| e.to_string())?;
    tar::Archive::new(file).unpack(dest).map_err(|e| format!("unpacking {}: {e}", txz.display()))?;
    let _ = std::fs::remove_file(&tar_path);
    Ok(())
}

/// Stops the native dev server in `PG_HOME` if it is running (fast shutdown).
/// Returns whether there was one.
pub fn stop_managed(ctx: &Ctx) -> R<bool> {
    let data = pg_home(ctx).join("data");
    let Some(bin) = system_bin(ctx) else { return Ok(false) };
    if !data.join("PG_VERSION").exists() {
        return Ok(false);
    }
    let pg_ctl = bin.join(exe("pg_ctl"));
    let running = as_pg(ctx, &pg_ctl).arg("-D").arg(&data).arg("status").stdout(std::process::Stdio::null()).status().is_ok_and(|s| s.success());
    if running {
        util::run(as_pg(ctx, &pg_ctl).arg("-D").arg(&data).args(["-m", "fast", "-w", "stop"]))?;
        println!("dev PostgreSQL stopped ({})", data.display());
    }
    Ok(running)
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
            if let Some(bin) = system_bin(ctx) {
                let data = pg_home(ctx).join("data");
                if data.join("PG_VERSION").exists() {
                    let mut c = as_pg(ctx, &bin.join(exe("pg_ctl")));
                    c.arg("-D").arg(&data).arg(sub);
                    // `pg_ctl status` exits 3 when stopped; that is an answer, not an error.
                    return if sub == "status" { util::status(&mut c).map(|_| ()) } else { util::run(&mut c) };
                }
            }
            if docker_ok() && docker_exists() {
                return if sub == "stop" {
                    util::run(Command::new("docker").args(["stop", CONTAINER]))
                } else {
                    util::run(Command::new("docker").args(["ps", "-a", "--filter", &format!("name=^/{CONTAINER}$")]))
                };
            }
            if sub == "status" {
                println!("{}:{} {}", db.host, db.port, if db.reachable() { "reachable" } else { "not reachable" });
                return Ok(());
            }
            Err("no dev PostgreSQL started by xtask was found (stop your own server yourself)".into())
        }
        _ => Err("usage: cargo xtask pg start|stop|status".into()),
    }
}

/// Drops and recreates DATABASE_URL's database (psql, or psql inside the Docker container).
pub fn reset(ctx: &Ctx, db: &DbUrl) -> R {
    let drop = format!("drop database if exists \"{}\" with (force)", db.name);
    let create = format!("create database \"{}\"", db.name);
    if let Some(psql) = psql(ctx) {
        return util::run(Command::new(psql).arg(db.admin_url()).args(["-qc", &drop, "-c", &create]));
    }
    if docker_ok() && docker_exists() {
        return util::run(Command::new("docker").args(["exec", CONTAINER, "psql", "-U", &db.user, "-d", "postgres", "-qc", &drop, "-c", &create]));
    }
    Err("db-reset needs psql (PostgreSQL client tools on PATH) or the xtask Docker container".into())
}

#[cfg(test)]
mod tests {
    use super::DbUrl;

    #[test]
    fn parses_database_urls() {
        let d = DbUrl::parse("postgres://postgres@127.0.0.1:55432/peltycms").unwrap();
        assert_eq!((d.user.as_str(), d.host.as_str(), d.port, d.name.as_str()), ("postgres", "127.0.0.1", 55432, "peltycms"));
        assert_eq!(d.admin_url(), "postgres://postgres@127.0.0.1:55432/postgres");
        let d = DbUrl::parse("postgresql://u:p@db.example.com/x?sslmode=require").unwrap();
        assert_eq!((d.password.as_str(), d.port, d.name.as_str(), d.is_local()), ("p", 5432, "x", false));
        assert!(DbUrl::parse("postgres://localhost/y").unwrap().is_local());
    }
}
