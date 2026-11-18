//! Tracing setup. Log lines go through [`Sink`]: straight to stdout, or, //! while the interactive console runs, through the line editor's external
//! printer so they never garble the prompt. The filter sits behind a reload
//! handle so `log level <filter>` can change it at runtime.
//!
//! Like a Minecraft server, `serve` / `setup` also keep `logs/latest.log`
//! (plain text, no colours): the previous one is gzipped to
//! `logs/YYYY-MM-DD-N.log.gz` on start, and old archives are pruned.
//! `PELTY_LOG_DIR` moves the folder, `PELTY_LOG_FILE=false` turns it off,
//! `PELTY_LOG_RETENTION_DAYS` (default 30, 0 = keep all) prunes archives.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Mutex, OnceLock};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{reload, EnvFilter, Layer, Registry};

type Printer = Box<dyn FnMut(String) + Send>;

/// Queue to the printer thread. Sending never blocks, so a log call can never
/// stall a runtime worker even while the line editor is not draining its
/// printer (e.g. between prompts while a console command runs).
static PRINTER: Mutex<Option<mpsc::Sender<String>>> = Mutex::new(None);
static FILTER: OnceLock<reload::Handle<EnvFilter, Registry>> = OnceLock::new();
static CURRENT: Mutex<String> = Mutex::new(String::new());
/// `logs/latest.log` while file logging is on.
static FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

thread_local! {
    static IN_PRINT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub const DEFAULT_FILTER: &str = "info,sqlx=warn,tower_http=info";

pub fn init(json: bool) {
    let spec = std::env::var("RUST_LOG")
        .ok()
        .filter(|v| EnvFilter::try_new(v).is_ok())
        .unwrap_or_else(|| DEFAULT_FILTER.into());
    let (filter, handle) = reload::Layer::new(EnvFilter::new(&spec));
    let fmt = if json {
        tracing_subscriber::fmt::layer()
            .json()
            .with_writer(Sink::default)
            .boxed()
    } else {
        tracing_subscriber::fmt::layer()
            .with_writer(Sink::default)
            .boxed()
    };
    let file = file_enabled().then(|| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(FileSink::default)
    });
    if tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
        .with(file)
        .try_init()
        .is_ok()
    {
        let _ = FILTER.set(handle);
        if let Ok(mut c) = CURRENT.lock() {
            *c = spec;
        }
    }
}

/// Replace the log filter (`info`, `debug`, `info,sqlx=debug`, ...).
pub fn set_filter(spec: &str) -> Result<(), String> {
    let f = EnvFilter::try_new(spec).map_err(|e| format!("invalid filter: {e}"))?;
    FILTER
        .get()
        .ok_or("logging is not initialised")?
        .reload(f)
        .map_err(|e| e.to_string())?;
    if let Ok(mut c) = CURRENT.lock() {
        *c = spec.to_string();
    }
    Ok(())
}

pub fn current_filter() -> String {
    CURRENT.lock().map(|c| c.clone()).unwrap_or_default()
}

/// Route log lines through `p` (the console's external printer); `None` = stdout.
pub fn set_printer(p: Option<Printer>) {
    let tx = p.map(|mut p| {
        let (tx, rx) = mpsc::channel::<String>();
        let _ = std::thread::Builder::new()
            .name("log-printer".into())
            .spawn(move || {
                for line in rx {
                    IN_PRINT.with(|f| f.set(true));
                    p(line);
                }
            });
        tx
    });
    // Dropping the old sender ends the old printer thread once it drains.
    if let Ok(mut g) = PRINTER.lock() {
        *g = tx;
    }
}

/// One log event; emitted as a whole when dropped.
#[derive(Default)]
pub struct Sink(Vec<u8>);

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Sink {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        let buf = std::mem::take(&mut self.0);
        // Lines logged from the printer thread itself go straight to stdout.
        if !IN_PRINT.with(|f| f.get()) {
            let line = String::from_utf8_lossy(&buf).into_owned();
            let sent = match PRINTER.lock() {
                Ok(g) => g.as_ref().map(|tx| tx.send(line).is_ok()).unwrap_or(false),
                Err(_) => false,
            };
            if sent {
                return;
            }
        }
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(&buf);
        let _ = out.flush();
    }
}

