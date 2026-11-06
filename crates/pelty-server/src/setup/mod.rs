//! Interactive first-run setup wizard (`pelty-server setup`, or automatically
//! when `serve` starts in a terminal while `.env` says `PELTY_SETUP=pending`).
//!
//! The questions only touch settings that exist in the code: `.env` keys read
//! by `pelty_core::config` (database, bind, public URL, cookies, SMTP, folders)
//! and the General Settings / currencies stored in the database. Nothing is
//! written until the summary is confirmed. Prompts and live checks sit behind
//! [`Prompt`] and [`Services`] so the flow is tested with scripted answers.
mod ai;
pub mod term;

use crate::bootstrap;
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

/// `.env` key marking whether the wizard has run (`pending` | `done`).
pub const MARKER: &str = "PELTY_SETUP";

pub trait Prompt {
    fn say(&mut self, text: &str);
    /// Free text; an empty answer means "keep the default".
    fn input(&mut self, q: &str, default: &str) -> io::Result<String>;
    /// Hidden input (never echoed); `confirm` asks twice.
    fn secret(&mut self, q: &str, confirm: bool) -> io::Result<String>;
    fn confirm(&mut self, q: &str, default: bool) -> io::Result<bool>;
    /// One of `items` (index); `default` is preselected.
    fn select(&mut self, q: &str, items: &[&str], default: usize) -> io::Result<usize>;
    /// Any number of `items` (indexes, in order); `defaults` are preselected.
    fn multi_select(
        &mut self,
        q: &str,
        items: &[&str],
        defaults: &[bool],
    ) -> io::Result<Vec<usize>>;
}

#[derive(Debug, PartialEq)]
pub enum DbCheck {
    Ok,
    /// The server answered but the database does not exist (SQLSTATE 3D000).
    MissingDatabase,
    Failed(String),
}

/// Live side effects the wizard needs before the server runs.
pub trait Services {
    fn check_db(&mut self, url: &str) -> DbCheck;
    fn create_db(&mut self, url: &str) -> Result<(), String>;
    fn send_test_mail(&mut self, smtp_url: &str, from: &str, to: &str) -> Result<(), String>;
    /// Currency codes in the database (enabled first); empty when unknown.
    fn currencies(&mut self, url: &str) -> Vec<String>;
    /// `PING` a Redis URL; the round trip in milliseconds.
    fn redis_ping(&mut self, url: &str) -> Result<u128, String>;
}

