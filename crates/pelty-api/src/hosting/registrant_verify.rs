//! ICANN WHOIS accuracy: after a registration, transfer or registrant
//! e-mail change the registrant must confirm their address within 15 days.
//! An address confirmed once is trusted for later domains. Reminders go out
//! 10 and 5 days before the deadline; past it the request is `overdue`
//! (the registrar suspends such names on its side; staff see it here).
use crate::{mail, AppState};
use pelty_core::AppResult;
use pelty_database::domains::Domain;
use pelty_database::registrant_verify as db;
use serde_json::json;
use time::{Date, Duration};

pub const DEADLINE_DAYS: i64 = 15;
pub const REMIND_AT: &[i64] = &[10, 5];
const ENABLED_KEY: &str = "registrant_verification_enabled";

pub async fn enabled(s: &AppState) -> AppResult<bool> {
    Ok(pelty_database::settings::get(&s.pool, ENABLED_KEY)
        .await?
        .and_then(|v| v.as_bool())
        .unwrap_or(true))
}

pub async fn set_enabled(s: &AppState, on: bool) -> AppResult<()> {
    pelty_database::settings::set(&s.pool, ENABLED_KEY, json!(on)).await
}

/// The registrant address: the domain contact's e-mail, else the account's.
pub async fn registrant_email(
    s: &AppState,
    d: &Domain,
) -> AppResult<Option<(String, String, String)>> {
    let Some((to, locale, name)) = crate::domains::recipient_pub(s, d).await? else {
        return Ok(None);
    };
    let contact = d
        .contact
        .get("email")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|e| e.contains('@'));
    let cname = d
        .contact
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|n| !n.trim().is_empty())
        .map(str::to_string);
    Ok(Some(match contact {
        Some(e) => (e.to_string(), locale, cname.unwrap_or(name)),
        None => (to, locale, name),
    }))
}

async fn send(
    s: &AppState,
    d: &Domain,
    email: &str,
    locale: &str,
    name: &str,
    token: &str,
    deadline: Date,
) -> AppResult<()> {
    let link = format!(
        "{}/verify-registrant?token={token}",
        s.cfg.public_url.trim_end_matches('/')
    );
    let dl = deadline.to_string();
    mail::enqueue_always(
        s,
        "registrant_verify",
        locale,
        email,
        mail::vars(&[
            ("name", name),
            ("domain", &d.name),
            ("email", email),
            ("deadline", &dl),
            ("link", &link),
        ]),
    )
    .await
}

/// Ask for verification unless the address is already known good.
/// `force` re-asks even for a verified address (staff action).
pub async fn require(
    s: &AppState,
    d: &Domain,
    today: Date,
    force: bool,
) -> AppResult<Option<db::Verification>> {
    if !enabled(s).await? {
        return Ok(None);
    }
    let Some((email, locale, name)) = registrant_email(s, d).await? else {
        return Ok(None);
    };
    let token = pelty_auth::token::generate();
    let h = pelty_auth::token::hash(&token);
    if !force && db::email_verified(&s.pool, &email).await? {
        return Ok(Some(
            db::mark_verified_direct(&s.pool, d.id, &email, &h, today).await?,
        ));
    }
    let deadline = today + Duration::days(DEADLINE_DAYS);
    let v = db::start(&s.pool, d.id, &email, &h, deadline).await?;
    send(s, d, &email, &locale, &name, &token, deadline).await?;
    pelty_database::audit::record(
        &s.pool,
        None,
        "domain.registrant_verification_sent",
        Some(&d.id.to_string()),
        None,
        json!({ "deadline": deadline.to_string() }),
    )
    .await?;
    Ok(Some(v))
}

/// Contact changed: only a new address needs verifying.
pub async fn contact_changed(
    s: &AppState,
    before: &Domain,
    after: &Domain,
    today: Date,
) -> AppResult<()> {
    let old = registrant_email(s, before)
        .await?
        .map(|x| x.0.to_ascii_lowercase());
    let new = registrant_email(s, after)
        .await?
        .map(|x| x.0.to_ascii_lowercase());
    if old != new && after.status == "active" {
        require(s, after, today, false).await?;
    }
    Ok(())
}

/// Resend the mail with a fresh token (deadline unchanged).
pub async fn resend(
    s: &AppState,
    d: &Domain,
    reminder: bool,
) -> AppResult<Option<db::Verification>> {
    let Some(cur) = db::get(&s.pool, d.id).await? else {
        return Ok(None);
    };
    if cur.status == "verified" {
        return Ok(Some(cur));
    }
    let Some((_, locale, name)) = registrant_email(s, d).await? else {
        return Ok(None);
    };
    let token = pelty_auth::token::generate();
    let Some(v) = db::rotate(&s.pool, d.id, &pelty_auth::token::hash(&token), reminder).await?
    else {
        return Ok(None);
    };
    send(s, d, &v.email, &locale, &name, &token, v.deadline).await?;
    Ok(Some(v))
}

/// Daily pass: reminders, then overdue marking.
pub async fn daily(s: &AppState, today: Date) -> AppResult<(u32, u32)> {
    let mut reminded = 0;
    for v in db::open(&s.pool).await? {
        let left = (v.deadline - today).whole_days();
        // reminders already sent = how many thresholds were passed.
        let due = REMIND_AT.iter().filter(|&&t| left <= t).count() as i32;
        if left >= 0 && due > v.reminders {
            if let Some(d) = pelty_database::domains::get(&s.pool, v.domain_id).await? {
                resend(s, &d, true).await?;
                reminded += 1;
            }
        }
    }
    let over = db::mark_overdue(&s.pool, today).await?;
    for id in &over {
        pelty_database::audit::record(
            &s.pool,
            None,
            "domain.registrant_verification_overdue",
            Some(&id.to_string()),
            None,
            json!({}),
        )
        .await?;
    }
    Ok((reminded, over.len() as u32))
}

/// Consume a token from the e-mail link.
pub async fn verify(s: &AppState, token: &str) -> AppResult<usize> {
    if token.len() != 43 {
        return Ok(0);
    }
    let ids = db::verify_token(&s.pool, &pelty_auth::token::hash(token)).await?;
    for id in &ids {
        pelty_database::audit::record(
            &s.pool,
            None,
            "domain.registrant_verified",
            Some(&id.to_string()),
            None,
            json!({}),
        )
        .await?;
    }
    Ok(ids.len())
}