/// Print a line to stdout and append it to `logs/latest.log`.
pub fn say(line: &str) {
    println!("{line}");
    to_file(line);
}

/// Print a line to stderr and append it to `logs/latest.log`.
pub fn say_err(line: &str) {
    eprintln!("{line}");
    to_file(line);
}

/// Append one non-tracing line (timestamped, colours stripped) to the file.
pub fn to_file(line: &str) {
    if !file_enabled() {
        return;
    }
    // Same shape as the tracing lines: 2026-11-18T22:43:27.875970Z.
    let ts = time::OffsetDateTime::now_utc()
        .format(time::macros::format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:6]Z"
        ))
        .unwrap_or_default();
    let mut text = String::new();
    for l in strip_ansi(line).lines() {
        text.push_str(&format!("{ts} {l}\n"));
    }
    write_file(text.as_bytes());
}

fn file_enabled() -> bool {
    FILE.lock().map(|g| g.is_some()).unwrap_or(false)
}

fn write_file(b: &[u8]) {
    if let Ok(mut g) = FILE.lock() {
        if let Some(f) = g.as_mut() {
            let _ = f.write_all(b);
            let _ = f.flush();
        }
    }
}

/// Settings for the log folder, from the environment / `.env`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileCfg {
    pub dir: PathBuf,
    pub enabled: bool,
    pub retention_days: u64,
}

impl FileCfg {
    pub fn from(root: &Path, get: &dyn Fn(&str) -> Option<String>) -> FileCfg {
        let dir = PathBuf::from(
            // Folder for latest.log and the dated .log.gz archives (relative = under the server folder).
            get("PELTY_LOG_DIR")
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| "logs".into()),
        );
        let dir = if dir.is_absolute() {
            dir
        } else {
            root.join(dir)
        };
        let off = |v: String| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        };
        // Write logs/latest.log (Minecraft style) on serve and setup; false turns it off.
        let enabled = !get("PELTY_LOG_FILE").is_some_and(off);
        let retention_days =
            // Days to keep rotated logs/*.log.gz archives (default 30; 0 keeps all).
            get("PELTY_LOG_RETENTION_DAYS")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(30);
        FileCfg {
            dir,
            enabled,
            retention_days,
        }
    }
}

/// Rotate the previous `latest.log`, prune old archives, open a fresh one.
/// Returns the path of `latest.log`; on error, file logging stays off.
pub fn init_file(cfg: &FileCfg) -> Result<Option<PathBuf>, String> {
    if !cfg.enabled {
        return Ok(None);
    }
    std::fs::create_dir_all(&cfg.dir).map_err(|e| format!("{}: {e}", cfg.dir.display()))?;
    let latest = cfg.dir.join("latest.log");
    if latest.exists() {
        rotate(&cfg.dir, &latest)?;
    }
    if cfg.retention_days > 0 {
        prune(
            &cfg.dir,
            std::time::Duration::from_secs(cfg.retention_days * 86_400),
        );
    }
    let f = std::fs::File::create(&latest).map_err(|e| format!("{}: {e}", latest.display()))?;
    if let Ok(mut g) = FILE.lock() {
        *g = Some(f);
    }
    Ok(Some(latest))
}

/// `latest.log` -> `YYYY-MM-DD-N.log.gz` (date of its last write, UTC).
fn rotate(dir: &Path, latest: &Path) -> Result<PathBuf, String> {
    let modified = std::fs::metadata(latest)
        .and_then(|m| m.modified())
        .unwrap_or_else(|_| std::time::SystemTime::now());
    let date = time::OffsetDateTime::from(modified).date();
    let day = format!(
        "{:04}-{:02}-{:02}",
        date.year(),
        u8::from(date.month()),
        date.day()
    );
    let target = archive_name(dir, &day);
    // Move it first: on Windows this fails while another running server
    // still writes to it, and then nothing is archived twice.
    let moving = dir.join("latest.log.rotating");
    std::fs::rename(latest, &moving).map_err(|e| {
        format!(
            "{}: {e} (is another pelty-server running here?)",
            latest.display()
        )
    })?;
    let raw = std::fs::read(&moving).map_err(|e| format!("{}: {e}", moving.display()))?;
    let out = std::fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
    gz.write_all(&raw)
        .and_then(|_| gz.finish().map(|_| ()))
        .map_err(|e| format!("{}: {e}", target.display()))?;
    let _ = std::fs::remove_file(&moving);
    Ok(target)
}