#[derive(Debug)]
pub enum Error {
    /// EULA declined.
    Declined,
    /// Summary not confirmed; nothing written.
    Cancelled,
    Io(io::Error),
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Site {
    pub company_name: String,
    pub email: String,
    pub currency: String,
    pub language: String,
    pub timezone: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Admin {
    /// Display name (`users.display_name`).
    pub name: String,
    pub email: String,
    pub password: String,
    /// Generated: print it once after the account exists.
    pub generated: bool,
}

/// Work left for after the migrations (database settings and the admin).
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub site: Option<Site>,
    pub admin: Option<Admin>,
    pub captcha: Option<Captcha>,
    /// Gateways to install and enable after the migrations.
    pub gateways: Vec<GatewayPlan>,
    /// Daily billing run hour (company time zone) for Admin > Automation; `None` = unchanged.
    pub billing_hour: Option<i16>,
    /// `pelty-server setup` in a terminal: offer to start the server after.
    pub ask_start: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GatewayPlan {
    pub addon: String,
    pub label: String,
    /// Secret name -> env var NAME (the values go into `.env`).
    pub secret_refs: Vec<(String, String)>,
    pub test_mode: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Captcha {
    /// Addon id (`captcha-turnstile`, `captcha-internal`, ...).
    pub provider: String,
    pub label: String,
    pub site_key: String,
    pub secret_key: String,
}

#[derive(Debug, Clone, Default)]
pub struct Answers {
    pub eula: bool,
    /// Keys written into `.env`.
    pub env: BTreeMap<String, String>,
    pub plan: Plan,
}

// validation

pub fn valid_email(e: &str) -> Result<String, String> {
    let e = e.trim();
    match e.split_once('@') {
        Some((a, b))
            if !a.is_empty()
                && b.contains('.')
                && !b.starts_with('.')
                && !b.ends_with('.')
                && e.len() <= 254
                && !e.contains(char::is_whitespace) =>
        {
            Ok(e.to_string())
        }
        _ => Err("enter an email address like you@example.com".into()),
    }
}

pub fn valid_bind(v: &str) -> Result<String, String> {
    v.trim()
        .parse::<std::net::SocketAddr>()
        .map(|a| a.to_string())
        .map_err(|_| "use host:port, e.g. 127.0.0.1:8080 or 0.0.0.0:8080".into())
}

pub fn valid_public_url(v: &str) -> Result<String, String> {
    let v = v.trim().trim_end_matches('/');
    match url::Url::parse(v) {
        Ok(u) if (u.scheme() == "http" || u.scheme() == "https") && u.host_str().is_some() => {
            Ok(v.to_string())
        }
        _ => Err("must be an absolute http:// or https:// URL".into()),
    }
}

pub fn valid_pg_url(v: &str) -> Result<String, String> {
    let v = v.trim();
    match url::Url::parse(v) {
        Ok(u)
            if (u.scheme() == "postgres" || u.scheme() == "postgresql")
                && u.host_str().is_some() =>
        {
            Ok(v.to_string())
        }
        _ => Err("must look like postgres://user:password@host:5432/database".into()),
    }
}

/// PostgreSQL database names the wizard will create: plain identifiers only.
pub fn valid_db_name(v: &str) -> Result<String, String> {
    let v = v.trim();
    if !v.is_empty()
        && v.len() <= 63
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Ok(v.to_string())
    } else {
        Err("letters, digits, _ and - only (max 63)".into())
    }
}

pub fn valid_port(v: &str) -> Result<String, String> {
    match v.trim().parse::<u16>() {
        Ok(p) if p > 0 => Ok(p.to_string()),
        _ => Err("a port number between 1 and 65535".into()),
    }
}

/// Same rule as `general_settings::validate`.
pub fn valid_timezone(v: &str) -> Result<String, String> {
    let v = v.trim();
    if !v.is_empty()
        && v.len() <= 64
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/_+-".contains(c))
    {
        Ok(v.to_string())
    } else {
        Err("an IANA time zone such as UTC or Europe/Bucharest".into())
    }
}

pub fn valid_currency(v: &str) -> Result<String, String> {
    let v = v.trim().to_ascii_uppercase();
    if v.len() == 3 && v.chars().all(|c| c.is_ascii_uppercase()) {
        Ok(v)
    } else {
        Err("a three-letter ISO 4217 code, e.g. EUR, USD, RON".into())
    }
}

/// Minimum length enforced by the password policy default.
pub const MIN_PASSWORD: usize = 12;

// .env I/O

fn quote(v: &str) -> String {
    if v.is_empty() || !v.chars().any(|c| c.is_whitespace() || "#\"'".contains(c)) {
        v.to_string()
    } else if !v.contains('"') {
        format!("\"{v}\"")
    } else {
        format!("'{v}'")
    }
}

fn line_key(line: &str) -> Option<(&str, bool)> {
    let t = line.trim_start();
    let (t, commented) = match t.strip_prefix('#') {
        Some(rest) => (rest.trim_start(), true),
        None => (t, false),
    };
    let t = t.strip_prefix("export ").unwrap_or(t);
    let (k, _) = t.split_once('=')?;
    let k = k.trim();
    (!k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
    .then_some((k, commented))
}

/// Set `updates` in `.env` text: an active `KEY=` line is replaced in place,
/// otherwise the first commented `# KEY=` example is, otherwise the key is
/// appended. Comments, unknown keys and order are kept.
pub fn merge_env(text: &str, updates: &BTreeMap<String, String>) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut rest = Vec::new();
    for (k, v) in updates {
        let active = lines
            .iter()
            .position(|l| line_key(l) == Some((k.as_str(), false)));
        let target = active.or_else(|| {
            lines
                .iter()
                .position(|l| line_key(l) == Some((k.as_str(), true)))
        });
        let new = format!("{k}={}", quote(v));
        match target {
            Some(i) => lines[i] = new,
            None => rest.push(new),
        }
    }
    if !rest.is_empty() {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(
            "# --- Set by pelty-server setup ----------------------------------------------".into(),
        );
        lines.extend(rest);
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Write `.env` (private: mode 0600 on Unix) through a temp file + rename.
pub fn write_env(root: &Path, updates: &BTreeMap<String, String>) -> io::Result<()> {
    let path = root.join(bootstrap::ENV_FILE);
    let old = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            bootstrap::env_template(&bootstrap::generate_key())
        }
        Err(e) => return Err(e),
    };
    let tmp = root.join(".env.setup-tmp");
    let _ = std::fs::remove_file(&tmp);
    {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(merge_env(&old, updates).as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &path)
}

/// `eula.txt` with `eula=true` (other lines kept).
pub fn accept_eula(root: &Path) -> io::Result<()> {
    let path = root.join(crate::eula::FILE);
    let now = time::OffsetDateTime::now_utc()
        .replace_nanosecond(0)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        crate::eula::template(
            &now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default(),
        )
    });
    let mut found = false;
    let mut out: Vec<String> = text
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("eula=") {
                found = true;
                "eula=true".to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        out.push("eula=true".into());
    }
    std::fs::write(&path, out.join("\n") + "\n")
}

/// Does `.env` ask for the wizard (`PELTY_SETUP=pending`)?
pub fn pending(root: &Path) -> bool {
    bootstrap::read_env_file(root)
        .iter()
        .any(|(k, v)| k == MARKER && v.trim() == "pending")
}

// helpers

fn ask<P: Prompt + ?Sized>(
    p: &mut P,
    q: &str,
    default: &str,
    check: impl Fn(&str) -> Result<String, String>,
) -> io::Result<String> {
    loop {
        let v = p.input(q, default)?;
        let v = if v.trim().is_empty() {
            default.to_string()
        } else {
            v
        };
        match check(&v) {
            Ok(v) => return Ok(v),
            Err(e) => p.say(&format!("  ! {e}")),
        }
    }
}

fn any(v: &str) -> Result<String, String> {
    Ok(v.trim().to_string())
}

fn required(v: &str) -> Result<String, String> {
    let v = v.trim();
    if v.is_empty() {
        Err("required".into())
    } else {
        Ok(v.to_string())
    }
}

/// Hide the password in a URL for display.
pub fn mask_url(u: &str) -> String {
    match url::Url::parse(u) {
        Ok(mut x) if x.password().is_some() => {
            let _ = x.set_password(Some("****"));
            x.to_string()
        }
        _ => u.to_string(),
    }
}

pub struct PgParts {
    pub host: String,
    pub port: String,
    pub user: String,
    pub password: String,
    pub db: String,
}

pub fn pg_parts(u: &str) -> PgParts {
    let x = url::Url::parse(u).ok();
    let dec = |s: &str| {
        percent_encoding::percent_decode_str(s)
            .decode_utf8_lossy()
            .into_owned()
    };
    PgParts {
        host: x
            .as_ref()
            .and_then(|x| x.host_str().map(str::to_string))
            .unwrap_or_else(|| "127.0.0.1".into()),
        port: x
            .as_ref()
            .and_then(|x| x.port())
            .unwrap_or(5432)
            .to_string(),
        user: x
            .as_ref()
            .map(|x| dec(x.username()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "postgres".into()),
        password: x
            .as_ref()
            .and_then(|x| x.password().map(dec))
            .unwrap_or_default(),
        db: x
            .as_ref()
            .map(|x| x.path().trim_start_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "peltycms".into()),
    }
}

pub fn pg_url(p: &PgParts) -> String {
    let mut u = url::Url::parse("postgres://localhost").expect("static url");
    let _ = u.set_host(Some(&p.host));
    let _ = u.set_port(p.port.parse().ok());
    let _ = u.set_username(&p.user);
    if !p.password.is_empty() {
        let _ = u.set_password(Some(&p.password));
    }
    u.set_path(&p.db);
    u.to_string()
}

fn smtp_url(host: &str, port: &str, security: &str, user: &str, password: &str) -> String {
    let mut u = url::Url::parse(if security == "tls" {
        "smtps://localhost"
    } else {
        "smtp://localhost"
    })
    .expect("static url");
    let _ = u.set_host(Some(host));
    let _ = u.set_port(port.parse().ok());
    if !user.is_empty() {
        let _ = u.set_username(user);
        let _ = u.set_password(Some(password));
    }
    if security == "starttls" {
        u.set_query(Some("tls=required"));
    }
    u.to_string()
}

// steps

/// Cloudflare edge ranges (https://www.cloudflare.com/ips/), for the preset.
pub const CLOUDFLARE: &str = "173.245.48.0/20,103.21.244.0/22,103.22.200.0/22,103.31.4.0/22,141.101.64.0/18,108.162.192.0/18,190.93.240.0/20,188.114.96.0/20,197.234.240.0/22,198.41.128.0/17,162.158.0.0/15,104.16.0.0/13,104.24.0.0/14,172.64.0.0/13,131.0.72.0/22,2400:cb00::/32,2606:4700::/32,2803:f800::/32,2405:b500::/32,2405:8100::/32,2a06:98c0::/29,2c0f:f248::/32";
const LOCAL_PROXY: &str = "127.0.0.1,::1";

/// Common IANA zones for the picker; "Other" allows any valid name.
pub const TIMEZONES: &[&str] = &[
    "UTC",
    "Europe/London",
    "Europe/Dublin",
    "Europe/Lisbon",
    "Europe/Paris",
    "Europe/Berlin",
    "Europe/Amsterdam",
    "Europe/Brussels",
    "Europe/Madrid",
    "Europe/Rome",
    "Europe/Vienna",
    "Europe/Zurich",
    "Europe/Prague",
    "Europe/Warsaw",
    "Europe/Budapest",
    "Europe/Bucharest",
    "Europe/Sofia",
    "Europe/Athens",
    "Europe/Helsinki",
    "Europe/Stockholm",
    "Europe/Oslo",
    "Europe/Copenhagen",
    "Europe/Kyiv",
    "Europe/Istanbul",
    "Europe/Moscow",
    "America/New_York",
    "America/Chicago",
    "America/Denver",
    "America/Los_Angeles",
    "America/Toronto",
    "America/Vancouver",
    "America/Mexico_City",
    "America/Sao_Paulo",
    "America/Buenos_Aires",
    "Africa/Cairo",
    "Africa/Johannesburg",
    "Africa/Lagos",
    "Asia/Dubai",
    "Asia/Kolkata",
    "Asia/Singapore",
    "Asia/Hong_Kong",
    "Asia/Shanghai",
    "Asia/Tokyo",
    "Asia/Seoul",
    "Asia/Jakarta",
    "Australia/Sydney",
    "Australia/Perth",
    "Pacific/Auckland",
];

pub const COMMON_CURRENCIES: &[&str] = &[
    "EUR", "USD", "GBP", "RON", "CHF", "PLN", "CZK", "HUF", "BGN", "SEK", "NOK", "DKK", "TRY",
    "CAD", "AUD", "NZD", "JPY", "CNY", "HKD", "SGD", "INR", "AED", "KWD", "BRL", "MXN", "ZAR",
];

pub const SECTIONS: [&str; 9] = [
    "License",
    "Database",
    "Web and reverse proxy",
    "Company and localisation",
    "Administrator",
    "Outgoing email",
    "Captcha",
    "Payment gateways",
    "Advanced",
];

/// A captcha provider the wizard can offer (addon id, label, needs keys).
#[derive(Debug, Clone, PartialEq)]
pub struct CaptchaProvider {
    pub id: String,
    pub label: String,
    pub needs_keys: bool,
}

/// Captcha providers available on this install: the built-in challenge and
/// every bundled `captcha-*` addon in `dist` (release builds may ship none).
pub fn captcha_providers(dist: &Path) -> Vec<CaptchaProvider> {
    let mut v = vec![CaptchaProvider {
        id: "captcha-internal".into(),
        label: "Built-in arithmetic challenge (no keys needed)".into(),
        needs_keys: false,
    }];
    let mut ids: Vec<String> = std::fs::read_dir(dist)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_string_lossy()
                .strip_suffix(".zip")
                .filter(|n| n.starts_with("captcha-"))
                .map(str::to_string)
        })
        .collect();
    ids.sort();
    for id in ids {
        let label = match id.as_str() {
            "captcha-turnstile" => "Cloudflare Turnstile".to_string(),
            "captcha-hcaptcha" => "hCaptcha".into(),
            "captcha-recaptcha-v2" => "Google reCAPTCHA v2".into(),
            "captcha-recaptcha-v3" => "Google reCAPTCHA v3".into(),
            other => other.to_string(),
        };
        v.push(CaptchaProvider {
            id,
            label,
            needs_keys: true,
        });
    }
    v
}

