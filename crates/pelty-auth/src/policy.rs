//! Admin-configurable security policy (`security.policy` system setting):
//! password rules, session lifetimes and the login rate limit. Read through a
//! short in-process cache; [`invalidate`] after an update.
use pelty_core::{AppError, AppResult};
use pelty_database::Pool as PgPool;
use serde_json::{json, Value};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const SETTING_KEY: &str = "security.policy";

#[derive(Debug, Clone, PartialEq)]
pub struct SecurityPolicy {
    pub password_min_length: usize,
    pub require_upper: bool,
    pub require_lower: bool,
    pub require_digit: bool,
    pub require_symbol: bool,
    pub session_idle_minutes: Option<i64>,
    pub session_absolute_hours: Option<i64>,
    pub login_rate_per_minute: Option<u32>,
    /// Staff must re-enter their password for sensitive changes when the last
    /// full authentication is older than this (step-up / sudo mode).
    pub admin_reauth_minutes: Option<i64>,
    /// Staff passwords expire after this many days (must be changed).
    pub password_max_age_days: Option<i64>,
    /// Audit + notify on staff sign-ins from a new IP address.
    pub login_alerts: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        SecurityPolicy {
            password_min_length: 12,
            require_upper: false,
            require_lower: false,
            require_digit: false,
            require_symbol: false,
            session_idle_minutes: None,
            session_absolute_hours: None,
            login_rate_per_minute: None,
            admin_reauth_minutes: None,
            password_max_age_days: None,
            login_alerts: true,
        }
    }
}

impl SecurityPolicy {
    /// Parse + validate (ranges) a JSON document; missing keys keep defaults.
    pub fn from_value(v: &Value) -> AppResult<Self> {
        let d = SecurityPolicy::default();
        let bad = AppError::Validation("error.security_policy");
        let b = |k: &str, def: bool| v[k].as_bool().unwrap_or(def);
        let opt = |k: &str| -> Result<Option<i64>, AppError> {
            match &v[k] {
                Value::Null => Ok(None),
                x => x
                    .as_i64()
                    .map(Some)
                    .ok_or(AppError::Validation("error.security_policy")),
            }
        };
        let min = match &v["password_min_length"] {
            Value::Null => d.password_min_length,
            x => x
                .as_u64()
                .ok_or(AppError::Validation("error.security_policy"))? as usize,
        };
        if !(8..=128).contains(&min) {
            return Err(bad);
        }
        let idle = opt("session_idle_minutes")?;
        let abs = opt("session_absolute_hours")?;
        let rate = opt("login_rate_per_minute")?;
        let reauth = opt("admin_reauth_minutes")?;
        let max_age = opt("password_max_age_days")?;
        if reauth.is_some_and(|m| !(1..=60 * 24 * 7).contains(&m))
            || max_age.is_some_and(|d| !(1..=3650).contains(&d))
        {
            return Err(AppError::Validation("error.security_policy"));
        }
        if idle.is_some_and(|m| !(5..=60 * 24 * 30).contains(&m))
            || abs.is_some_and(|h| !(1..=24 * 365).contains(&h))
            || rate.is_some_and(|r| !(1..=10_000).contains(&r))
        {
            return Err(bad);
        }
        Ok(SecurityPolicy {
            password_min_length: min,
            require_upper: b("require_upper", d.require_upper),
            require_lower: b("require_lower", d.require_lower),
            require_digit: b("require_digit", d.require_digit),
            require_symbol: b("require_symbol", d.require_symbol),
            session_idle_minutes: idle,
            session_absolute_hours: abs,
            login_rate_per_minute: rate.map(|r| r as u32),
            admin_reauth_minutes: reauth,
            password_max_age_days: max_age,
            login_alerts: b("login_alerts", d.login_alerts),
        })
    }

    pub fn to_value(&self) -> Value {
        json!({
            "password_min_length": self.password_min_length,
            "require_upper": self.require_upper,
            "require_lower": self.require_lower,
            "require_digit": self.require_digit,
            "require_symbol": self.require_symbol,
            "session_idle_minutes": self.session_idle_minutes,
            "session_absolute_hours": self.session_absolute_hours,
            "login_rate_per_minute": self.login_rate_per_minute,
            "admin_reauth_minutes": self.admin_reauth_minutes,
            "password_max_age_days": self.password_max_age_days,
            "login_alerts": self.login_alerts,
        })
    }

    /// Configurable rules on top of the always-on checks in `password::check_policy_min`.
    pub fn check_password(&self, password: &str, email: &str) -> AppResult<()> {
        crate::password::check_policy_min(password, email, self.password_min_length)?;
        let has = |f: fn(char) -> bool| password.chars().any(f);
        if (self.require_upper && !has(char::is_uppercase))
            || (self.require_lower && !has(char::is_lowercase))
            || (self.require_digit && !has(|c| c.is_ascii_digit()))
            || (self.require_symbol && !has(|c| !c.is_alphanumeric() && !c.is_whitespace()))
        {
            return Err(AppError::Validation("error.password_complexity"));
        }
        Ok(())
    }
}

static CACHE: Mutex<Option<(Instant, SecurityPolicy)>> = Mutex::new(None);

pub fn invalidate() {
    if let Ok(mut c) = CACHE.lock() {
        *c = None;
    }
}

/// Current policy (cached 15 s). Invalid stored JSON falls back to defaults.
pub async fn load(pool: &PgPool) -> AppResult<SecurityPolicy> {
    if let Ok(c) = CACHE.lock() {
        if let Some((t, p)) = c.as_ref() {
            if t.elapsed() < Duration::from_secs(15) {
                return Ok(p.clone());
            }
        }
    }
    let v = pelty_database::settings::get(pool, SETTING_KEY).await?;
    let p = v
        .and_then(|v| SecurityPolicy::from_value(&v).ok())
        .unwrap_or_default();
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((Instant::now(), p.clone()));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parse_and_check() {
        assert_eq!(
            SecurityPolicy::from_value(&json!({})).unwrap(),
            SecurityPolicy::default()
        );
        assert!(SecurityPolicy::from_value(&json!({"password_min_length": 4})).is_err());
        assert!(SecurityPolicy::from_value(&json!({"session_idle_minutes": 1})).is_err());
        let p = SecurityPolicy::from_value(
            &json!({"password_min_length": 10, "require_digit": true, "require_symbol": true}),
        )
        .unwrap();
        assert!(p.check_password("Zebra-Quartz-Pine", "a@b.c").is_err()); // no digit
        assert!(p.check_password("Zebra-Quartz-9", "a@b.c").is_ok());
        assert!(p.check_password("Zebra9Quar", "a@b.c").is_err()); // no symbol
        let a = SecurityPolicy::from_value(&json!({"admin_reauth_minutes": 15, "password_max_age_days": 90, "login_alerts": false})).unwrap();
        assert_eq!(
            (
                a.admin_reauth_minutes,
                a.password_max_age_days,
                a.login_alerts
            ),
            (Some(15), Some(90), false)
        );
        assert_eq!(SecurityPolicy::from_value(&a.to_value()).unwrap(), a);
        assert!(SecurityPolicy::from_value(&json!({"admin_reauth_minutes": 0})).is_err());
        assert!(SecurityPolicy::from_value(&json!({"password_max_age_days": 99999})).is_err());
        let short = SecurityPolicy::from_value(&json!({"password_min_length": 8})).unwrap();
        assert!(short.check_password("Qz7!kLpw", "a@b.c").is_ok());
        assert!(SecurityPolicy::default()
            .check_password("Qz7!kLpw", "a@b.c")
            .is_err());
    }
}
