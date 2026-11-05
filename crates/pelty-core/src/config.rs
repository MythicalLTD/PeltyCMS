//! Environment-based configuration, validated at startup.
use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    /// Listen address.
    pub bind: SocketAddr,
    /// PostgreSQL connection URL.
    pub database_url: String,
    /// Database pool size.
    pub db_max_connections: u32,
    /// Set the `Secure` flag on cookies. Must be true behind HTTPS.
    pub cookie_secure: bool,
    /// Sign out after this many idle minutes.
    pub session_idle_minutes: i64,
    /// Sessions end after this many hours regardless of activity.
    pub session_absolute_hours: i64,
    /// Built frontend served as the SPA.
    pub frontend_dir: PathBuf,
    /// Theme folders (`theme.json`).
    pub themes_dir: PathBuf,
    /// Webhook plugin manifests.
    pub plugins_dir: PathBuf,
    /// Translation JSON files.
    pub locales_dir: PathBuf,
    /// Language for visitors without a preference.
    pub default_locale: String,
    /// Log as JSON lines instead of text.
    pub log_json: bool,
    /// Max auth attempts per IP per minute.
    pub auth_rate_per_minute: u32,
    /// SMTP URL (`smtps://user:pass@host:465`). Unset: mail is logged, not sent.
    pub smtp_url: Option<Redacted>,
    /// From address for outgoing mail.
    pub mail_from: String,
    /// Absolute base URL used in email links, e.g. `https://billing.example.com`.
    pub public_url: String,
    /// Base64 32-byte key for secrets at rest (TOTP). No default: 2FA
    /// enrolment is refused while unset.
    pub encryption_key: Option<Redacted>,
    /// Reverse proxies (IPs / CIDRs) whose `X-Forwarded-For`, `X-Real-IP`
    /// and `X-Forwarded-Proto` are believed, in addition to the list stored
    /// under Admin > Security > Trusted proxies.
    pub trusted_proxies: Vec<String>,
    /// Run the built-in scheduler (enqueues recurring billing, overdue,
    /// reminders, backups, ...). With several instances, keep it on in one
    /// only, or set `PELTY_REDIS_URL` so they elect a leader. Off: run
    /// `pelty-server cron run` from system cron instead.
    pub scheduler: bool,
    /// Seconds between scheduler passes.
    pub scheduler_interval_secs: u64,
    /// Jobs a worker claims per batch.
    pub worker_concurrency: i64,
    /// Optional Redis (`redis://host:6379/0`): shared rate limits, cache
    /// invalidation and events across instances, scheduler leader lock.
    pub redis_url: Option<Redacted>,
    /// Upload size caps (bytes), from `PELTY_UPLOAD_*_MB`.
    pub uploads: UploadLimits,
    /// Optional, staff-only ticket draft provider. Never sends replies.
    pub ai: AiConfig,
    /// Operator-owned JSON map of addon signing key ids to public keys.
    pub addon_trusted_keys: Option<PathBuf>,
    /// Refuse unsigned disk addons (built-in host implementations are trusted code).
    pub addon_require_signatures: bool,
    /// Optional MaxMind-compatible MMDB (DB-IP Lite or GeoLite2). When unset,
    /// `data/geoip/dbip-city-lite.mmdb` is used if present.
    pub geoip_db: Option<PathBuf>,
}

/// OpenAI-compatible provider. All three values are required to enable it.
#[derive(Debug, Clone, Default)]
pub struct AiConfig {
    /// `PELTY_AI_BASE_URL`: OpenAI-compatible API base URL including `/v1`.
    pub base_url: Option<String>,
    /// `PELTY_AI_API_KEY`: provider API key (redacted in logs).
    pub api_key: Option<Redacted>,
    /// `PELTY_AI_MODEL`: model name, e.g. `gpt-4o-mini`.
    pub model: Option<String>,
}

impl AiConfig {
    pub fn enabled(&self) -> bool {
        self.base_url.as_ref().is_some_and(|v| !v.trim().is_empty())
            && self
                .api_key
                .as_ref()
                .is_some_and(|v| !v.0.trim().is_empty())
            && self.model.as_ref().is_some_and(|v| !v.trim().is_empty())
    }
}