/// What the wizard knows besides the answers: current values and lists.
pub struct Ctx<'a> {
    pub cur: &'a dyn Fn(&str) -> Option<String>,
    pub eula_accepted: bool,
    /// Languages with a locale file (`en`, `ro`, ...).
    pub languages: Vec<String>,
    /// Currency codes from the database (enabled first); empty = type one.
    pub currencies: Vec<String>,
    /// Captcha providers; empty = the section is skipped.
    pub captcha: Vec<CaptchaProvider>,
    /// Bundled gateway packages; empty = the section is skipped.
    pub gateways: Vec<GatewayChoice>,
}

/// A payment gateway package the wizard can enable.
#[derive(Debug, Clone, PartialEq)]
pub struct GatewayChoice {
    /// Addon id (`gateway-stripe`).
    pub addon: String,
    /// Gateway id it provides (`stripe`).
    pub provides: String,
    pub label: String,
    /// Secret names it needs (`secret_key`, `webhook_secret`).
    pub secrets: Vec<String>,
}

/// Gateway packages (`gateway-*.zip`) in the addon dist folder, by name.
pub fn gateway_choices(dist: &Path) -> Vec<GatewayChoice> {
    let mut v: Vec<GatewayChoice> = std::fs::read_dir(dist)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            e.file_name().to_string_lossy().starts_with("gateway-")
                && e.file_name().to_string_lossy().ends_with(".zip")
        })
        .filter_map(|e| {
            let bytes = std::fs::read(e.path()).ok()?;
            let m = pelty_extensions::addon_pkg::inspect(&bytes, pelty_api::addons::PKG_LIMITS)
                .ok()?
                .manifest;
            let s = |k: &str| m.get(k).and_then(|v| v.as_str()).map(str::to_string);
            if s("kind").as_deref() != Some("gateway") {
                return None;
            }
            let secrets = m
                .pointer("/capabilities/secrets")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str())
                        .filter(|x| *x != "*")
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            Some(GatewayChoice {
                addon: s("id")?,
                provides: s("provides")?,
                label: s("name")?,
                secrets,
            })
        })
        .collect();
    v.sort_by(|a, b| a.label.cmp(&b.label));
    v
}

/// Environment variable holding a gateway secret (`PELTY_GW_STRIPE_SECRET_KEY`).
pub fn gateway_env(provides: &str, secret: &str) -> String {
    format!("PELTY_GW_{}_{}", provides, secret)
        .to_ascii_uppercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

impl Ctx<'_> {
    fn c(&self, k: &str, d: &str) -> String {
        (self.cur)(k)
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| d.to_string())
    }
}

fn header<P: Prompt + ?Sized>(p: &mut P, n: usize, help: &str) {
    p.say(&format!(
        "\n{} {}",
        console::style(format!("[{}/{}]", n + 1, SECTIONS.len()))
            .cyan()
            .bold(),
        console::style(SECTIONS[n]).bold()
    ));
    if !help.is_empty() {
        p.say(&console::style(format!("  {help}")).dim().to_string());
    }
}

/// Rough strength label shown after a typed password.
pub fn strength(pw: &str) -> &'static str {
    let classes = [
        pw.chars().any(|c| c.is_ascii_lowercase()),
        pw.chars().any(|c| c.is_ascii_uppercase()),
        pw.chars().any(|c| c.is_ascii_digit()),
        pw.chars().any(|c| !c.is_ascii_alphanumeric()),
    ]
    .iter()
    .filter(|b| **b)
    .count();
    match (pw.chars().count(), classes) {
        (n, c) if n >= 20 && c >= 3 => "strong",
        (n, c) if n >= 14 && c >= 3 => "good",
        (n, c) if n >= 12 && c >= 2 => "fair",
        _ => "weak",
    }
}

fn pick<P: Prompt + ?Sized>(
    p: &mut P,
    q: &str,
    items: &[String],
    current: &str,
) -> io::Result<String> {
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let d = items.iter().position(|i| i == current).unwrap_or(0);
    Ok(items[p.select(q, &refs, d)?].clone())
}

/// Run every section, then the summary (save / edit a section / cancel).
/// `cur` gives the current value of a key (environment, then `.env`).
pub fn wizard<P: Prompt + ?Sized, S: Services + ?Sized>(
    p: &mut P,
    svc: &mut S,
    ctx: &mut Ctx,
) -> Result<Answers, Error> {
    let mut a = Answers::default();
    p.say(&format!(
        "\n{}\n{}",
        console::style(format!("PeltyCMS {} setup", env!("CARGO_PKG_VERSION"))).bold().green(),
        console::style("Enter keeps the value in [brackets]; optional sections can be skipped; nothing is saved until you confirm the summary.").dim()
    ));
    for n in 0..SECTIONS.len() {
        section(n, p, svc, ctx, &mut a)?;
    }
    loop {
        p.say(&summary(&a));
        let mut items = vec!["Save and finish".to_string()];
        items.extend(SECTIONS[1..].iter().map(|s| format!("Edit: {s}")));
        items.push("Cancel (nothing is saved)".into());
        let refs: Vec<&str> = items.iter().map(String::as_str).collect();
        match p.select("What now?", &refs, 0)? {
            0 => break,
            i if i == items.len() - 1 => return Err(Error::Cancelled),
            i => section(i, p, svc, ctx, &mut a)?,
        }
    }
    a.env.insert(MARKER.into(), "done".into());
    Ok(a)
}

fn section<P: Prompt + ?Sized, S: Services + ?Sized>(
    n: usize,
    p: &mut P,
    svc: &mut S,
    ctx: &mut Ctx,
    a: &mut Answers,
) -> Result<(), Error> {
    match n {
        0 => license(p, ctx, a),
        1 => database(p, svc, ctx, a),
        2 => web(p, ctx, a),
        3 => company(p, ctx, a),
        4 => admin(p, ctx, a),
        5 => mail(p, svc, a),
        6 => captcha(p, ctx, a),
        7 => gateways(p, ctx, a),
        _ => advanced(p, svc, ctx, a),
    }
}

fn license<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(p, 0, "Source-available Free Tier: production use free with 50 or fewer Active Clients. Above that you need a Commercial License; the panel does not lock features for overage.");
    if ctx.eula_accepted {
        p.say("  EULA already accepted (eula.txt / PELTY_EULA).");
    } else {
        p.say(&format!(
            "  Read LICENSE, EULA.md and COMMERCIAL.md (published EULA: {}).",
            crate::eula::URL
        ));
        p.say("  Free Tier: ≤50 Active Clients (status active, not merged), or non-production.");
        p.say("  Above 50 Active Clients: buy a Commercial License. Soft notices only; no feature lock for overage.");
        p.say("  Accepting records agreement (same as eula=true in eula.txt).");
        if !p.confirm("Do you accept the PeltyCMS License and EULA?", false)? {
            return Err(Error::Declined);
        }
    }
    a.eula = true;
    Ok(())
}

fn database<P: Prompt + ?Sized, S: Services + ?Sized>(
    p: &mut P,
    svc: &mut S,
    ctx: &mut Ctx,
    a: &mut Answers,
) -> Result<(), Error> {
    header(
        p,
        1,
        "PostgreSQL 16+. A missing database can be created for you.",
    );
    let mut url = a.env.get("DATABASE_URL").cloned().unwrap_or_else(|| {
        ctx.c(
            "DATABASE_URL",
            "postgres://postgres@127.0.0.1:5432/peltycms",
        )
    });
    loop {
        if p.confirm("Enter the connection as one postgres:// URL? (No = host, port, user, password, database)", false)? {
            url = ask(p, "PostgreSQL URL", &url, valid_pg_url)?;
        } else {
            let d = pg_parts(&url);
            let host = ask(p, "Host", &d.host, required)?;
            let port = ask(p, "Port", &d.port, valid_port)?;
            let user = ask(p, "User", &d.user, required)?;
            let pw = p.secret(if d.password.is_empty() { "Password (Enter = none)" } else { "Password (Enter = keep current)" }, false)?;
            let password = if pw.is_empty() { d.password } else { pw };
            let db = ask(p, "Database name", &d.db, valid_db_name)?;
            url = pg_url(&PgParts { host, port, user, password, db });
        }
        p.say(&format!("  Testing {} ...", mask_url(&url)));
        let mut res = svc.check_db(&url);
        if res == DbCheck::MissingDatabase {
            let db = pg_parts(&url).db;
            if p.confirm(
                &format!("Database \"{db}\" does not exist. Create it now?"),
                true,
            )? {
                match svc.create_db(&url) {
                    Ok(()) => {
                        p.say(&format!("  Created database \"{db}\"."));
                        res = svc.check_db(&url);
                    }
                    Err(e) => p.say(&format!("  ! could not create it: {e}")),
                }
            }
        }
        match res {
            DbCheck::Ok => {
                p.say(&console::style("  Connection OK.").green().to_string());
                ctx.currencies = svc.currencies(&url);
                break;
            }
            DbCheck::MissingDatabase => p.say("  ! the database does not exist"),
            DbCheck::Failed(e) => p.say(&format!("  ! {e}")),
        }
        if !p.confirm("Try again?", true)? {
            p.say("  Keeping it; the server will not start until this database is reachable.");
            break;
        }
    }
    a.env.insert("DATABASE_URL".into(), url);
    Ok(())
}