/// First free `day-N.log.gz` (N from 1), Minecraft style.
fn archive_name(dir: &Path, day: &str) -> PathBuf {
    (1..)
        .map(|n| dir.join(format!("{day}-{n}.log.gz")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{day}.log.gz")))
}

/// Delete `*.log.gz` archives older than `keep`.
fn prune(dir: &Path, keep: std::time::Duration) {
    let now = std::time::SystemTime::now();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.to_string_lossy().ends_with(".log.gz") {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| now.duration_since(m).ok())
            .is_some_and(|age| age > keep);
        if old {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Drop ANSI escape sequences (`ESC [ ... letter`, `ESC ] ... BEL`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match it.peek() {
            Some('[') => {
                it.next();
                for c in it.by_ref() {
                    if c.is_ascii_alphabetic() || c == '~' {
                        break;
                    }
                }
            }
            Some(']') => {
                it.next();
                while let Some(c) = it.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && it.peek() == Some(&'\\') {
                        it.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Tracing writer for `latest.log`; one event per drop.
#[derive(Default)]
pub struct FileSink(Vec<u8>);

impl Write for FileSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for FileSink {
    fn drop(&mut self) {
        if self.0.is_empty() {
            return;
        }
        let text = strip_ansi(&String::from_utf8_lossy(&self.0));
        write_file(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("pelty-logs-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn strips_colours() {
        assert_eq!(
            strip_ansi("\u{1b}[2m2026\u{1b}[0m \u{1b}[31mERROR\u{1b}[0m x"),
            "2026 ERROR x"
        );
        assert_eq!(strip_ansi("plain [PeltyCMS] ok"), "plain [PeltyCMS] ok");
        assert_eq!(strip_ansi("\u{1b}]8;;http://x\u{7}link"), "link");
    }

    #[test]
    fn config_from_env() {
        let root = Path::new("/srv/pelty");
        let none = |_: &str| None;
        let c = FileCfg::from(root, &none);
        assert_eq!(c.dir, root.join("logs"));
        assert!(c.enabled);
        assert_eq!(c.retention_days, 30);
        let set = |k: &str| match k {
            "PELTY_LOG_DIR" => Some("/var/log/pelty".to_string()),
            "PELTY_LOG_FILE" => Some("false".to_string()),
            "PELTY_LOG_RETENTION_DAYS" => Some("0".to_string()),
            _ => None,
        };
        let c = FileCfg::from(root, &set);
        assert_eq!(c.dir, PathBuf::from("/var/log/pelty"));
        assert!(!c.enabled);
        assert_eq!(c.retention_days, 0);
    }

    #[test]
    fn rotates_latest_into_numbered_gz() {
        use std::io::Read;
        let d = tmp();
        let latest = d.join("latest.log");
        std::fs::write(&latest, "first run\n").unwrap();
        let a = rotate(&d, &latest).unwrap();
        assert!(!latest.exists());
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("-1.log.gz"), "{name}");
        assert_eq!(name.len(), "YYYY-MM-DD-1.log.gz".len());
        std::fs::write(&latest, "second run\n").unwrap();
        let b = rotate(&d, &latest).unwrap();
        assert!(b.to_string_lossy().ends_with("-2.log.gz"));
        let mut s = String::new();
        flate2::read::GzDecoder::new(std::fs::File::open(&a).unwrap())
            .read_to_string(&mut s)
            .unwrap();
        assert_eq!(s, "first run\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn prune_keeps_recent_and_other_files() {
        let d = tmp();
        std::fs::write(d.join("2026-01-01-1.log.gz"), b"x").unwrap();
        std::fs::write(d.join("notes.txt"), b"x").unwrap();
        prune(&d, std::time::Duration::from_secs(3600));
        assert!(d.join("2026-01-01-1.log.gz").exists());
        prune(&d, std::time::Duration::ZERO);
        std::thread::sleep(std::time::Duration::from_millis(10));
        prune(&d, std::time::Duration::ZERO);
        assert!(!d.join("2026-01-01-1.log.gz").exists());
        assert!(d.join("notes.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