/// Upload size caps in bytes. Defaults are the historical built-in limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UploadLimits {
    pub attachment: usize,
    pub avatar: usize,
    pub kyc: usize,
    pub download: usize,
    pub import: usize,
    pub addon: usize,
}

const MB: usize = 1024 * 1024;

impl Default for UploadLimits {
    fn default() -> Self {
        UploadLimits {
            attachment: 50 * MB,
            avatar: 2 * MB,
            kyc: 20 * MB,
            download: 25 * MB,
            import: 5 * MB,
            addon: 64 * MB,
        }
    }
}

impl UploadLimits {
    pub fn from_env() -> Result<Self, ConfigError> {
        let d = UploadLimits::default();
        let mb = |k: &'static str, def: usize| -> Result<usize, ConfigError> {
            let v: usize = parse(k, def / MB)?;
            if v == 0 || v > 4096 {
                return Err(ConfigError::Invalid(k, format!("{v} (1..=4096 MB)")));
            }
            Ok(v * MB)
        };
        Ok(UploadLimits {
            // Ticket attachment cap; the per-file limit in Admin > General settings stays below it.
            attachment: mb("PELTY_UPLOAD_ATTACHMENT_MB", d.attachment)?,
            // Profile pictures and logos.
            avatar: mb("PELTY_UPLOAD_AVATAR_MB", d.avatar)?,
            // Identity (KYC) documents.
            kyc: mb("PELTY_UPLOAD_KYC_MB", d.kyc)?,
            // Files offered in the client downloads area.
            download: mb("PELTY_UPLOAD_DOWNLOAD_MB", d.download)?,
            // CSV imports (clients, products, ...).
            import: mb("PELTY_UPLOAD_IMPORT_MB", d.import)?,
            // Addon packages; never above the 64 MB package hard cap.
            addon: mb("PELTY_UPLOAD_ADDON_MB", d.addon)?.min(d.addon),
        })
    }
}

/// Process-wide upload caps (read once from the environment; defaults when
/// unset or invalid, which `Config::from_env` reports at startup).
pub fn upload_limits() -> &'static UploadLimits {
    static L: std::sync::OnceLock<UploadLimits> = std::sync::OnceLock::new();
    L.get_or_init(|| UploadLimits::from_env().unwrap_or_default())
}