const PROXIES: [&str; 5] = [
    "No proxy (clients connect directly)",
    "nginx or Apache on this machine",
    "Caddy on this machine",
    "Cloudflare (proxied DNS)",
    "Other proxy / load balancer (enter its IPs)",
];

fn web<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(p, 2, "Where the server listens, the address people use, and whether a reverse proxy sits in front.");
    let old_bind = a
        .env
        .get("PELTY_BIND")
        .cloned()
        .unwrap_or_else(|| ctx.c("PELTY_BIND", "127.0.0.1:8080"));
    let bind = ask(
        p,
        "Listen address (host:port; 0.0.0.0 = all interfaces)",
        &old_bind,
        valid_bind,
    )?;
    let port_of = |b: &str| b.rsplit(':').next().unwrap_or("8080").to_string();
    let local = format!("http://localhost:{}", port_of(&bind));
    // Keep a real public URL; follow the port while it is still the local default.
    let public_default = match a
        .env
        .get("PELTY_PUBLIC_URL")
        .cloned()
        .or_else(|| (ctx.cur)("PELTY_PUBLIC_URL"))
        .filter(|v| !v.trim().is_empty())
    {
        Some(u)
            if u.trim_end_matches('/') != format!("http://localhost:{}", port_of(&old_bind)) =>
        {
            u
        }
        _ => local,
    };
    let public = ask(
        p,
        "Public URL (used in links and emails)",
        &public_default,
        valid_public_url,
    )?;
    let cur_proxies = a
        .env
        .get("PELTY_TRUSTED_PROXIES")
        .cloned()
        .unwrap_or_else(|| ctx.c("PELTY_TRUSTED_PROXIES", ""));
    let d = match cur_proxies.as_str() {
        "" => 0,
        LOCAL_PROXY => 1,
        CLOUDFLARE => 3,
        _ => 4,
    };
    p.say(&console::style("  Behind a proxy, the client IP (rate limits, audit log, sessions) and HTTPS come from its X-Forwarded-* headers, believed only from these addresses.").dim().to_string());
    let proxy = p.select("Reverse proxy in front of PeltyCMS?", &PROXIES, d)?;
    let proxies = match proxy {
        0 => String::new(),
        1 | 2 => LOCAL_PROXY.to_string(),
        3 => CLOUDFLARE.to_string(),
        _ => ask(
            p,
            "Proxy IPs or CIDR ranges (comma separated)",
            if d == 4 { &cur_proxies } else { "" },
            valid_proxies,
        )?,
    };
    if (proxy == 1 || proxy == 2) && bind.starts_with("0.0.0.0") {
        p.say(
            "  Tip: with the proxy on this machine, listen on 127.0.0.1 so nobody can bypass it.",
        );
    }
    let https = p.confirm(
        "Is the site reached over HTTPS? (enables Secure cookies and HSTS)",
        public.starts_with("https://") || proxy != 0,
    )?;
    if https && public.starts_with("http://") {
        p.say("  ! the public URL starts with http://; links in emails will not use HTTPS (change it with \"Edit: Web and reverse proxy\").");
    }
    a.env.insert("PELTY_BIND".into(), bind);
    a.env.insert("PELTY_PUBLIC_URL".into(), public);
    a.env
        .insert("PELTY_COOKIE_SECURE".into(), https.to_string());
    if proxy != 0 || !cur_proxies.is_empty() {
        a.env.insert("PELTY_TRUSTED_PROXIES".into(), proxies);
    }
    Ok(())
}

pub fn valid_proxies(v: &str) -> Result<String, String> {
    let items: Vec<String> = v
        .split([',', ' '])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if items.is_empty() {
        return Err("at least one IP or CIDR, e.g. 10.0.0.2 or 10.0.0.0/24".into());
    }
    for i in &items {
        let net = i
            .parse::<ipnet::IpNet>()
            .ok()
            .or_else(|| i.parse::<std::net::IpAddr>().ok().map(ipnet::IpNet::from));
        match net {
            Some(n) if n.prefix_len() > 0 => {}
            Some(_) => {
                return Err(format!(
                    "{i} would trust everyone; list the proxy addresses"
                ))
            }
            None => return Err(format!("{i} is not an IP address or CIDR range")),
        }
    }
    Ok(items.join(","))
}

fn company<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(
        p,
        3,
        "Stored in the database; editable later in Admin > General settings.",
    );
    if !p.confirm("Set them now? (No = skip this section)", true)? {
        a.plan.site = None;
        return Ok(());
    }
    let old = a.plan.site.clone().unwrap_or_default();
    let company_name = ask(p, "Company name", &old.company_name, |v| {
        let v = required(v)?;
        if v.len() > 200 {
            Err("max 200 characters".into())
        } else {
            Ok(v)
        }
    })?;
    let email = ask(
        p,
        "Company email (shown to clients, sender of notices)",
        &old.email,
        valid_email,
    )?;
    let cur_ccy = if old.currency.is_empty() {
        "EUR".to_string()
    } else {
        old.currency.clone()
    };
    // Fresh database: no currency table yet, offer the common ISO codes.
    let mut ccys: Vec<String> = if ctx.currencies.is_empty() {
        COMMON_CURRENCIES.iter().map(|c| c.to_string()).collect()
    } else {
        ctx.currencies.clone()
    };
    if !ccys.contains(&cur_ccy) {
        ccys.insert(0, cur_ccy.clone());
    }
    ccys.push("Other (type it)".into());
    let mut currency = pick(p, "Default currency (type to search)", &ccys, &cur_ccy)?;
    if currency.starts_with("Other") {
        currency = ask(p, "ISO 4217 code", &cur_ccy, valid_currency)?;
    }
    let langs = if ctx.languages.is_empty() {
        vec!["en".to_string()]
    } else {
        ctx.languages.clone()
    };
    let cur_lang = if old.language.is_empty() {
        ctx.c("PELTY_DEFAULT_LOCALE", "en")
    } else {
        old.language.clone()
    };
    let language = pick(p, "Default language", &langs, &cur_lang)?;
    let tz_cur = if old.timezone.is_empty() {
        std::env::var("TZ")
            .ok()
            .and_then(|t| valid_timezone(&t).ok())
            .unwrap_or_else(|| "UTC".into())
    } else {
        old.timezone.clone()
    };
    let mut zones: Vec<String> = TIMEZONES.iter().map(|s| s.to_string()).collect();
    if !zones.contains(&tz_cur) {
        zones.insert(0, tz_cur.clone());
    }
    zones.push("Other (type it)".into());
    let mut timezone = pick(p, "Time zone (type to search)", &zones, &tz_cur)?;
    if timezone.starts_with("Other") {
        timezone = ask(p, "IANA time zone", &tz_cur, valid_timezone)?;
    }
    a.env
        .insert("PELTY_DEFAULT_LOCALE".into(), language.clone());
    a.plan.site = Some(Site {
        company_name,
        email,
        currency,
        language,
        timezone,
    });
    Ok(())
}

