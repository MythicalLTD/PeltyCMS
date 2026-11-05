//! Admin SMTP settings. Stored in `system_settings` under `mail.smtp`; the
//! password is sealed with the app SecretBox (AES-256-GCM) and never
//! returned. When enabled these settings replace PELTY_SMTP_URL.
use axum::{extract::State, routing::get, routing::post, Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, settings};
use pelty_notifications::{Email, Mailer, SmtpMailer, SmtpSecurity};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::{
    error::ApiResult,
    extract::{perms, RequirePermission},
    AppState,
};

pub const KEY: &str = "mail.smtp";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/settings/smtp", get(get_smtp).put(put_smtp))
        .route("/admin/settings/smtp/test", post(test_smtp))
}

/// Persisted form (password sealed, base64).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    enabled: bool,
    host: String,
    port: u16,
    security: String,
    username: String,
    password_sealed: Option<String>,
    from_address: String,
    from_name: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SmtpView {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    /// `tls` | `starttls` | `none`
    pub security: String,
    pub username: String,
    pub password_set: bool,
    pub from_address: String,
    pub from_name: String,
    /// Which transport is live now: `settings`, `env` or `log`.
    pub source: String,
    /// False when PELTY_ENCRYPTION_KEY is unset (passwords cannot be stored).
    pub encryption_available: bool,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SmtpInput {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub security: String,
    #[serde(default)]
    pub username: String,
    /// New password. Omit or null to keep the stored one.
    #[serde(default)]
    pub password: Option<String>,
    /// Remove the stored password.
    #[serde(default)]
    pub clear_password: bool,
    pub from_address: String,
    #[serde(default)]
    pub from_name: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct SmtpTest {
    pub to: String,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct SmtpTestResult {
    pub ok: bool,
    /// Transport error text when `ok` is false.
    pub error: Option<String>,
}

fn security(s: &str) -> Option<SmtpSecurity> {
    match s {
        "tls" => Some(SmtpSecurity::Tls),
        "starttls" => Some(SmtpSecurity::StartTls),
        "none" => Some(SmtpSecurity::None),
        _ => None,
    }
}

async fn load(s: &AppState) -> AppResult<Stored> {
    Ok(settings::get(&s.pool, KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| Stored {
            port: 587,
            security: "starttls".into(),
            ..Default::default()
        }))
}

fn from_header(st: &Stored) -> String {
    if st.from_name.trim().is_empty() {
        st.from_address.clone()
    } else {
        format!("\"{}\" <{}>", st.from_name.replace(['"', '\\'], ""), st.from_address)
    }
}

fn build(s: &AppState, st: &Stored, timeout: u64) -> Result<SmtpMailer, String> {
    let sec = security(&st.security).ok_or("invalid security")?;
    let creds = if st.username.is_empty() {
        None
    } else {
        let pw = match &st.password_sealed {
            Some(b) => {
                let sealed = STANDARD.decode(b).map_err(|_| "corrupt password")?;
                let sb = s.secret_box.as_ref().ok_or("PELTY_ENCRYPTION_KEY not set")?;
                String::from_utf8(sb.open(&sealed).ok_or("cannot decrypt password")?)
                    .map_err(|_| "corrupt password")?
            }
            None => String::new(),
        };
        Some((st.username.clone(), pw))
    };
    SmtpMailer::from_parts(&st.host, st.port, sec, creds, &from_header(st), timeout)
}

/// Rebuild the runtime mailer from stored settings.
pub async fn reload(s: &AppState) -> AppResult<()> {
    let st = load(s).await?;
    let m: Option<Arc<dyn Mailer>> = if st.enabled {
        match build(s, &st, 30) {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                tracing::error!(error = %e, "stored SMTP settings unusable; falling back");
                None
            }
        }
    } else {
        None
    };
    if let Ok(mut g) = s.smtp_runtime.write() {
        *g = m;
    }
    Ok(())
}

fn view(s: &AppState, st: Stored) -> SmtpView {
    let live = s.smtp_runtime.read().map(|g| g.is_some()).unwrap_or(false);
    let source = if live {
        "settings"
    } else if s.mailer.name() == "log" {
        "log"
    } else {
        "env"
    };
    SmtpView {
        enabled: st.enabled,
        host: st.host,
        port: st.port,
        security: st.security,
        username: st.username,
        password_set: st.password_sealed.is_some(),
        from_address: st.from_address,
        from_name: st.from_name,
        source: source.into(),
        encryption_available: s.secret_box.is_some(),
    }
}

#[utoipa::path(get, path = "/api/v1/admin/settings/smtp", responses((status = 200, body = SmtpView), (status = 403)))]
pub async fn get_smtp(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<SmtpView>> {
    let st = load(&s).await?;
    Ok(Json(view(&s, st)))
}

fn valid_host(h: &str) -> bool {
    !h.is_empty()
        && h.len() <= 253
        && h.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']'))
}

#[utoipa::path(put, path = "/api/v1/admin/settings/smtp", request_body = SmtpInput, responses((status = 200, body = SmtpView), (status = 422)))]
pub async fn put_smtp(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(i): Json<SmtpInput>,
) -> ApiResult<Json<SmtpView>> {
    let mut st = load(&s).await?;
    let host = i.host.trim().to_string();
    let from_address = i.from_address.trim().to_string();
    let username = i.username.trim().to_string();
    if security(&i.security).is_none() {
        return Err(AppError::Validation("error.smtp_invalid").into());
    }
    if (i.enabled || !host.is_empty()) && (!valid_host(&host) || i.port == 0) {
        return Err(AppError::Validation("error.smtp_invalid").into());
    }
    if i.enabled && from_address.parse::<pelty_notifications::mailer::Address>().is_err() {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    if i.from_name.chars().any(char::is_control) || i.from_name.len() > 100 || username.len() > 255 {
        return Err(AppError::Validation("error.smtp_invalid").into());
    }
    if i.clear_password {
        st.password_sealed = None;
    }
    if let Some(pw) = i.password.filter(|p| !p.is_empty()) {
        let sb = s
            .secret_box
            .as_ref()
            .ok_or(AppError::BadRequest("error.encryption_unavailable"))?;
        st.password_sealed = Some(STANDARD.encode(sb.seal(pw.as_bytes())));
    }
    st.enabled = i.enabled;
    st.host = host;
    st.port = i.port;
    st.security = i.security;
    st.username = username;
    st.from_address = from_address;
    st.from_name = i.from_name.trim().to_string();
    if st.enabled {
        build(&s, &st, 30).map_err(|_| AppError::Validation("error.smtp_invalid"))?;
    }
    settings::set(
        &s.pool,
        KEY,
        serde_json::to_value(&st).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.smtp",
        None,
        u.ip.as_deref(),
        serde_json::json!({ "enabled": st.enabled, "host": st.host }),
    )
    .await?;
    reload(&s).await?;
    Ok(Json(view(&s, st)))
}

/// Send a test mail synchronously with the *saved* settings.
#[utoipa::path(post, path = "/api/v1/admin/settings/smtp/test", request_body = SmtpTest, responses((status = 200, body = SmtpTestResult), (status = 422)))]
pub async fn test_smtp(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(t): Json<SmtpTest>,
) -> ApiResult<Json<SmtpTestResult>> {
    let to = t.to.trim().to_string();
    if to.parse::<pelty_notifications::mailer::Address>().is_err() {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    let st = load(&s).await?;
    if st.host.is_empty() {
        return Err(AppError::Validation("error.smtp_invalid").into());
    }
    let locale = s.cfg.default_locale.clone();
    let text = s.i18n.t(&locale, "smtp.test_body").to_string();
    let email = Email {
        to,
        subject: s.i18n.t(&locale, "smtp.test_subject").to_string(),
        html: pelty_notifications::template::text_to_html(&text),
        text,
        list_unsubscribe: None,
    };
    let res = match build(&s, &st, 15) {
        Ok(m) => m.send(&email).await,
        Err(e) => Err(e),
    };
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.smtp_test",
        None,
        u.ip.as_deref(),
        serde_json::json!({ "ok": res.is_ok() }),
    )
    .await?;
    Ok(Json(SmtpTestResult {
        ok: res.is_ok(),
        error: res.err(),
    }))
}
