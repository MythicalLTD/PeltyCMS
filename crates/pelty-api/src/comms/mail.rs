//! Rendering + queueing of transactional email. Requests only enqueue an
//! `email.send` job; the worker renders and delivers it, so SMTP latency or
//! outages never block a request and failures retry with backoff.
use crate::AppState;
use pelty_core::{AppError, AppResult};
use pelty_notifications::template::{def, render, render_html, text_to_html};
use pelty_notifications::Email;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const JOB_KIND: &str = "email.send";

pub type Vars = BTreeMap<String, String>;

/// Resolved template content for one locale.
#[derive(Debug, Clone)]
pub struct TemplateSource {
    pub subject: String,
    pub body: String,
    /// Custom HTML when set; otherwise HTML is derived from `body`.
    pub html: Option<String>,
    pub customized: bool,
}

pub fn vars(pairs: &[(&str, &str)]) -> Vars {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Template for a locale: DB override, else locale-file default
/// (`email.<name>.subject|body`). Defaults never include custom HTML.
pub async fn source(s: &AppState, name: &str, locale: &str) -> AppResult<TemplateSource> {
    if let Some(t) = pelty_database::email_templates::get(&s.pool, name, locale).await? {
        return Ok(TemplateSource {
            subject: t.subject,
            body: t.body,
            html: t.html.filter(|h| !h.trim().is_empty()),
            customized: true,
        });
    }
    let sk = format!("email.{name}.subject");
    let bk = format!("email.{name}.body");
    Ok(TemplateSource {
        subject: s.i18n.t(locale, &sk).to_string(),
        body: s.i18n.t(locale, &bk).to_string(),
        html: None,
        customized: false,
    })
}

/// Build multipart text + HTML from a resolved template and merge vars.
pub fn build_parts(src: &TemplateSource, v: &Vars) -> (String, String, String) {
    let subject = render(&src.subject, v, false).replace(['\r', '\n'], " ");
    let text = render(&src.body, v, false);
    let html = match src.html.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => render_html(h, v),
        None => text_to_html(&text),
    };
    (subject, text, html)
}

pub async fn render_email(
    s: &AppState,
    name: &str,
    locale: &str,
    to: &str,
    mut v: Vars,
) -> AppResult<Email> {
    if def(name).is_none() {
        return Err(AppError::Validation("error.unknown_template"));
    }
    v.entry("company".into())
        .or_insert_with(|| s.i18n.t(locale, "app.name").to_string());
    let src = source(s, name, locale).await?;
    let (subject, text, html) = build_parts(&src, &v);
    Ok(Email {
        to: to.to_string(),
        subject,
        html,
        text,
        list_unsubscribe: None,
    })
}

/// Queue a templated email. The payload holds the merge vars (which may
/// contain one-time links); it is redacted in the API and wiped on success.
pub async fn enqueue(s: &AppState, name: &str, locale: &str, to: &str, v: Vars) -> AppResult<()> {
    if disabled(s).await?.iter().any(|d| d == name) {
        return Ok(());
    }
    if ALWAYS_ON.contains(&name) {
        return enqueue_always(s, name, locale, to, v).await;
    }
    #[derive(serde::Serialize, serde::Deserialize)]
    struct MailInput {
        locale: String,
        vars: Vars,
    }
    let i = crate::addon_functions::rewrite_input(
        s,
        "mail.enqueue",
        serde_json::json!({ "template": name, "to": to }),
        MailInput {
            locale: locale.to_string(),
            vars: v,
        },
    )
    .await;
    let locale = if i.locale.len() <= 16 {
        i.locale
    } else {
        locale.to_string()
    };
    enqueue_always(s, name, &locale, to, i.vars).await
}

/// Templates an operator may never switch off (account security).
pub const ALWAYS_ON: &[&str] = &[
    "password_reset",
    "email_verify",
    "admin_account_locked",
    "admin_password_reset",
];
const DISABLED_KEY: &str = "email_templates_disabled";