fn admin<P: Prompt + ?Sized>(p: &mut P, _ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(
        p,
        4,
        "The first account with full access. More can be added later (console: admin create).",
    );
    if !p.confirm(
        "Create an administrator now? (No = skip this section)",
        true,
    )? {
        a.plan.admin = None;
        return Ok(());
    }
    let old = a.plan.admin.clone();
    let name = ask(
        p,
        "Name shown in the admin area",
        old.as_ref()
            .map(|o| o.name.as_str())
            .unwrap_or("Administrator"),
        |v| {
            let v = required(v)?;
            if v.chars().count() > 100 {
                Err("max 100 characters".into())
            } else {
                Ok(v)
            }
        },
    )?;
    let email_default = old
        .as_ref()
        .map(|o| o.email.clone())
        .or_else(|| a.plan.site.as_ref().map(|s| s.email.clone()))
        .unwrap_or_default();
    let email = ask(
        p,
        "Admin email (used to sign in)",
        &email_default,
        valid_email,
    )?;
    let admin = if p.confirm("Generate a strong password? (No = type one)", true)? {
        Admin {
            name,
            email,
            password: crate::console::generate_password(),
            generated: true,
        }
    } else {
        loop {
            let pw = p.secret(
                &format!("Password (min {MIN_PASSWORD} characters, mix letters, digits, symbols)"),
                true,
            )?;
            match pelty_auth::password::check_policy(&pw, &email) {
                Ok(()) => {
                    p.say(&format!("  Password strength: {}", strength(&pw)));
                    break Admin { name, email, password: pw, generated: false };
                }
                Err(e) => p.say(&format!("  ! password rejected by the policy: {e} (at least {MIN_PASSWORD} characters, mixed character types)")),
            }
        }
    };
    a.plan.admin = Some(admin);
    Ok(())
}

const SECURITY: [&str; 3] = ["starttls", "tls", "none"];

fn mail<P: Prompt + ?Sized, S: Services + ?Sized>(
    p: &mut P,
    svc: &mut S,
    a: &mut Answers,
) -> Result<(), Error> {
    header(
        p,
        5,
        "Optional. Without SMTP, emails are only written to the log.",
    );
    if !p.confirm("Configure SMTP now? (No = skip this section)", false)? {
        return Ok(());
    }
    let public = a.env.get("PELTY_PUBLIC_URL").cloned().unwrap_or_default();
    loop {
        let host = ask(p, "SMTP host", "", required)?;
        let port = ask(
            p,
            "Port (587 STARTTLS, 465 TLS, 25 none)",
            "587",
            valid_port,
        )?;
        let d = match port.as_str() {
            "465" => 1,
            "25" => 2,
            _ => 0,
        };
        let security = SECURITY[p.select("Connection security", &SECURITY, d)?];
        let user = ask(p, "Username (Enter = none)", "", any)?;
        let password = if user.is_empty() {
            String::new()
        } else {
            p.secret("Password", false)?
        };
        let domain = url::Url::parse(&public)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "localhost".into());
        let from = ask(
            p,
            "From address",
            &format!("PeltyCMS <no-reply@{domain}>"),
            |v| {
                if v.contains('@') {
                    Ok(v.trim().to_string())
                } else {
                    Err("an address, optionally with a name: Name <a@b.c>".into())
                }
            },
        )?;
        let surl = smtp_url(&host, &port, security, &user, &password);
        let mut keep = true;
        if p.confirm("Send a test email now?", false)? {
            let to = ask(
                p,
                "Send it to",
                &a.plan
                    .admin
                    .as_ref()
                    .map(|x| x.email.clone())
                    .unwrap_or_default(),
                valid_email,
            )?;
            match svc.send_test_mail(&surl, &from, &to) {
                Ok(()) => p.say(
                    &console::style("  Sent. Check the inbox.")
                        .green()
                        .to_string(),
                ),
                Err(e) => {
                    p.say(&format!("  ! sending failed: {e}"));
                    keep = !p.confirm("Change the SMTP settings?", true)?;
                }
            }
        }
        if keep {
            a.env.insert("PELTY_SMTP_URL".into(), surl);
            a.env.insert("PELTY_MAIL_FROM".into(), from);
            return Ok(());
        }
    }
}

fn captcha<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(
        p,
        6,
        "Protects sign-in, registration, password reset and contact forms for guests.",
    );
    if ctx.captcha.is_empty() {
        p.say("  No captcha providers available on this install; skipped.");
        return Ok(());
    }
    let mut items = vec!["None (no captcha)".to_string()];
    items.extend(ctx.captcha.iter().map(|c| c.label.clone()));
    let cur = a
        .plan
        .captcha
        .as_ref()
        .and_then(|c| ctx.captcha.iter().position(|x| x.id == c.provider))
        .map_or(0, |i| i + 1);
    let refs: Vec<&str> = items.iter().map(String::as_str).collect();
    let i = p.select("Captcha provider", &refs, cur)?;
    if i == 0 {
        a.plan.captcha = None;
        return Ok(());
    }
    let prov = &ctx.captcha[i - 1];
    let (site_key, secret_key) = if prov.needs_keys {
        p.say(
            &console::style(format!(
                "  Create a site in the {} dashboard and paste its keys.",
                prov.label
            ))
            .dim()
            .to_string(),
        );
        let site = ask(p, "Site key", "", required)?;
        let secret = loop {
            let s = p.secret("Secret key", false)?;
            if !s.trim().is_empty() {
                break s.trim().to_string();
            }
            p.say("  ! required");
        };
        (site, secret)
    } else {
        (String::new(), String::new())
    };
    a.plan.captcha = Some(Captcha {
        provider: prov.id.clone(),
        label: prov.label.clone(),
        site_key,
        secret_key,
    });
    Ok(())
}

const LOG_LEVELS: [&str; 5] = [
    "info,sqlx=warn",
    "debug,sqlx=warn",
    "warn",
    "error",
    "trace",
];

fn num(min: u64, max: u64) -> impl Fn(&str) -> Result<String, String> {
    move |v| match v.trim().parse::<u64>() {
        Ok(n) if (min..=max).contains(&n) => Ok(n.to_string()),
        _ => Err(format!("a whole number from {min} to {max}")),
    }
}

/// Pick gateways from the bundled packages; secrets are typed hidden and
/// saved in `.env` as `PELTY_GW_<GATEWAY>_<SECRET>`, referenced by name from
/// the gateway settings (the same mechanism Admin > Gateways uses).
fn gateways<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    header(
        p,
        7,
        "Card, PayPal, bank and other payment providers clients can pay invoices with.",
    );
    if ctx.gateways.is_empty() {
        p.say("  No gateway addons available on this install; upload one later in Admin > Addons, then enable it in Admin > Gateways.");
        return Ok(());
    }
    if !p.confirm(
        "Enable payment gateways now? (No = skip; Admin > Gateways later)",
        !a.plan.gateways.is_empty(),
    )? {
        a.plan.gateways.clear();
        return Ok(());
    }
    let labels: Vec<&str> = ctx.gateways.iter().map(|g| g.label.as_str()).collect();
    let pre: Vec<bool> = ctx
        .gateways
        .iter()
        .map(|g| a.plan.gateways.iter().any(|x| x.addon == g.addon))
        .collect();
    let picked = p.multi_select(
        "Gateways to enable (space toggles, Enter confirms)",
        &labels,
        &pre,
    )?;
    a.plan.gateways.clear();
    for i in picked {
        let g = &ctx.gateways[i];
        p.say(&format!("  {}", console::style(&g.label).bold()));
        let test_mode = p.confirm("  Use test / sandbox mode?", true)?;
        let mut refs = vec![];
        for s in &g.secrets {
            let env = gateway_env(&g.provides, s);
            let have = a
                .env
                .get(&env)
                .cloned()
                .or_else(|| (ctx.cur)(&env))
                .filter(|v| !v.is_empty());
            let v = p.secret(
                &format!(
                    "  {} {}{}",
                    g.label,
                    s.replace('_', " "),
                    if have.is_some() {
                        " (Enter keeps the current one)"
                    } else {
                        " (Enter = add later)"
                    }
                ),
                false,
            )?;
            let v = if v.trim().is_empty() {
                have.unwrap_or_default()
            } else {
                v.trim().to_string()
            };
            if !v.is_empty() {
                a.env.insert(env.clone(), v);
            }
            refs.push((s.clone(), env));
        }
        a.plan.gateways.push(GatewayPlan {
            addon: g.addon.clone(),
            label: g.label.clone(),
            secret_refs: refs,
            test_mode,
        });
    }
    Ok(())
}

