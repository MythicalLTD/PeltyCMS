//! Admin editing of email templates. Overrides are stored per (template,
//! locale); deleting one reverts to the locale-file default. Optional
//! `html` replaces the auto-generated branded wrapper.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::mail;
use crate::AppState;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::{audit, email_templates};
use pelty_notifications::template::{def, unknown_vars, TEMPLATES};
use serde::Deserialize;
use serde_json::{json, Value};
use utoipa::ToSchema;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/email-templates", get(list))
        .route(
            "/admin/email-templates/{name}/{locale}",
            put(save).delete(revert),
        )
        .route(
            "/admin/email-templates/{name}/{locale}/preview",
            post(preview),
        )
        .route(
            "/admin/email-templates/{name}/{locale}/test",
            post(send_test),
        )
        .route("/admin/email-templates/{name}/enabled", put(set_enabled))
}

#[derive(Deserialize, ToSchema)]
pub struct TemplateBody {
    pub subject: String,
    pub body: String,
    /// Custom HTML. Empty / omitted → branded HTML from plain text.
    #[serde(default)]
    pub html: Option<String>,
}

fn check(
    s: &AppState,
    name: &str,
    locale: &str,
) -> Result<&'static pelty_notifications::TemplateDef, AppError> {
    if !s.i18n.has(locale) {
        return Err(AppError::Validation("error.unknown_locale"));
    }
    def(name).ok_or(AppError::NotFound)
}

fn normalize_html(html: &Option<String>) -> Option<&str> {
    html.as_deref().map(str::trim).filter(|h| !h.is_empty())
}

fn validate(d: &pelty_notifications::TemplateDef, b: &TemplateBody) -> Result<(), AppError> {
    let subject = b.subject.trim();
    if subject.is_empty() || subject.chars().count() > 300 || subject.contains(['\r', '\n']) {
        return Err(AppError::Validation("error.invalid_subject"));
    }
    if b.body.trim().is_empty() || b.body.chars().count() > 20000 {
        return Err(AppError::Validation("error.invalid_body"));
    }
    if let Some(h) = normalize_html(&b.html) {
        if h.chars().count() > 100_000 {
            return Err(AppError::Validation("error.invalid_body"));
        }
        if !unknown_vars(h, d).is_empty() {
            return Err(AppError::Validation("error.unknown_template_var"));
        }
    }
    if !unknown_vars(&b.subject, d).is_empty() || !unknown_vars(&b.body, d).is_empty() {
        return Err(AppError::Validation("error.unknown_template_var"));
    }
    Ok(())
}

fn sample(d: &pelty_notifications::TemplateDef, s: &AppState) -> mail::Vars {
    d.vars
        .iter()
        .map(|v| {
            let val = match *v {
                "link" => format!("{}/example", s.cfg.public_url),
                "name" => "Ana Popescu".into(),
                "invoice_number" => "INV-000123".into(),
                "total" => "49.00 EUR".into(),
                "due_date" => "2026-11-30".into(),
                "expires_minutes" => "60".into(),
                "days_overdue" => "3".into(),
                "ticket_number" => "T-1042".into(),
                "reason" => "insufficient_funds".into(),
                "next_retry" => "2026-12-03".into(),
                other => format!("[{other}]"),
            };
            (v.to_string(), val)
        })
        .collect()
}