/// A secret config value; never shown by `Debug`.
#[derive(Clone)]
pub struct Redacted(pub String);
impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required env var {0}")]
    Missing(&'static str),
    #[error("invalid value for {0}: {1}")]
    Invalid(&'static str, String),
}

fn var(k: &'static str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.trim().is_empty())
}
fn parse<T: std::str::FromStr>(k: &'static str, default: T) -> Result<T, ConfigError> {
    match var(k) {
        None => Ok(default),
        Some(v) => v.parse().map_err(|_| ConfigError::Invalid(k, v)),
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = var("DATABASE_URL").ok_or(ConfigError::Missing("DATABASE_URL"))?;
        if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
            return Err(ConfigError::Invalid(
                "DATABASE_URL",
                "must be a postgres:// URL".into(),
            ));
        }
        let c = Config {
            bind: parse("PELTY_BIND", "0.0.0.0:8080".parse().expect("valid default"))?,
            database_url,
            db_max_connections: parse("PELTY_DB_MAX_CONNECTIONS", 10)?,
            cookie_secure: parse("PELTY_COOKIE_SECURE", true)?,
            session_idle_minutes: parse("PELTY_SESSION_IDLE_MINUTES", 60 * 24)?,
            session_absolute_hours: parse("PELTY_SESSION_ABSOLUTE_HOURS", 24 * 30)?,
            frontend_dir: parse("PELTY_FRONTEND_DIR", PathBuf::from("frontend/dist"))?,
            themes_dir: parse("PELTY_THEMES_DIR", PathBuf::from("extensions/themes"))?,
            plugins_dir: parse("PELTY_PLUGINS_DIR", PathBuf::from("extensions/plugins"))?,
            locales_dir: parse("PELTY_LOCALES_DIR", PathBuf::from("locales"))?,
            default_locale: parse("PELTY_DEFAULT_LOCALE", "en".to_string())?,
            log_json: parse("PELTY_LOG_JSON", false)?,
            auth_rate_per_minute: parse("PELTY_AUTH_RATE_PER_MINUTE", 10)?,
            smtp_url: var("PELTY_SMTP_URL").map(Redacted),
            mail_from: parse(
                "PELTY_MAIL_FROM",
                "PeltyCMS <no-reply@localhost>".to_string(),
            )?,
            public_url: parse("PELTY_PUBLIC_URL", "http://localhost:8080".to_string())?
                .trim_end_matches('/')
                .to_string(),
            encryption_key: var("PELTY_ENCRYPTION_KEY").map(Redacted),
            trusted_proxies: var("PELTY_TRUSTED_PROXIES")
                .map(|v| {
                    v.split([',', ' '])
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default(),
            scheduler: parse("PELTY_SCHEDULER", true)?,
            scheduler_interval_secs: parse("PELTY_SCHEDULER_INTERVAL_SECS", 60)?,
            worker_concurrency: parse("PELTY_WORKER_CONCURRENCY", 10)?,
            redis_url: var("PELTY_REDIS_URL").map(Redacted),
            uploads: UploadLimits::from_env()?,
            addon_trusted_keys: var("PELTY_ADDON_TRUSTED_KEYS").map(PathBuf::from),
            addon_require_signatures: parse("PELTY_ADDON_REQUIRE_SIGNATURES", false)?,
            geoip_db: var("PELTY_GEOIP_DB").map(PathBuf::from),
            ai: AiConfig {
                // OpenAI-compatible API base URL including `/v1` (HTTPS, or HTTP on loopback).
                // AI ticket reply suggestions stay off unless all three PELTY_AI_* are set.
                base_url: var("PELTY_AI_BASE_URL"),
                // API key for the AI provider. Secret; never logged or shown in the admin UI.
                api_key: var("PELTY_AI_API_KEY").map(Redacted),
                // Model name sent to the provider, e.g. `gpt-4o-mini`.
                model: var("PELTY_AI_MODEL"),
            },
        };
        if c.addon_require_signatures && c.addon_trusted_keys.is_none() {
            return Err(ConfigError::Invalid(
                "PELTY_ADDON_TRUSTED_KEYS",
                "required when addon signatures are enforced".into(),
            ));
        }
        if !(5..=3600).contains(&c.scheduler_interval_secs) {
            return Err(ConfigError::Invalid(
                "PELTY_SCHEDULER_INTERVAL_SECS",
                "5..=3600".into(),
            ));
        }
        if !(1..=200).contains(&c.worker_concurrency) {
            return Err(ConfigError::Invalid(
                "PELTY_WORKER_CONCURRENCY",
                "1..=200".into(),
            ));
        }
        if let Some(r) = &c.redis_url {
            if !r.0.starts_with("redis://")
                && !r.0.starts_with("rediss://")
                && !r.0.starts_with("redis+unix://")
                && !r.0.starts_with("unix://")
            {
                return Err(ConfigError::Invalid(
                    "PELTY_REDIS_URL",
                    "must be a redis:// URL".into(),
                ));
            }
        }
        if !c.public_url.starts_with("http://") && !c.public_url.starts_with("https://") {
            return Err(ConfigError::Invalid(
                "PELTY_PUBLIC_URL",
                "must start with http(s)://".into(),
            ));
        }
        if c.session_idle_minutes <= 0 || c.session_absolute_hours <= 0 {
            return Err(ConfigError::Invalid(
                "PELTY_SESSION_*",
                "must be positive".into(),
            ));
        }
        if c.db_max_connections == 0 {
            return Err(ConfigError::Invalid("PELTY_DB_MAX_CONNECTIONS", "0".into()));
        }
        Ok(c)
    }
}