fn advanced<P: Prompt + ?Sized, S: Services + ?Sized>(
    p: &mut P,
    svc: &mut S,
    ctx: &Ctx,
    a: &mut Answers,
) -> Result<(), Error> {
    header(p, 8, "Logging, sessions, limits, uploads, background jobs and Redis. The defaults suit most installs.");
    if !p.confirm("Change advanced settings? (No = keep the defaults)", false)? {
        return Ok(());
    }
    let g = |a: &Answers, k: &str, d: &str| a.env.get(k).cloned().unwrap_or_else(|| ctx.c(k, d));
    let levels: Vec<String> = LOG_LEVELS.iter().map(|s| s.to_string()).collect();
    let rust_log = pick(p, "Log level", &levels, &g(a, "RUST_LOG", "info,sqlx=warn"))?;
    let json = p.confirm(
        "Log as JSON (for log collectors)?",
        g(a, "PELTY_LOG_JSON", "false") == "true",
    )?;
    let idle = ask(
        p,
        "Sign out after inactivity (minutes)",
        &g(a, "PELTY_SESSION_IDLE_MINUTES", "1440"),
        num(5, 525_600),
    )?;
    let absolute = ask(
        p,
        "Maximum session length (hours)",
        &g(a, "PELTY_SESSION_ABSOLUTE_HOURS", "720"),
        num(1, 8760),
    )?;
    let rate = ask(
        p,
        "Sign-in attempts per IP per minute",
        &g(a, "PELTY_AUTH_RATE_PER_MINUTE", "10"),
        num(1, 10_000),
    )?;
    let pool = ask(
        p,
        "Database connections (pool size)",
        &g(a, "PELTY_DB_MAX_CONNECTIONS", "10"),
        num(1, 1000),
    )?;
    let backups = ask(
        p,
        "Backup folder",
        &g(a, "PELTY_BACKUP_DIR", "backups"),
        required,
    )?;
    let docs = p.confirm(
        "Make the docs at /docs public (default: staff only)?",
        g(a, "PELTY_DOCS_PUBLIC", "false") == "true",
    )?;
    for (k, v) in [
        ("RUST_LOG", rust_log),
        ("PELTY_LOG_JSON", json.to_string()),
        ("PELTY_SESSION_IDLE_MINUTES", idle),
        ("PELTY_SESSION_ABSOLUTE_HOURS", absolute),
        ("PELTY_AUTH_RATE_PER_MINUTE", rate),
        ("PELTY_DB_MAX_CONNECTIONS", pool),
        ("PELTY_BACKUP_DIR", backups),
        ("PELTY_DOCS_PUBLIC", docs.to_string()),
    ] {
        a.env.insert(k.into(), v);
    }
    uploads(p, ctx, a)?;
    jobs(p, ctx, a)?;
    redis(p, svc, ctx, a)?;
    ai::ask(p, ctx, a)?;
    Ok(())
}

/// Upload caps (`PELTY_UPLOAD_*_MB`), MB per file.
fn uploads<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    p.say("  Uploads: attachments 50 MB, avatars/logos 2, ID documents 20, downloads 25, CSV imports 5, addon packages 64 (MB).");
    if !p.confirm("Change upload limits?", false)? {
        return Ok(());
    }
    let g = |a: &Answers, k: &str, d: &str| a.env.get(k).cloned().unwrap_or_else(|| ctx.c(k, d));
    for (k, q, d, max) in [
        (
            "PELTY_UPLOAD_ATTACHMENT_MB",
            "Ticket attachments (MB, max 50)",
            "50",
            50,
        ),
        (
            "PELTY_UPLOAD_AVATAR_MB",
            "Avatars and logos (MB)",
            "2",
            4096,
        ),
        ("PELTY_UPLOAD_KYC_MB", "Identity documents (MB)", "20", 4096),
        (
            "PELTY_UPLOAD_DOWNLOAD_MB",
            "Client download files (MB)",
            "25",
            4096,
        ),
        ("PELTY_UPLOAD_IMPORT_MB", "CSV imports (MB)", "5", 4096),
        (
            "PELTY_UPLOAD_ADDON_MB",
            "Addon packages (MB, max 64)",
            "64",
            64,
        ),
    ] {
        let v = ask(p, q, &g(a, k, d), num(1, max))?;
        a.env.insert(k.into(), v);
    }
    p.say("  A reverse proxy has its own limit too (nginx: client_max_body_size).");
    Ok(())
}

/// Built-in scheduler on/off, worker batch size and the daily billing hour.
fn jobs<P: Prompt + ?Sized>(p: &mut P, ctx: &Ctx, a: &mut Answers) -> Result<(), Error> {
    let g = |a: &Answers, k: &str, d: &str| a.env.get(k).cloned().unwrap_or_else(|| ctx.c(k, d));
    let on = p.confirm("Run the built-in scheduler here? (billing, overdue notices, reminders, renewals; No = use `pelty-server cron run` from cron)", g(a, "PELTY_SCHEDULER", "true") != "false")?;
    a.env.insert("PELTY_SCHEDULER".into(), on.to_string());
    if !on {
        p.say("  Add to crontab:  * * * * *  cd <this folder> && pelty-server cron run");
    }
    let every = |secs: i64| {
        if secs % 86400 == 0 {
            "daily".to_string()
        } else if secs % 3600 == 0 {
            format!("every {}h", secs / 3600)
        } else {
            format!("every {}m", secs / 60)
        }
    };
    let list: Vec<String> = pelty_api::routes::automation::TASKS
        .iter()
        .map(|(k, secs)| format!("{k} ({})", every(*secs)))
        .collect();
    p.say(&format!("  Scheduled tasks: {}.", list.join(", ")));
    p.say("  Addon jobs show up as <addon-id>:<op>. Manage them in Admin > Automation or the console (`cron list`, `cron run <task>`, `cron disable <task>`).");
    let n = ask(
        p,
        "Jobs per worker batch",
        &g(a, "PELTY_WORKER_CONCURRENCY", "10"),
        num(1, 200),
    )?;
    a.env.insert("PELTY_WORKER_CONCURRENCY".into(), n);
    let cur = a
        .plan
        .billing_hour
        .map(|h| h.to_string())
        .unwrap_or_default();
    let h = ask(
        p,
        "Daily billing run hour 0-23, company time zone (empty = right after midnight)",
        &cur,
        |v: &str| {
            if v.trim().is_empty() {
                return Ok(String::new());
            }
            num(0, 23)(v)
        },
    )?;
    a.plan.billing_hour = h.parse().ok();
    Ok(())
}

/// Optional Redis with a live test.
fn redis<P: Prompt + ?Sized, S: Services + ?Sized>(
    p: &mut P,
    svc: &mut S,
    ctx: &Ctx,
    a: &mut Answers,
) -> Result<(), Error> {
    let cur = a
        .env
        .get("PELTY_REDIS_URL")
        .cloned()
        .unwrap_or_else(|| ctx.c("PELTY_REDIS_URL", ""));
    p.say("  Redis is optional: with several instances it shares rate limits, lets one of them run the scheduler and reloads admin changes on all of them.");
    if !p.confirm("Use Redis?", !cur.is_empty())? {
        if !cur.is_empty() {
            a.env.insert("PELTY_REDIS_URL".into(), String::new());
        }
        return Ok(());
    }
    loop {
        let url = ask(
            p,
            "Redis URL",
            if cur.is_empty() {
                "redis://127.0.0.1:6379/0"
            } else {
                &cur
            },
            |v: &str| {
                let v = v.trim();
                if ["redis://", "rediss://", "unix://", "redis+unix://"]
                    .iter()
                    .any(|s| v.starts_with(s))
                {
                    Ok(v.to_string())
                } else {
                    Err("use redis://host:6379/0".into())
                }
            },
        )?;
        p.say(&format!("  Testing {} ...", mask_url(&url)));
        match svc.redis_ping(&url) {
            Ok(ms) => {
                p.say(&format!("  Redis OK ({ms} ms)."));
                a.env.insert("PELTY_REDIS_URL".into(), url);
                return Ok(());
            }
            Err(e) => {
                p.say(&format!("  ! Redis test failed: {e}"));
                if !p.confirm("Try another URL? (No = keep this one anyway)", true)? {
                    a.env.insert("PELTY_REDIS_URL".into(), url);
                    return Ok(());
                }
            }
        }
    }
}

