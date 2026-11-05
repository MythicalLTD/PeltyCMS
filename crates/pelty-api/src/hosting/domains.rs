//! Domain lifecycle driven by the job queue: register/transfer paid orders,
//! apply paid renewals, raise auto-renew invoices, expire lapsed domains and
//! send expiry reminders. Registrar calls go through `RegistrarModule`.
use crate::{mail, AppState};
use pelty_core::AppResult;
use pelty_database::domains::{self as db, Domain};
use pelty_database::{customers, jobs, users};
use pelty_provisioning::registrar::{Contact, RegistrarError};
use serde::Serialize;
use serde_json::json;
use time::OffsetDateTime;

pub const PROCESS_JOB: &str = "domains.process";
pub const DAILY_JOB: &str = "domains.daily";
/// Days before expiry an auto-renew invoice is raised.
pub const RENEW_LEAD_DAYS: i64 = 14;
/// Domains synced with their registrar per daily run.
pub const SYNC_BATCH: i64 = 200;
pub const REMINDER_DAYS: &[i64] = &[30, 7, 1];

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct ProcessStats {
    pub registered: u32,
    pub renewed: u32,
    pub failed: u32,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct DailyStats {
    pub expired: u64,
    pub renewal_invoices: u32,
    pub reminders: u32,
    pub synced: u32,
    pub sync_failed: u32,
    pub registrant_reminders: u32,
    pub registrant_overdue: u32,
}

fn today() -> time::Date {
    OffsetDateTime::now_utc().date()
}

fn contact_of(d: &Domain) -> Contact {
    serde_json::from_value(d.contact.clone()).unwrap_or_default()
}

/// Run registrar operations for paid orders and renewals.
pub async fn process(s: &AppState) -> AppResult<ProcessStats> {
    let mut st = ProcessStats::default();
    let t = today();
    for (d, sealed) in db::paid_pending(&s.pool).await? {
        let Some(reg) = crate::addons::registrar(s, &d.registrar).await? else {
            db::mark_error(&s.pool, d.id, "unknown registrar", true).await?;
            st.failed += 1;
            continue;
        };
        let exp = db::add_years(t, d.years);
        let exp_s = exp.to_string();
        let r = if d.kind == "transfer" {
            let code = sealed
                .and_then(|b| s.secret_box.as_ref().and_then(|sb| sb.open(&b)))
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default();
            reg.transfer_with_fields(&d.name, &code, &exp_s, &d.extra_fields)
                .await
        } else {
            #[derive(serde::Serialize, serde::Deserialize)]
            struct RegInput {
                nameservers: Vec<String>,
                extra_fields: serde_json::Value,
            }
            let i = crate::addon_functions::rewrite_input(
                s,
                "domain.register",
                serde_json::json!({ "domain_id": d.id, "name": d.name, "registrar": d.registrar, "years": d.years }),
                RegInput { nameservers: d.nameservers.clone(), extra_fields: d.extra_fields.clone() },
            )
            .await;
            let ns = if i.nameservers.len() <= 13
                && i.nameservers
                    .iter()
                    .all(|n| !n.trim().is_empty() && n.len() <= 253)
            {
                i.nameservers
            } else {
                d.nameservers.clone()
            };
            reg.register_with_fields(
                &d.name,
                d.years as u32,
                &exp_s,
                &contact_of(&d),
                &ns,
                &i.extra_fields,
            )
            .await
        };
        match r {
            Ok(()) => {
                db::mark_active(&s.pool, d.id, t, exp).await?;
                st.registered += 1;
                if let Some(fresh) = db::get(&s.pool, d.id).await? {
                    crate::registrant_verify::require(s, &fresh, t, false).await?;
                }
            }
            Err(e) => {
                let fatal = matches!(e, RegistrarError::Unavailable | RegistrarError::BadAuthCode);
                db::mark_error(&s.pool, d.id, &e.to_string(), fatal).await?;
                st.failed += 1;
            }
        }
    }
    for r in db::paid_renewals(&s.pool).await? {
        let Some(d) = db::get(&s.pool, r.domain_id).await? else {
            continue;
        };
        let Some(reg) = crate::addons::registrar(s, &d.registrar).await? else {
            continue;
        };
        let new_exp = db::add_years(r.for_expiry, r.years);
        match reg
            .renew(&d.name, r.years as u32, &new_exp.to_string())
            .await
        {
            Ok(()) => {
                db::apply_renewal(&s.pool, &r, new_exp).await?;
                st.renewed += 1;
            }
            Err(e) => {
                db::mark_error(&s.pool, d.id, &e.to_string(), false).await?;
                st.failed += 1;
            }
        }
    }
    Ok(st)
}

pub async fn recipient_pub(
    s: &AppState,
    d: &Domain,
) -> AppResult<Option<(String, String, String)>> {
    recipient(s, d).await
}

async fn recipient(s: &AppState, d: &Domain) -> AppResult<Option<(String, String, String)>> {
    let Some(c) = customers::get(&s.pool, d.customer_id).await? else {
        return Ok(None);
    };
    if let Some(uid) = c.owner_user_id {
        if let Some(u) = users::find(&s.pool, uid).await? {
            let name = if u.display_name.is_empty() {
                u.email.clone()
            } else {
                u.display_name.clone()
            };
            return Ok(Some((u.email.clone(), mail::user_locale(s, &u), name)));
        }
    }
    Ok(Some((
        c.email.clone(),
        s.cfg.default_locale.clone(),
        c.first_name.clone(),
    )))
}

/// Daily pass: expire, raise auto-renew invoices, send reminders.
pub async fn daily(s: &AppState, today: time::Date) -> AppResult<DailyStats> {
    let mut st = DailyStats {
        expired: db::expire_overdue(&s.pool, today).await?,
        ..Default::default()
    };
    (st.registrant_reminders, st.registrant_overdue) =
        crate::registrant_verify::daily(s, today).await?;
    for d in db::due_auto_renew(&s.pool, today, RENEW_LEAD_DAYS).await? {
        let cur = customers::get(&s.pool, d.customer_id)
            .await?
            .map(|c| c.currency)
            .unwrap_or_default();
        let Some(t) = db::tld(&s.pool, &d.tld, &cur).await? else {
            continue;
        };
        if db::create_renewal(&s.pool, &d, &t, 1).await?.is_some() {
            st.renewal_invoices += 1;
        }
    }
    let max = REMINDER_DAYS.iter().copied().max().unwrap_or(0);
    for d in db::expiring(&s.pool, today, max).await? {
        let Some(exp) = d.expires_on else { continue };
        let left = (exp - today).whole_days();
        let Some(th) = db::threshold(left, REMINDER_DAYS) else {
            continue;
        };
        if !db::claim_reminder(&s.pool, &d, th).await? {
            continue;
        }
        if let Some((to, locale, name)) = recipient(s, &d).await? {
            let link = format!("{}/domains/{}", s.cfg.public_url, d.id);
            let (days, exps) = (left.to_string(), exp.to_string());
            mail::enqueue(
                s,
                "domain_expiry",
                &locale,
                &to,
                mail::vars(&[
                    ("name", &name),
                    ("domain", &d.name),
                    ("expires_on", &exps),
                    ("days", &days),
                    ("link", &link),
                ]),
            )
            .await?;
            st.reminders += 1;
        }
    }
    // Registrar sync: pull expiry/status/nameservers for a batch per day.
    for d in db::sync_batch(&s.pool, SYNC_BATCH).await? {
        match crate::routes::domains::sync_one(s, &d).await {
            Ok(()) => st.synced += 1,
            Err(e) => {
                db::mark_error(&s.pool, d.id, &format!("sync: {}", e.0), false).await?;
                st.sync_failed += 1;
            }
        }
    }
    Ok(st)
}

/// Enqueue `domains.process` per 5 minutes, `usage.bill` hourly and `domains.daily`
/// once per company-time-zone day.
pub async fn schedule(s: &AppState, ok: &(dyn Fn(&str) -> bool + Sync)) -> AppResult<()> {
    let now = OffsetDateTime::now_utc();
    if ok(PROCESS_JOB) {
        jobs::enqueue(
            &s.pool,
            PROCESS_JOB,
            json!({}),
            None,
            Some(&format!("{PROCESS_JOB}:{}", now.unix_timestamp() / 300)),
            3,
        )
        .await?;
    }
    if ok(crate::routes::usage::BILL_JOB) {
        let hour = now.unix_timestamp() / 3600;
        jobs::enqueue(
            &s.pool,
            crate::routes::usage::BILL_JOB,
            json!({}),
            None,
            Some(&format!("usage.bill:{hour}")),
            3,
        )
        .await?;
    }
    if ok(DAILY_JOB) {
        jobs::enqueue(
            &s.pool,
            DAILY_JOB,
            json!({}),
            None,
            Some(&format!(
                "{DAILY_JOB}:{}",
                crate::routes::automation::local_today()
            )),
            3,
        )
        .await?;
    }
    Ok(())
}