/// Template names the operator switched off (WHMCS "Disable" checkbox).
pub async fn disabled(s: &AppState) -> AppResult<Vec<String>> {
    Ok(pelty_database::settings::get(&s.pool, DISABLED_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn set_enabled(s: &AppState, name: &str, enabled: bool) -> AppResult<()> {
    if !enabled && ALWAYS_ON.contains(&name) {
        return Err(AppError::Validation("error.template_always_on"));
    }
    let mut d = disabled(s).await?;
    d.retain(|x| x != name);
    if !enabled {
        d.push(name.to_string());
    }
    pelty_database::settings::set(&s.pool, DISABLED_KEY, json!(d)).await
}

/// Queue regardless of the disabled list (test sends).
pub async fn enqueue_always(
    s: &AppState,
    name: &str,
    locale: &str,
    to: &str,
    v: Vars,
) -> AppResult<()> {
    pelty_database::jobs::enqueue(
        &s.pool,
        JOB_KIND,
        json!({ "template": name, "locale": locale, "to": to, "vars": v }),
        None,
        None,
        6,
    )
    .await?;
    Ok(())
}

/// Worker side of `email.send`.
pub async fn deliver(s: &AppState, payload: &Value) -> Result<Value, String> {
    let name = payload["template"].as_str().ok_or("missing template")?;
    let locale = payload["locale"].as_str().unwrap_or("en");
    let to = payload["to"].as_str().ok_or("missing recipient")?;
    let v: Vars = serde_json::from_value(payload["vars"].clone()).unwrap_or_default();
    let mut email = render_email(s, name, locale, to, v)
        .await
        .map_err(|e| e.to_string())?;
    // Pre-event: addons may drop the mail or rewrite subject and text.
    // Security mails (one-time links) never leave the host.
    if !SENSITIVE.contains(&name) {
        let pre = crate::addon_ext::pre_event(s, "email.before_send", json!({ "template": name, "locale": locale, "to": to, "subject": email.subject, "text": email.text })).await;
        match pre {
            Ok(p) => {
                let subject = crate::addon_ext::pre_str(&p, "subject", 500, &email.subject)
                    .replace(['\r', '\n'], " ");
                let text = crate::addon_ext::pre_str(&p, "text", 200_000, &email.text);
                if text != email.text {
                    email.html = text_to_html(&text);
                    email.text = text;
                }
                email.subject = subject;
            }
            Err(AppError::Conflict(_)) => {
                return Ok(json!({ "template": name, "cancelled": true }))
            }
            Err(e) => return Err(e.to_string()),
        }
        if crate::addon_filters::has(s, "email.content") {
            let v = crate::addon_filters::apply(
                s,
                "email.content",
                json!({ "subject": email.subject, "text": email.text }),
                json!({ "template": name, "locale": locale, "to": to }),
            )
            .await;
            let subject = v["subject"]
                .as_str()
                .map(|x| {
                    x.chars()
                        .take(500)
                        .collect::<String>()
                        .replace(['\r', '\n'], " ")
                })
                .unwrap_or_else(|| email.subject.clone());
            let text: String = v["text"]
                .as_str()
                .map(|x| x.chars().take(200_000).collect())
                .unwrap_or_else(|| email.text.clone());
            if text != email.text {
                email.html = text_to_html(&text);
                email.text = text;
            }
            email.subject = subject;
        }
    }
    let m = s.active_mailer();
    let number = payload["vars"]["ticket_number"]
        .as_str()
        .and_then(|v| v.parse::<i64>().ok());
    let ticket_headers = if name.starts_with("ticket_") {
        if let Some(number) = number {
            let row: Option<(uuid::Uuid, String)> = sqlx::query_as("SELECT t.id,d.email FROM tickets t JOIN ticket_departments d ON d.id=t.department_id WHERE t.number=$1 AND d.email <> ''")
                .bind(number).fetch_optional(&s.pool).await.map_err(|e| e.to_string())?;
            row.map(
                |(id, reply_to)| pelty_notifications::mailer::TicketHeaders {
                    reply_to,
                    message_id: format!("<ticket.{id}.{}@pelty.invalid>", uuid::Uuid::new_v4()),
                },
            )
        } else {
            None
        }
    } else {
        None
    };
    let res = if let Some(headers) = ticket_headers {
        m.send_ticket(&email, &headers).await
    } else {
        m.send(&email).await
    };
    let entry = pelty_database::email_log::NewEntry {
        recipient: to,
        template: name,
        locale,
        subject: &email.subject,
        // Security mails carry one-time links: never keep their body.
        body_text: if SENSITIVE.contains(&name) {
            REDACTED
        } else {
            &email.text
        },
        status: if res.is_ok() { "sent" } else { "failed" },
        error: res.as_ref().err().map(String::as_str),
        mailer: m.name(),
    };
    // Logging never turns a delivered mail into a retry.
    if let Err(e) = pelty_database::email_log::record(&s.pool, &entry).await {
        tracing::warn!(error = %e, "email log insert failed");
    }
    if res.is_ok() {
        s.emit(pelty_extensions::events::DomainEvent::new(
            "email.sent",
            json!({ "template": name, "to": to, "subject": email.subject, "mailer": m.name() }),
        ));
    } else {
        s.emit(pelty_extensions::events::DomainEvent::new("email.failed", json!({ "template": name, "to": to, "subject": email.subject, "error": res.as_ref().err() })));
    }
    res?;
    Ok(json!({ "template": name, "mailer": m.name() }))
}

/// Locale to write to a user in: their preference, else the default.
pub fn user_locale(s: &AppState, u: &pelty_database::users::User) -> String {
    u.locale
        .clone()
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone())
}

/// Re-send a logged mail exactly as it was rendered (WHMCS "Resend").
pub async fn resend_logged(
    s: &AppState,
    d: &pelty_database::email_log::EmailLogDetail,
) -> AppResult<()> {
    pelty_database::jobs::enqueue(
        &s.pool,
        RAW_JOB_KIND,
        json!({ "to": d.entry.recipient, "subject": d.entry.subject, "text": d.body_text, "template": d.entry.template, "locale": d.entry.locale }),
        None,
        None,
        6,
    )
    .await?;
    Ok(())
}

/// Body stored for security mails.
pub const REDACTED: &str = "[redacted]";

/// Templates carrying live tokens or credentials: bodies are never logged
/// and staff can never resend them (a resend would replay the secret).
pub const SENSITIVE: &[&str] = &[
    "password_reset",
    "email_verify",
    "registrant_verify",
    "admin_account_locked",
    "admin_password_reset",
    "contact_invite",
    "guest_claim",
    "new_service_info",
    "client_invite",
    "client_welcome",
    "staff_invite",
];

pub const RAW_JOB_KIND: &str = "email.send_raw";

/// Worker side of `email.send_raw` (already-rendered body).
pub async fn deliver_raw(s: &AppState, p: &Value) -> Result<Value, String> {
    let to = p["to"].as_str().ok_or("missing recipient")?;
    let text = p["text"].as_str().unwrap_or_default().to_string();
    let email = Email {
        to: to.to_string(),
        subject: p["subject"]
            .as_str()
            .unwrap_or_default()
            .replace(['\r', '\n'], " "),
        html: text_to_html(&text),
        text,
        list_unsubscribe: None,
    };
    let m = s.active_mailer();
    let res = m.send(&email).await;
    let entry = pelty_database::email_log::NewEntry {
        recipient: to,
        template: p["template"].as_str().unwrap_or("resend"),
        locale: p["locale"].as_str().unwrap_or("en"),
        subject: &email.subject,
        body_text: &email.text,
        status: if res.is_ok() { "sent" } else { "failed" },
        error: res.as_ref().err().map(String::as_str),
        mailer: m.name(),
    };
    if let Err(e) = pelty_database::email_log::record(&s.pool, &entry).await {
        tracing::warn!(error = %e, "email log insert failed");
    }
    res?;
    Ok(json!({ "resent": true }))
}