/// Sectioned summary; secrets are masked.
pub fn summary(a: &Answers) -> String {
    let e = |k: &str| a.env.get(k).map(String::as_str);
    let mut s = format!("\n{}\n", console::style("Summary").bold().underlined());
    let mut sec = |title: &str, rows: Vec<(&str, String)>| {
        s.push_str(&format!("  {}\n", console::style(title).cyan()));
        for (k, v) in rows {
            s.push_str(&format!("    {k:<22} {v}\n"));
        }
    };
    sec("License", vec![("EULA", "accepted".into())]);
    let db = e("DATABASE_URL").unwrap_or_default();
    let pw = if url::Url::parse(db)
        .ok()
        .and_then(|u| u.password().map(str::to_string))
        .is_some()
    {
        "****"
    } else {
        "(none)"
    };
    sec(
        "Database",
        vec![("URL", mask_url(db)), ("password", pw.into())],
    );
    let proxies = match e("PELTY_TRUSTED_PROXIES").unwrap_or("") {
        "" => "none (direct)".to_string(),
        LOCAL_PROXY => "this machine (127.0.0.1, ::1)".into(),
        CLOUDFLARE => "Cloudflare ranges".into(),
        x => x.to_string(),
    };
    sec(
        "Web",
        vec![
            ("listen", e("PELTY_BIND").unwrap_or_default().into()),
            (
                "public URL",
                e("PELTY_PUBLIC_URL").unwrap_or_default().into(),
            ),
            ("trusted proxies", proxies),
            ("HTTPS", e("PELTY_COOKIE_SECURE").unwrap_or("false").into()),
        ],
    );
    sec(
        "Company",
        match &a.plan.site {
            Some(x) => vec![
                ("name", x.company_name.clone()),
                ("email", x.email.clone()),
                ("currency", x.currency.clone()),
                ("language", x.language.clone()),
                ("time zone", x.timezone.clone()),
            ],
            None => vec![("", "(skipped, unchanged)".into())],
        },
    );
    sec(
        "Administrator",
        match &a.plan.admin {
            Some(x) => vec![
                ("name", x.name.clone()),
                ("email", x.email.clone()),
                (
                    "password",
                    if x.generated {
                        "generated, shown once after setup".into()
                    } else {
                        "entered (hidden)".into()
                    },
                ),
            ],
            None => vec![("", "(skipped, none created)".into())],
        },
    );
    sec(
        "Email",
        match e("PELTY_SMTP_URL") {
            Some(u) => vec![
                ("SMTP", mask_url(u)),
                ("from", e("PELTY_MAIL_FROM").unwrap_or_default().into()),
            ],
            None => vec![("", "(skipped, mail is logged only)".into())],
        },
    );
    sec(
        "Captcha",
        match &a.plan.captcha {
            Some(c) if c.site_key.is_empty() => vec![("provider", c.label.clone())],
            Some(c) => vec![
                ("provider", c.label.clone()),
                ("site key", c.site_key.clone()),
                ("secret key", "****".into()),
            ],
            None => vec![("", "(none)".into())],
        },
    );
    let gws: Vec<(&str, String)> = a
        .plan
        .gateways
        .iter()
        .map(|g| {
            let keys = g
                .secret_refs
                .iter()
                .filter(|(_, env)| a.env.get(env).is_some_and(|v| !v.is_empty()))
                .count();
            (
                g.label.as_str(),
                format!(
                    "{}, {}/{} keys (hidden)",
                    if g.test_mode { "test mode" } else { "live" },
                    keys,
                    g.secret_refs.len()
                ),
            )
        })
        .collect();
    sec(
        "Payment gateways",
        if gws.is_empty() {
            vec![("", "(none now)".into())]
        } else {
            gws
        },
    );
    let adv: Vec<(&str, String)> = [
        ("log level", "RUST_LOG"),
        ("JSON logs", "PELTY_LOG_JSON"),
        ("idle minutes", "PELTY_SESSION_IDLE_MINUTES"),
        ("session hours", "PELTY_SESSION_ABSOLUTE_HOURS"),
        ("sign-ins/min", "PELTY_AUTH_RATE_PER_MINUTE"),
        ("DB pool", "PELTY_DB_MAX_CONNECTIONS"),
        ("backups", "PELTY_BACKUP_DIR"),
        ("public docs", "PELTY_DOCS_PUBLIC"),
        ("uploads: attachments", "PELTY_UPLOAD_ATTACHMENT_MB"),
        ("uploads: avatars", "PELTY_UPLOAD_AVATAR_MB"),
        ("uploads: ID docs", "PELTY_UPLOAD_KYC_MB"),
        ("uploads: downloads", "PELTY_UPLOAD_DOWNLOAD_MB"),
        ("uploads: imports", "PELTY_UPLOAD_IMPORT_MB"),
        ("uploads: addons", "PELTY_UPLOAD_ADDON_MB"),
        ("scheduler", "PELTY_SCHEDULER"),
        ("jobs per batch", "PELTY_WORKER_CONCURRENCY"),
    ]
    .iter()
    .filter_map(|(l, k)| e(k).map(|v| (*l, v.to_string())))
    .chain(
        a.plan
            .billing_hour
            .map(|h| ("billing run", format!("{h:02}:00 company time"))),
    )
    .chain(e("PELTY_REDIS_URL").map(|v| {
        (
            "redis",
            if v.is_empty() {
                "off".to_string()
            } else {
                mask_url(v)
            },
        )
    }))
    .chain(e("PELTY_AI_MODEL").map(|v| {
        (
            "AI ticket drafts",
            if v.is_empty() {
                "off".to_string()
            } else {
                format!("on ({v}); API key hidden")
            },
        )
    }))
    .collect();
    sec(
        "Advanced",
        if adv.is_empty() {
            vec![("", "(defaults)".into())]
        } else {
            adv
        },
    );
    s.push_str(
        &console::style("  The encryption key in .env is kept as is.\n")
            .dim()
            .to_string(),
    );
    s
}

/// One-click admin URL printed after setup (`GET /api/v1/auth/setup-login`).
pub fn setup_login_url(public_url: &str, token: &str) -> String {
    format!(
        "{}/api/v1/auth/setup-login?token={token}",
        public_url.trim_end_matches('/')
    )
}

/// Boxed "what now" text printed after setup.
pub fn next_steps(
    public_url: &str,
    admin_email: Option<&str>,
    login_token: Option<&str>,
    root: &Path,
) -> String {
    let mut rows = vec![
        "Next steps".to_string(),
        format!("Open        {public_url}"),
    ];
    match (admin_email, login_token) {
        (Some(e), Some(tok)) => {
            rows.push(format!("Sign in as  {e}"));
            rows.push(format!("Quick login {}", setup_login_url(public_url, tok)));
            rows.push("            one-time link · expires in 1 hour".into());
        }
        (Some(e), None) => rows.push(format!("Sign in as  {e}")),
        (None, _) => {
            rows.push("Admin       console: admin create <email> --name <name>".into())
        }
    }
    rows.push("Console     type `help` while the server runs (stop, status, admin ...)".into());
    rows.push(format!(
        "Config      {} and eula.txt",
        root.join(bootstrap::ENV_FILE).display()
    ));
    rows.push("Change it   pelty-server setup   (or edit .env and restart)".into());
    let w = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
    let line = |l: char, r: char| format!("{l}{}{r}", "─".repeat(w + 2));
    let mut s = format!("\n{}\n", line('┌', '┐'));
    for (i, r) in rows.iter().enumerate() {
        let pad = " ".repeat(w - r.chars().count());
        let r = if i == 0 {
            console::style(r).bold().to_string()
        } else {
            r.clone()
        };
        s.push_str(&format!("│ {r}{pad} │\n"));
        if i == 0 {
            s.push_str(&format!("{}\n", line('├', '┤')));
        }
    }
    s.push_str(&line('└', '┘'));
    s
}

/// Ask whether to start the server now (after `pelty-server setup`).
pub fn ask_start<P: Prompt + ?Sized>(p: &mut P) -> io::Result<bool> {
    p.confirm("Start the server now?", true)
}

// headless

/// `--yes`: accept every default, overridden by flags or the environment.
#[derive(Debug, Default, Clone)]
pub struct Headless {
    pub db_url: Option<String>,
    pub bind: Option<String>,
    pub public_url: Option<String>,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub admin_name: Option<String>,
    pub accept_eula: bool,
}

