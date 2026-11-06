//! Local PostgreSQL for PeltyCMS: find system binaries, download Zonky
//! embedded-postgres into `data/pg/dist`, initdb/start/stop with `pg_ctl`.
//! Used by `pelty-server` (`PELTY_LOCAL_PG`) and by `cargo xtask` (dev).
//!
//! Prod path never uses Docker or apt; those stay in xtask.

use rand::RngExt;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub type R<T = ()> = Result<T, String>;

/// How the first `initdb` authenticates the superuser.
#[derive(Debug, Clone)]
pub enum Auth {
    /// Superuser `postgres`, trust (dev / xtask).
    Trust,
    /// Named superuser with a password (SCRAM-SHA-256 on first init).
    Password { user: String, password: String },
}

impl Auth {
    pub fn user(&self) -> &str {
        match self {
            Auth::Trust => "postgres",
            Auth::Password { user, .. } => user,
        }
    }

    pub fn password(&self) -> &str {
        match self {
            Auth::Trust => "",
            Auth::Password { password, .. } => password,
        }
    }
}

/// Where binaries and the data directory live, and how to auth.
#[derive(Debug, Clone)]
pub struct Config {
    /// Install / repo folder (relative `data/pg` is under this).
    pub root: PathBuf,
    /// Override for `PELTY_PG_HOME` / `PG_HOME`.
    pub home: Option<PathBuf>,
    /// Linux-as-root: run as this system user (default `pelty`).
    pub run_as: Option<String>,
    /// Zonky artifact version (default `16.4.0`).
    pub pg_ver: Option<String>,
    pub auth: Auth,
}

impl Config {
    /// Build from the process environment and an install root.
    pub fn from_env(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let home = std::env::var("PELTY_PG_HOME")
            .ok()
            .or_else(|| std::env::var("PG_HOME").ok())
            .filter(|v| !v.trim().is_empty())
            .map(PathBuf::from);
        let run_as = std::env::var("PG_USER")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let pg_ver = std::env::var("PG_VER")
            .ok()
            .filter(|v| !v.trim().is_empty());
        let auth = match (
            std::env::var("DATABASE_URL").ok().as_deref().and_then(|u| DbUrl::parse(u).ok()),
            std::env::var("PELTY_LOCAL_PG").ok(),
        ) {
            (Some(db), _) if !db.password.is_empty() => Auth::Password {
                user: db.user,
                password: db.password,
            },
            (_, Some(v)) if v == "1" || v.eq_ignore_ascii_case("true") => Auth::Password {
                user: "pelty".into(),
                password: generate_password(),
            },
            _ => Auth::Trust,
        };
        Config {
            root,
            home,
            run_as,
            pg_ver,
            auth,
        }
    }

    pub fn home_dir(&self) -> PathBuf {
        if let Some(h) = &self.home {
            return if h.is_absolute() {
                h.clone()
            } else {
                self.root.join(h)
            };
        }
        if is_root() {
            return PathBuf::from("/var/lib/pelty-pg");
        }
        if cfg!(unix) && Path::new("/workspace/pg").is_dir() {
            return PathBuf::from("/workspace/pg");
        }
        self.root.join("data").join("pg")
    }

    pub fn data_dir(&self) -> PathBuf {
        self.home_dir().join("data")
    }

    pub fn dist_bin(&self) -> PathBuf {
        self.home_dir().join("dist").join("bin")
    }
}

