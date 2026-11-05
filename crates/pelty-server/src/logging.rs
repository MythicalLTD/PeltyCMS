//! Tracing setup. Log lines go through [`Sink`]: straight to stdout, or, //! while the interactive console runs, through the line editor's external
//! printer so they never garble the prompt. The filter sits behind a reload
//! handle so `log level <filter>` can change it at runtime.
use std::io::Write;
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
    if tracing_subscriber::registry()
        .with(filter)
        .with(fmt)
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