impl Headless {
    /// Take `--db-url X` / `--db-url=X` style flags out of `args`.
    pub fn take_flags(args: &mut Vec<String>) -> Result<Headless, String> {
        let mut h = Headless::default();
        let mut out = Vec::new();
        let mut it = std::mem::take(args).into_iter();
        while let Some(a) = it.next() {
            if a == "--accept-eula" {
                h.accept_eula = true;
                continue;
            }
            let (k, inline) = match a.split_once('=') {
                Some((k, v)) => (k.to_string(), Some(v.to_string())),
                None => (a.clone(), None),
            };
            let slot = match k.as_str() {
                "--db-url" => &mut h.db_url,
                "--bind" => &mut h.bind,
                "--public-url" => &mut h.public_url,
                "--admin-email" => &mut h.admin_email,
                "--admin-password" => &mut h.admin_password,
                "--admin-name" => &mut h.admin_name,
                _ => {
                    out.push(a);
                    continue;
                }
            };
            *slot = Some(match inline.or_else(|| it.next()) {
                Some(v) => v,
                None => return Err(format!("{k} needs a value")),
            });
        }
        *args = out;
        Ok(h)
    }
}

/// Non-interactive setup. Errors explain which flag or variable to set.
pub fn headless<S: Services + ?Sized>(
    svc: &mut S,
    cur: &dyn Fn(&str) -> Option<String>,
    eula_accepted: bool,
    h: &Headless,
) -> Result<Answers, String> {
    let c = |flag: &Option<String>, k: &str, d: &str| {
        flag.clone()
            .or_else(|| cur(k).filter(|v| !v.trim().is_empty()))
            .unwrap_or_else(|| d.to_string())
    };
    let mut a = Answers::default();
    if !(eula_accepted || h.accept_eula) {
        return Err("the EULA is not accepted: add --accept-eula (or PELTY_EULA=true, or eula=true in eula.txt)".into());
    }
    a.eula = true;
    let url = valid_pg_url(&c(
        &h.db_url,
        "DATABASE_URL",
        "postgres://postgres@127.0.0.1:5432/peltycms",
    ))
    .map_err(|e| format!("--db-url: {e}"))?;
    let mut res = svc.check_db(&url);
    if res == DbCheck::MissingDatabase {
        svc.create_db(&url)
            .map_err(|e| format!("cannot create the database: {e}"))?;
        println!(
            "[PeltyCMS] setup: created database \"{}\"",
            pg_parts(&url).db
        );
        res = svc.check_db(&url);
    }
    if let DbCheck::Failed(e) = res {
        return Err(format!("database {}: {e}", mask_url(&url)));
    }
    let bind = valid_bind(&c(&h.bind, "PELTY_BIND", "127.0.0.1:8080"))
        .map_err(|e| format!("--bind: {e}"))?;
    let local = |b: &str| {
        format!(
            "http://localhost:{}",
            b.rsplit(':').next().unwrap_or("8080")
        )
    };
    let old_bind = cur("PELTY_BIND").unwrap_or_else(|| "127.0.0.1:8080".into());
    let public = match h.public_url.clone().or_else(|| {
        cur("PELTY_PUBLIC_URL")
            .filter(|u| !u.trim().is_empty() && u.trim_end_matches('/') != local(&old_bind))
    }) {
        Some(u) => u,
        None => local(&bind),
    };
    let public = valid_public_url(&public).map_err(|e| format!("--public-url: {e}"))?;
    a.env.insert("DATABASE_URL".into(), url);
    a.env.insert("PELTY_BIND".into(), bind);
    a.env.insert(
        "PELTY_COOKIE_SECURE".into(),
        public.starts_with("https://").to_string(),
    );
    a.env.insert("PELTY_PUBLIC_URL".into(), public);
    // `setup --yes` only: PELTY_ADMIN_EMAIL (with PELTY_ADMIN_PASSWORD, else a
    // generated password) creates the first administrator when no flag is given.
    if let Some(email) = h
        .admin_email
        .clone()
        .or_else(|| {
            // First administrator email for `setup --yes` when `--admin-email` is omitted.
            cur("PELTY_ADMIN_EMAIL")
        })
        .filter(|e| !e.trim().is_empty())
    {
        let email = valid_email(&email).map_err(|e| format!("--admin-email: {e}"))?;
        let typed = h
            .admin_password
            .clone()
            .or_else(|| cur("PELTY_ADMIN_PASSWORD"))
            .filter(|p| !p.is_empty());
        let generated = typed.is_none();
        let password = typed.unwrap_or_else(crate::console::generate_password);
        pelty_auth::password::check_policy(&password, &email)
            .map_err(|e| format!("--admin-password: {e}"))?;
        let name = h
            .admin_name
            .clone()
            .unwrap_or_else(|| "Administrator".into());
        a.plan.admin = Some(Admin {
            name,
            email,
            password,
            generated,
        });
    }
    a.env.insert(MARKER.into(), "done".into());
    Ok(a)
}

/// Language codes with a `<code>.json` file in `dir` (sorted), else `en`.
pub fn languages(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let code = n.strip_suffix(".json")?;
            (code.len() <= 8 && code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
                .then(|| code.to_string())
        })
        .collect();
    v.sort();
    if v.is_empty() {
        v.push("en".into());
    }
    v
}

// after migrate

/// Apply the database part of the plan (after migrations). Failures are
/// reported and do not stop the server. Returns a one-time setup-login token
/// when an administrator was created.
pub async fn apply(pool: &pelty_database::Pool, plan: &Plan) -> Option<String> {
    // Wizard / headless already required EULA; mirror that into admin license status.
    match pelty_database::license::accept_from_setup(pool).await {
        Ok(()) => println!("[PeltyCMS] setup: License and EULA recorded for this deployment"),
        Err(e) => println!("[PeltyCMS] setup warning: license acceptance not saved: {e}"),
    }
    if let Some(site) = &plan.site {
        match apply_site(pool, site).await {
            Ok(()) => println!("[PeltyCMS] setup: company settings saved"),
            Err(e) => println!("[PeltyCMS] setup warning: company settings not saved: {e}"),
        }
    }
    let mut login_token = None;
    if let Some(admin) = &plan.admin {
        let auth = pelty_auth::AuthService {
            pool: pool.clone(),
            idle: time::Duration::minutes(1),
            absolute: time::Duration::minutes(1),
        };
        match crate::create_admin(&auth, &admin.email, &admin.password, &admin.name).await {
            Ok(u) => {
                println!("[PeltyCMS] setup: administrator {} created", u.email);
                if admin.generated {
                    println!("[PeltyCMS]   password: {}   <- shown only now; store it in a password manager", admin.password);
                }
                let tok = pelty_auth::token::generate();
                match pelty_database::user_tokens::issue(
                    pool,
                    u.id,
                    pelty_database::user_tokens::SETUP_LOGIN,
                    &pelty_auth::token::hash(&tok),
                    time::Duration::hours(1),
                )
                .await
                {
                    Ok(()) => login_token = Some(tok),
                    Err(e) => println!("[PeltyCMS] setup warning: quick-login link not created: {e}"),
                }
            }
            Err(e) => println!("[PeltyCMS] setup warning: {e} (use `admin create` or `admin reset-password` in the console)"),
        }
    }
    if let Some(h) = plan.billing_hour {
        match pelty_database::automation_settings::upsert(pool, pelty_api::worker::RECURRING, true, Some(h)).await {
            Ok(()) => println!("[PeltyCMS] setup: daily billing runs from {h:02}:00 company time (Admin > Automation)"),
            Err(e) => println!("[PeltyCMS] setup warning: billing hour not saved: {e}"),
        }
    }
    login_token
}

async fn apply_site(pool: &pelty_database::Pool, s: &Site) -> Result<(), String> {
    use pelty_database::general_settings as gs;
    let mut g = gs::load(pool).await.map_err(|e| e.to_string())?;
    g.general.company_name = s.company_name.clone();
    g.general.email = s.email.clone();
    g.localisation.default_language = s.language.clone();
    g.localisation.timezone = s.timezone.clone();
    gs::validate(&g).map_err(|e| e.to_string())?;
    gs::save(pool, &g).await.map_err(|e| e.to_string())?;
    let cur: Option<(bool,)> = sqlx::query_as("SELECT is_default FROM currencies WHERE code = $1")
        .bind(&s.currency)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    match cur {
        None => return Err(format!("unknown currency {}", s.currency)),
        Some((true,)) => {}
        Some((false,)) => {
            sqlx::query("UPDATE currencies SET enabled = TRUE WHERE code = $1")
                .bind(&s.currency)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            pelty_database::currencies::set_default(pool, &s.currency)
                .await
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