/// Parts of a `postgres://` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            user: if user.is_empty() {
                "postgres".into()
            } else {
                percent_decode(user)
            },
            password: percent_decode(password),
            host: if host.is_empty() {
                "localhost".into()
            } else {
                host
            },
            port: port.parse().unwrap_or(5432),
            name: if name.is_empty() {
                "postgres".into()
            } else {
                name.into()
            },
        })
    }

    pub fn is_local(&self) -> bool {
        matches!(self.host.as_str(), "127.0.0.1" | "localhost" | "::1")
    }

    pub fn admin_url(&self) -> String {
        self.with_db("postgres")
    }

    pub fn with_db(&self, name: &str) -> String {
        let auth = if self.password.is_empty() {
            percent_encode(&self.user)
        } else {
            format!(
                "{}:{}",
                percent_encode(&self.user),
                percent_encode(&self.password)
            )
        };
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("postgres://{auth}@{host}:{}/{name}", self.port)
    }

    pub fn to_url(&self) -> String {
        self.with_db(&self.name)
    }

    pub fn reachable(&self) -> bool {
        let Ok(addrs) = (self.host.as_str(), self.port).to_socket_addrs() else {
            return false;
        };
        addrs
            .into_iter()
            .any(|a| TcpStream::connect_timeout(&a, Duration::from_secs(1)).is_ok())
    }
}

/// Build a local URL for a managed instance.
pub fn local_url(auth: &Auth, port: u16, db: &str) -> String {
    DbUrl {
        user: auth.user().into(),
        password: auth.password().into(),
        host: "127.0.0.1".into(),
        port,
        name: db.into(),
    }
    .to_url()
}

/// Random password suitable for SCRAM / URL userinfo (no ambiguous punctuation).
pub fn generate_password() -> String {
    const ALPH: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..24)
        .map(|_| ALPH[rng.random_range(0..ALPH.len())] as char)
        .collect()
}

