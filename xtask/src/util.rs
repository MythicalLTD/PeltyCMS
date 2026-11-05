//! Shared helpers: repo root, `.env` loading, program lookup (`.exe`/`.cmd`
//! on Windows), pnpm via corepack, and running commands.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

pub type R<T = ()> = Result<T, String>;

/// Repository root (the directory above `xtask/`).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives in the repo root").to_path_buf()
}

/// The environment every command runs with: the process environment plus
/// `.env` (values already set in the environment win, like scripts/dev.sh did)
/// plus the dev defaults for `PELTY_ADDONS_DIR` and `PELTY_BIND`.
pub struct Ctx {
    pub root: PathBuf,
    /// Variables added on top of the inherited environment.
    pub extra: BTreeMap<String, String>,
}

impl Ctx {
    /// Without `.env`: only the repo root (build, docs, lint, ...).
    pub fn plain() -> Ctx {
        Ctx { root: root(), extra: BTreeMap::new() }
    }

    /// Loads `.env`, creating it from `.env.example` when missing.
    pub fn with_dotenv() -> R<Ctx> {
        let mut ctx = Ctx::plain();
        let env_file = ctx.root.join(".env");
        if !env_file.exists() {
            let example = ctx.root.join(".env.example");
            if example.exists() {
                std::fs::copy(&example, &env_file).map_err(|e| format!("copy .env.example: {e}"))?;
                println!("created .env from .env.example");
            }
        }
        if let Ok(text) = std::fs::read_to_string(&env_file) {
            for (k, v) in parse_dotenv(&text) {
                if std::env::var_os(&k).is_none() && !ctx.extra.contains_key(&k) {
                    ctx.extra.insert(k, v);
                }
            }
        }
        let addons = ctx.root.join("data").join("addons").to_string_lossy().into_owned();
        ctx.default("PELTY_ADDONS_DIR", &addons);
        ctx.default("PELTY_BIND", "127.0.0.1:8080");
        Ok(ctx)
    }

    fn default(&mut self, k: &str, v: &str) {
        if self.get(k).is_none() {
            self.extra.insert(k.into(), v.into());
        }
    }

    /// A variable from the environment or `.env`.
    pub fn get(&self, k: &str) -> Option<String> {
        std::env::var(k).ok().or_else(|| self.extra.get(k).cloned()).filter(|v| !v.is_empty())
    }

    /// `program` resolved on PATH (so `pnpm.cmd` / `corepack.cmd` work on
    /// Windows), started in the repo root with the loaded environment.
    pub fn cmd(&self, program: &str) -> Command {
        let mut c = Command::new(which(program).unwrap_or_else(|| PathBuf::from(program)));
        c.current_dir(&self.root).envs(&self.extra);
        c
    }

    pub fn cargo(&self) -> Command {
        // CARGO points at the cargo that runs xtask (set by `cargo run`).
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let mut c = if Path::new(&cargo).is_absolute() { Command::new(cargo) } else { self.cmd(&cargo) };
        c.current_dir(&self.root).envs(&self.extra);
        c
    }

    /// pnpm at the version pinned in frontend/package.json "packageManager",
    /// through corepack when available (a globally activated pnpm or stale shim
    /// cannot then trip ERR_PNPM_BAD_PM_VERSION), else a pnpm on PATH.
    pub fn pnpm(&self) -> R<Command> {
        let pkg = std::fs::read_to_string(self.root.join("frontend").join("package.json")).unwrap_or_default();
        let wanted = serde_json::from_str::<serde_json::Value>(&pkg)
            .ok()
            .and_then(|v| v.get("packageManager")?.as_str()?.strip_prefix("pnpm@").map(String::from))
            .unwrap_or_else(|| "12.16.1".into());
        if which("corepack").is_some() {
            static ENABLED: std::sync::Once = std::sync::Once::new();
            ENABLED.call_once(|| {
                let _ = succeeds(self.cmd("corepack").arg("enable"));
            });
            let mut c = self.cmd("corepack");
            // A read-only corepack cache (e.g. a system COREPACK_HOME) falls back to the user cache.
            if let Some(home) = std::env::var_os("COREPACK_HOME") {
                let home = PathBuf::from(home);
                if std::fs::create_dir_all(&home).is_err() || !dir_writable(&home) {
                    if let Some(user_home) = home_dir() {
                        c.env("COREPACK_HOME", user_home.join(".cache").join("node").join("corepack"));
                    }
                }
            }
            c.env("COREPACK_ENABLE_DOWNLOAD_PROMPT", "0");
            c.arg(format!("pnpm@{wanted}"));
            return Ok(c);
        }
        if which("pnpm").is_some() {
            return Ok(self.cmd("pnpm"));
        }
        Err(format!("pnpm not found and no corepack: install Node.js 20+ (ships corepack) or: npm i -g pnpm@{wanted}"))
    }

    /// `pnpm install --frozen-lockfile` in frontend/ when node_modules is missing.
    pub fn frontend_deps(&self) -> R {
        if self.root.join("frontend").join("node_modules").is_dir() {
            return Ok(());
        }
        let mut c = self.pnpm()?;
        c.current_dir(self.root.join("frontend")).args(["install", "--frozen-lockfile"]);
        run(&mut c)
    }
}

/// `KEY=value` lines; `#` comments and blank lines are skipped. Values are
/// taken literally (no quote stripping), matching scripts/dev.sh.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.trim().to_string(), v.to_string())))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".xtask-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from)
}

/// Finds `name` on PATH. On Windows tries every PATHEXT extension
/// (`.exe`, `.cmd`, `.bat`, ...) so npm-style shims like `pnpm.cmd` resolve.
pub fn which(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.components().count() > 1 {
        return p.exists().then(|| p.to_path_buf());
    }
    let exts: Vec<String> = if cfg!(windows) {
        let pe = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        let mut v: Vec<String> = pe.split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect();
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
            if is_executable(&cand) {
                return Some(cand);
            }
        }
    }
    None
}

fn is_executable(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(m) if m.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            {
                true
            }
        }
        _ => false,
    }
}

/// Runs to completion; a non-zero exit is an error naming the command.
pub fn run(c: &mut Command) -> R {
    let st = status(c)?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("`{}` failed ({st})", describe(c)))
    }
}

pub fn status(c: &mut Command) -> R<ExitStatus> {
    c.status().map_err(|e| format!("cannot start `{}`: {e}", describe(c)))
}

/// True when the command exits 0 (output discarded).
pub fn succeeds(c: &mut Command) -> bool {
    c.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

pub fn describe(c: &Command) -> String {
    let prog = Path::new(c.get_program()).file_name().unwrap_or(OsStr::new("?")).to_string_lossy().into_owned();
    std::iter::once(prog).chain(c.get_args().map(|a| a.to_string_lossy().into_owned())).collect::<Vec<_>>().join(" ")
}

/// Removes a directory tree if it exists.
pub fn remove_dir(p: &Path) -> R {
    match std::fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", p.display())),
    }
}

/// Removes a file or directory tree if it exists.
pub fn remove_path(p: &Path) -> R {
    if p.is_dir() {
        return remove_dir(p);
    }
    match std::fs::remove_file(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("remove {}: {e}", p.display())),
    }
}

/// Copies a directory tree.
pub fn copy_dir(from: &Path, to: &Path) -> R {
    std::fs::create_dir_all(to).map_err(|e| format!("mkdir {}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from).map_err(|e| format!("read {}: {e}", from.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let dst = to.join(entry.file_name());
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_dir(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), &dst).map_err(|e| format!("copy {}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}