#[utoipa::path(get, path = "/api/v1/admin/email-templates", responses((status = 200), (status = 403)))]
pub async fn list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    let mut items = Vec::new();
    let off = mail::disabled(&s).await?;
    for d in TEMPLATES {
        let mut locales = Vec::new();
        for l in s.i18n.languages() {
            let src = mail::source(&s, d.name, &l).await?;
            locales.push(json!({
                "locale": l,
                "subject": src.subject,
                "body": src.body,
                "html": src.html.unwrap_or_default(),
                "customized": src.customized,
            }));
        }
        items.push(json!({
            "name": d.name, "vars": d.vars, "locales": locales,
            "category": pelty_notifications::template::category(d.name),
            "enabled": !off.iter().any(|x| x == d.name),
            "always_on": mail::ALWAYS_ON.contains(&d.name),
        }));
    }
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(put, path = "/api/v1/admin/email-templates/{name}/{locale}", request_body = TemplateBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn save(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path((name, locale)): Path<(String, String)>,
    Json(b): Json<TemplateBody>,
) -> ApiResult<Json<Value>> {
    let d = check(&s, &name, &locale)?;
    validate(d, &b)?;
    let html = normalize_html(&b.html);
    let t =
        email_templates::upsert(&s.pool, &name, &locale, b.subject.trim(), &b.body, html).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "email_template.updated",
        Some(&format!("{name}:{locale}")),
        u.ip.as_deref(),
        json!({ "has_html": t.html.is_some() }),
    )
    .await?;
    Ok(Json(json!({
        "locale": t.locale,
        "subject": t.subject,
        "body": t.body,
        "html": t.html.unwrap_or_default(),
        "customized": true,
    })))
}

#[utoipa::path(delete, path = "/api/v1/admin/email-templates/{name}/{locale}", responses((status = 204), (status = 404)))]
pub async fn revert(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path((name, locale)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    check(&s, &name, &locale)?;
    email_templates::delete(&s.pool, &name, &locale).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "email_template.reverted",
        Some(&format!("{name}:{locale}")),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/admin/email-templates/{name}/{locale}/preview", request_body = TemplateBody, responses((status = 200), (status = 422)))]
pub async fn preview(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
    Path((name, locale)): Path<(String, String)>,
    Json(b): Json<TemplateBody>,
) -> ApiResult<Json<Value>> {
    let d = check(&s, &name, &locale)?;
    validate(d, &b)?;
    let mut v = sample(d, &s);
    v.insert("company".into(), s.i18n.t(&locale, "app.name").to_string());
    let src = mail::TemplateSource {
        subject: b.subject,
        body: b.body,
        html: normalize_html(&b.html).map(str::to_string),
        customized: true,
    };
    let (subject, text, html) = mail::build_parts(&src, &v);
    Ok(Json(
        json!({ "subject": subject, "text": text, "html": html, "custom_html": src.html.is_some() }),
    ))
}

/// Queue the saved template, with sample data, to the signed-in admin.
#[utoipa::path(post, path = "/api/v1/admin/email-templates/{name}/{locale}/test", responses((status = 202), (status = 404)))]
pub async fn send_test(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path((name, locale)): Path<(String, String)>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let d = check(&s, &name, &locale)?;
    let me = pelty_database::users::find(&s.pool, u.user_id)
        .await?
        .ok_or(AppError::Unauthenticated)?;
    mail::enqueue_always(&s, name.as_str(), &locale, &me.email, sample(d, &s)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "email_template.test_sent",
        Some(&name),
        u.ip.as_deref(),
        json!({ "locale": locale }),
    )
    .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(json!({ "to": me.email, "mailer": s.active_mailer().name() })),
    ))
}

#[derive(Deserialize, ToSchema)]
pub struct EnabledBody {
    pub enabled: bool,
}

/// Switch a template on or off; disabled templates are never queued.
#[utoipa::path(put, path = "/api/v1/admin/email-templates/{name}/enabled", request_body = EnabledBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn set_enabled(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(name): Path<String>,
    Json(b): Json<EnabledBody>,
) -> ApiResult<Json<Value>> {
    def(&name).ok_or(AppError::NotFound)?;
    mail::set_enabled(&s, &name, b.enabled).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "email_template.toggled",
        Some(&name),
        u.ip.as_deref(),
        json!({ "enabled": b.enabled }),
    )
    .await?;
    Ok(Json(json!({ "name": name, "enabled": b.enabled })))
}