/// True when `PELTY_LOCAL_PG` is enabled.
pub fn local_enabled() -> bool {
    std::env::var("PELTY_LOCAL_PG").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Make sure a local server is up for `db` (download if needed). No-op when reachable.
pub fn ensure(cfg: &Config, db: &DbUrl) -> R {
    if db.reachable() {
        return Ok(());
    }
    if !db.is_local() {
        return Err(format!(
            "cannot reach PostgreSQL at {}:{} (DATABASE_URL)",
            db.host, db.port
        ));
    }
    start(cfg, db)?;
    wait_reachable(db, Duration::from_secs(30))
}

pub fn wait_reachable(db: &DbUrl, timeout: Duration) -> R {
    let deadline = Instant::now() + timeout;
    while !db.reachable() {
        if Instant::now() > deadline {
            return Err(format!("PostgreSQL did not come up on port {}", db.port));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

/// System bins → already-downloaded dist → download Zonky → start.
pub fn start(cfg: &Config, db: &DbUrl) -> R {
    if let Some(bin) = find_bin(cfg) {
        return start_native(cfg, &bin, db.port);
    }
    let bin = download(cfg).map_err(|e| {
        format!(
            "no PostgreSQL at {}:{}, none installed, and downloading failed:\n  {e}\n  \
             Fix the download (network / proxy; PG_VER picks the version) or install PostgreSQL 16+\n  \
             (https://www.postgresql.org/download/) and set DATABASE_URL,\n  \
             e.g. DATABASE_URL=postgres://postgres:<password>@127.0.0.1:5432/peltycms",
            db.host, db.port
        )
    })?;
    start_native(cfg, &bin, db.port)
}

/// Download only (no start). Returns the `dist/bin` path.
pub fn download(cfg: &Config) -> R<PathBuf> {
    let home = prepare_home(cfg)?;
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
        (_, a) => {
            return Err(format!(
                "no PostgreSQL download for {os}/{a}: install PostgreSQL 16+"
            ))
        }
    };
    let ver = cfg
        .pg_ver
        .clone()
        .or_else(|| std::env::var("PG_VER").ok())
        .unwrap_or_else(|| "16.4.0".into());
    let art = format!("embedded-postgres-binaries-{os}-{arch}");
    let url = format!(
        "https://repo1.maven.org/maven2/io/zonky/test/postgres/{art}/{ver}/{art}-{ver}.jar"
    );
    eprintln!("PostgreSQL not found; downloading PostgreSQL {ver} binaries ({url})");
    let jar = home.join("pg.jar");
    let curl = if cfg!(windows) {
        system32("curl.exe")
    } else {
        PathBuf::from("curl")
    };
    run(Command::new(curl)
        .args(["-fSL", "--retry", "2", &url, "-o"])
        .arg(&jar))?;
    let j = home.join("j");
    let _ = std::fs::remove_dir_all(&j);
    std::fs::create_dir_all(&j).map_err(|e| e.to_string())?;
    if cfg!(windows) {
        run(Command::new(system32("tar.exe"))
            .arg("-xf")
            .arg(&jar)
            .arg("-C")
            .arg(&j))?;
    } else {
        run(Command::new("unzip")
            .args(["-oq"])
            .arg(&jar)
            .current_dir(&j))?;
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
        return Err(format!(
            "the download has no {} / {} in {}",
            exe("initdb"),
            exe("pg_ctl"),
            bin.display()
        ));
    }
    if is_root() {
        let user = cfg.run_as.clone().unwrap_or_else(|| "pelty".into());
        let _ = Command::new("chown")
            .arg("-R")
            .arg(format!("{user}:"))
            .arg(&home)
            .status();
    }
    Ok(bin)
}

/// Stop the managed native server in `home/data` if running.
pub fn stop(cfg: &Config) -> R<bool> {
    let data = cfg.data_dir();
    let Some(bin) = find_bin(cfg) else {
        return Ok(false);
    };
    if !data.join("PG_VERSION").exists() {
        return Ok(false);
    }
    let pg_ctl = bin.join(exe("pg_ctl"));
    let running = as_pg(cfg, &pg_ctl)
        .arg("-D")
        .arg(&data)
        .arg("status")
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if running {
        run(as_pg(cfg, &pg_ctl)
            .arg("-D")
            .arg(&data)
            .args(["-m", "fast", "-w", "stop"]))?;
        println!("PostgreSQL stopped ({})", data.display());
    }
    Ok(running)
}

/// Print / run `pg_ctl status` for the managed data dir.
pub fn status(cfg: &Config) -> R {
    if let Some(bin) = find_bin(cfg) {
        let data = cfg.data_dir();
        if data.join("PG_VERSION").exists() {
            let mut c = as_pg(cfg, &bin.join(exe("pg_ctl")));
            c.arg("-D").arg(&data).arg("status");
            // Exit 3 = stopped; still an answer.
            let _ = status_cmd(&mut c)?;
            return Ok(());
        }
    }
    Err("no managed PostgreSQL data directory found".into())
}

/// `psql` on PATH or next to server bins.
pub fn psql(cfg: &Config) -> Option<PathBuf> {
    which("psql").or_else(|| {
        find_bin(cfg)
            .map(|d| d.join(exe("psql")))
            .filter(|p| p.is_file())
    })
}

/// Installed or downloaded bin directory.
pub fn find_bin(cfg: &Config) -> Option<PathBuf> {
    let mut cands: Vec<PathBuf> = vec![];
    let versioned = |base: &Path| -> Vec<PathBuf> {
        let mut v: Vec<(u32, PathBuf)> = std::fs::read_dir(base)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| {
                (
                    e.file_name()
                        .to_string_lossy()
                        .split('.')
                        .next()
                        .and_then(|n| n.parse().ok())
                        .unwrap_or(0),
                    e.path().join("bin"),
                )
            })
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
    cands.push(cfg.dist_bin());
    cands.into_iter().find(|d| has_server_bins(d))
}

fn mac_bins() -> Vec<PathBuf> {
    let mut v: Vec<(u32, PathBuf)> = vec![];
    for base in ["/opt/homebrew/opt", "/usr/local/opt"] {
        for e in std::fs::read_dir(base).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(rest) = name.strip_prefix("postgresql") {
                let n = rest
                    .strip_prefix('@')
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(1);
                v.push((n, e.path().join("bin")));
            }
        }
    }
    for e in std::fs::read_dir("/Applications/Postgres.app/Contents/Versions")
        .into_iter()
        .flatten()
        .flatten()
    {
        let n = e
            .file_name()
            .to_string_lossy()
            .split('.')
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(0);
        v.push((n, e.path().join("bin")));
    }
    v.sort_by_key(|e| std::cmp::Reverse(e.0));
    v.into_iter().map(|(_, p)| p).collect()
}

fn start_native(cfg: &Config, bin: &Path, port: u16) -> R {
    if is_root() && !cfg!(target_os = "linux") {
        return Err(
            "PostgreSQL refuses to run as root; run as your own user (not with sudo)".into(),
        );
    }
    let home = prepare_home(cfg)?;
    let data = home.join("data");
    if !data.join("PG_VERSION").exists() {
        let mut c = as_pg(cfg, &bin.join(exe("initdb")));
        c.arg("-D").arg(&data);
        match &cfg.auth {
            Auth::Trust => {
                c.args(["-U", "postgres", "-A", "trust"]);
            }
            Auth::Password { user, password } => {
                let pwfile = home.join("pwfile");
                std::fs::write(&pwfile, password)
                    .map_err(|e| format!("write {}: {e}", pwfile.display()))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = std::fs::set_permissions(&pwfile, std::fs::Permissions::from_mode(0o600));
                }
                c.args(["-U", user, "-A", "scram-sha-256", "--pwfile"])
                    .arg(&pwfile);
            }
        }
        if cfg!(windows) {
            c.args(["-E", "UTF8", "--no-locale"]);
        }
        run(c.stdout(std::process::Stdio::null()))?;
        if matches!(cfg.auth, Auth::Password { .. }) {
            let _ = std::fs::remove_file(home.join("pwfile"));
        }
    }
    let opts = if cfg!(windows) {
        format!("-p {port} -c listen_addresses=127.0.0.1")
    } else {
        format!("-p {port} -k /tmp -c listen_addresses=127.0.0.1")
    };
    eprintln!(
        "starting PostgreSQL ({}) on 127.0.0.1:{port}, data in {}",
        bin.display(),
        data.display()
    );
    let mut c = as_pg(cfg, &bin.join(exe("pg_ctl")));
    c.arg("-D")
        .arg(&data)
        .arg("-l")
        .arg(home.join("pg.log"))
        .args(["-w", "-o", &opts, "start"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        c.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    let _ = status_cmd(&mut c)?;
    println!("{}", local_url(&cfg.auth, port, "peltycms"));
    Ok(())
}

fn prepare_home(cfg: &Config) -> R<PathBuf> {
    let home = cfg.home_dir();
    std::fs::create_dir_all(&home).map_err(|e| format!("mkdir {}: {e}", home.display()))?;
    if is_root() {
        let user = cfg.run_as.clone().unwrap_or_else(|| "pelty".into());
        if !succeeds(Command::new("id").arg(&user)) {
            run(Command::new(sbin("useradd")).args(["-m", "-s", "/bin/bash", &user]))?;
        }
        let _ = Command::new("chown")
            .arg(format!("{user}:"))
            .arg(&home)
            .status();
    }
    Ok(home)
}

fn as_pg(cfg: &Config, program: &Path) -> Command {
    if is_root() {
        let user = cfg.run_as.clone().unwrap_or_else(|| "pelty".into());
        let mut c = Command::new(sbin("runuser"));
        c.args(["-u", &user, "--"]).arg(program);
        c
    } else {
        Command::new(program)
    }
}

fn is_root() -> bool {
    static ROOT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ROOT.get_or_init(|| {
        cfg!(unix)
            && Command::new("id")
                .arg("-u")
                .output()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
                .unwrap_or(false)
    })
}

fn sbin(name: &str) -> PathBuf {
    which(name).unwrap_or_else(|| Path::new("/usr/sbin").join(name))
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

fn has_server_bins(dir: &Path) -> bool {
    dir.join(exe("initdb")).is_file() && dir.join(exe("pg_ctl")).is_file()
}

fn system32(name: &str) -> PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let p = PathBuf::from(root).join("System32").join(name);
    if p.is_file() {
        p
    } else {
        PathBuf::from(name)
    }
}

fn unpack_txz(txz: &Path, dest: &Path) -> R {
    let tar_path = txz.with_extension("tar");
    {
        let mut input = std::io::BufReader::new(
            std::fs::File::open(txz).map_err(|e| format!("{}: {e}", txz.display()))?,
        );
        let mut out = std::io::BufWriter::new(
            std::fs::File::create(&tar_path).map_err(|e| format!("{}: {e}", tar_path.display()))?,
        );
        lzma_rs::xz_decompress(&mut input, &mut out)
            .map_err(|e| format!("{}: {e:?}", txz.display()))?;
        std::io::Write::flush(&mut out).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::open(&tar_path).map_err(|e| e.to_string())?;
    tar::Archive::new(file)
        .unpack(dest)
        .map_err(|e| format!("unpacking {}: {e}", txz.display()))?;
    let _ = std::fs::remove_file(&tar_path);
    Ok(())
}

fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 {
        return p.exists().then(|| p.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        let pe = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pe
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect();
        if p.extension().is_some() {
            v.insert(0, String::new());
        }
        v
    } else {
        vec![String::new()]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let cand = dir.join(format!("{name}{ext}"));
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

fn run(c: &mut Command) -> R {
    let st = status_cmd(c)?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("command failed ({st})"))
    }
}

fn status_cmd(c: &mut Command) -> R<std::process::ExitStatus> {
    c.status().map_err(|e| format!("cannot start process: {e}"))
}

fn succeeds(c: &mut Command) -> bool {
    c.stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_database_urls() {
        let d = DbUrl::parse("postgres://postgres@127.0.0.1:55432/peltycms").unwrap();
        assert_eq!(
            (d.user.as_str(), d.host.as_str(), d.port, d.name.as_str()),
            ("postgres", "127.0.0.1", 55432, "peltycms")
        );
        assert_eq!(
            d.admin_url(),
            "postgres://postgres@127.0.0.1:55432/postgres"
        );
        let d = DbUrl::parse("postgresql://u:p@db.example.com/x?sslmode=require").unwrap();
        assert_eq!(
            (d.password.as_str(), d.port, d.name.as_str(), d.is_local()),
            ("p", 5432, "x", false)
        );
        assert!(DbUrl::parse("postgres://localhost/y").unwrap().is_local());
        let d = DbUrl::parse("postgres://pelty:a%40b@127.0.0.1:5432/peltycms").unwrap();
        assert_eq!(d.password, "a@b");
    }

    #[test]
    fn home_defaults_under_root() {
        let cfg = Config {
            root: PathBuf::from("/srv/pelty"),
            home: None,
            run_as: None,
            pg_ver: None,
            auth: Auth::Trust,
        };
        if !is_root() {
            assert_eq!(cfg.home_dir(), PathBuf::from("/srv/pelty/data/pg"));
            assert_eq!(cfg.data_dir(), PathBuf::from("/srv/pelty/data/pg/data"));
        }
        let cfg = Config {
            root: PathBuf::from("/srv/pelty"),
            home: Some(PathBuf::from("custom/pg")),
            run_as: None,
            pg_ver: None,
            auth: Auth::Trust,
        };
        assert_eq!(cfg.home_dir(), PathBuf::from("/srv/pelty/custom/pg"));
    }

    #[test]
    fn download_skips_network_when_dist_present() {
        let tmp = std::env::temp_dir().join(format!("pelty-pg-{}", std::process::id()));
        let bin = tmp.join("dist").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for name in ["initdb", "pg_ctl", "postgres"] {
            std::fs::write(bin.join(exe(name)), b"").unwrap();
        }
        let cfg = Config {
            root: tmp.clone(),
            home: Some(tmp.clone()),
            run_as: None,
            pg_ver: None,
            auth: Auth::Trust,
        };
        assert_eq!(download(&cfg).unwrap(), bin);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn local_url_encodes_password() {
        let auth = Auth::Password {
            user: "pelty".into(),
            password: "a@b:c".into(),
        };
        assert_eq!(
            local_url(&auth, 5432, "peltycms"),
            "postgres://pelty:a%40b%3Ac@127.0.0.1:5432/peltycms"
        );
    }

    #[test]
    fn generate_password_is_long_enough() {
        assert!(generate_password().len() >= 20);
    }
}
