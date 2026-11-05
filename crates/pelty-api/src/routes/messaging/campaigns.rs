//! Mass mail campaigns (`campaigns.manage`), the client's marketing opt-out
//! and the public, scanner-safe unsubscribe endpoint.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::campaigns::{self as cp, CampaignInput, Segment};
use pelty_notifications::template::{render, text_to_html};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const JOB_KIND: &str = "campaign.send";
const BATCH: i64 = 50;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/campaigns", get(list).post(create))
        .route("/admin/campaigns/preview", post(preview))
        .route("/admin/campaigns/{id}", get(detail).put(update).delete(remove))
        .route("/admin/campaigns/{id}/send", post(send))
        .route("/admin/campaigns/{id}/cancel", post(cancel))
        .route("/admin/campaigns/{id}/schedule", post(schedule).delete(unschedule))
        .route("/client/marketing", get(my_pref).put(set_pref))
        .route("/unsubscribe/{token}", get(unsub_lookup).post(unsub_apply))
}

type Cm = RequirePermission<perms::CampaignsManage>;

#[utoipa::path(get, path = "/api/v1/admin/campaigns", responses((status = 200), (status = 403)))]
pub async fn list(State(s): State<AppState>, _p: Cm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": cp::list(&s.pool).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/campaigns/preview", request_body = Segment, responses((status = 200)))]
pub async fn preview(State(s): State<AppState>, _p: Cm, Json(seg): Json<Segment>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "recipients": cp::count_segment(&s.pool, &seg).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/campaigns", request_body = CampaignInput, responses((status = 200), (status = 422)))]
pub async fn create(State(s): State<AppState>, RequirePermission(u, _): Cm, Json(b): Json<CampaignInput>) -> ApiResult<Json<Value>> {
    let id = cp::create(&s.pool, u.user_id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.created", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name.trim() })).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/campaigns/{id}", responses((status = 200), (status = 404)))]
pub async fn detail(State(s): State<AppState>, _p: Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "campaign": cp::get(&s.pool, id).await? })))
}

#[utoipa::path(put, path = "/api/v1/admin/campaigns/{id}", request_body = CampaignInput, responses((status = 200), (status = 409)))]
pub async fn update(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>, Json(b): Json<CampaignInput>) -> ApiResult<Json<Value>> {
    cp::update(&s.pool, id, &b).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.updated", Some(&id.to_string()), u.ip.as_deref(), json!({ "name": b.name.trim() })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/campaigns/{id}", responses((status = 200), (status = 409)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    cp::delete(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.deleted", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/campaigns/{id}/send", responses((status = 200), (status = 409)))]
pub async fn send(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let n = cp::queue(&s.pool, id).await?;
    enqueue(&s, id).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.queued", Some(&id.to_string()), u.ip.as_deref(), json!({ "recipients": n })).await?;
    Ok(Json(json!({ "recipients": n })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct ScheduleBody {
    /// RFC 3339 time in the future.
    pub send_at: String,
}

/// Hold a draft until `send_at`; the worker snapshots recipients then.
#[utoipa::path(post, path = "/api/v1/admin/campaigns/{id}/schedule", request_body = ScheduleBody, responses((status = 200), (status = 409), (status = 422)))]
pub async fn schedule(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>, Json(b): Json<ScheduleBody>) -> ApiResult<Json<Value>> {
    let at = time::OffsetDateTime::parse(&b.send_at, &time::format_description::well_known::Rfc3339)
        .map_err(|_| AppError::Validation("error.schedule_in_past"))?;
    if cp::count_segment(&s.pool, &serde_json::from_value(cp::get(&s.pool, id).await?.segment).unwrap_or_default()).await? == 0 {
        return Err(AppError::Conflict("error.campaign_empty").into());
    }
    cp::schedule(&s.pool, id, at).await?;
    pelty_database::jobs::enqueue(&s.pool, JOB_KIND, json!({ "id": id, "scheduled": true }), Some(at), None, 5).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.scheduled", Some(&id.to_string()), u.ip.as_deref(), json!({ "send_at": b.send_at })).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(delete, path = "/api/v1/admin/campaigns/{id}/schedule", responses((status = 200), (status = 409)))]
pub async fn unschedule(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    cp::unschedule(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.unscheduled", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/campaigns/{id}/cancel", responses((status = 200), (status = 409)))]
pub async fn cancel(State(s): State<AppState>, RequirePermission(u, _): Cm, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    cp::cancel(&s.pool, id).await?;
    audit::record(&s.pool, Some(u.user_id), "campaign.cancelled", Some(&id.to_string()), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/client/marketing", responses((status = 200)))]
pub async fn my_pref(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    Ok(Json(json!({ "opt_out": cp::opt_out(&s.pool, c).await? })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct PrefBody {
    pub opt_out: bool,
}

#[utoipa::path(put, path = "/api/v1/client/marketing", request_body = PrefBody, responses((status = 200)))]
pub async fn set_pref(State(s): State<AppState>, u: CurrentUser, Json(b): Json<PrefBody>) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    cp::set_opt_out(&s.pool, c, b.opt_out).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Lookup only: link scanners prefetching the URL never unsubscribe anyone.
#[utoipa::path(get, path = "/api/v1/unsubscribe/{token}", responses((status = 200), (status = 404)))]
pub async fn unsub_lookup(State(s): State<AppState>, Path(token): Path<Uuid>) -> ApiResult<Json<Value>> {
    let e = cp::unsubscribe(&s.pool, token, false).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "email": e })))
}

/// No CSRF/session: the unguessable token is the credential.
#[utoipa::path(post, path = "/api/v1/unsubscribe/{token}", responses((status = 200), (status = 404)))]
pub async fn unsub_apply(State(s): State<AppState>, Path(token): Path<Uuid>) -> ApiResult<Json<Value>> {
    let e = cp::unsubscribe(&s.pool, token, true).await?.ok_or(AppError::NotFound)?;
    Ok(Json(json!({ "email": e, "unsubscribed": true })))
}

async fn enqueue(s: &AppState, id: Uuid) -> pelty_core::AppResult<()> {
    pelty_database::jobs::enqueue(&s.pool, JOB_KIND, json!({ "id": id }), None, None, 5).await?;
    Ok(())
}

/// Worker: send one batch; re-enqueue while recipients are pending. Each
/// recipient is marked sent/failed once, so a retried job never double-sends
/// an already-recorded recipient.
pub async fn run_job(s: &AppState, payload: &Value) -> Result<Value, String> {
    let id: Uuid = payload["id"].as_str().and_then(|v| v.parse().ok()).ok_or("missing id")?;
    if payload["scheduled"] == true {
        // A stale job (unscheduled, rescheduled later, cancelled) does nothing.
        let c = cp::get(&s.pool, id).await.map_err(|e| e.to_string())?;
        let due = c.scheduled_at.is_some_and(|t| t <= time::OffsetDateTime::now_utc());
        if c.status != "scheduled" || !due {
            return Ok(json!({ "skipped": true }));
        }
        match cp::queue(&s.pool, id).await {
            Ok(_) => {}
            Err(AppError::Conflict(_)) => {
                cp::cancel(&s.pool, id).await.map_err(|e| e.to_string())?;
                return Ok(json!({ "skipped": true, "empty": true }));
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    let Some((c, batch)) = cp::next_batch(&s.pool, id, BATCH).await.map_err(|e| e.to_string())? else {
        return Ok(json!({ "skipped": true }));
    };
    let m = s.active_mailer();
    let mut sent = 0;
    for r in &batch {
        let locale = r.locale.clone().filter(|l| s.i18n.has(l)).unwrap_or_else(|| s.cfg.default_locale.clone());
        let url = format!("{}/unsubscribe/{}", s.cfg.public_url, r.unsub_token);
        let v = crate::mail::vars(&[
            ("first_name", &r.first_name),
            ("last_name", &r.last_name),
            ("name", format!("{} {}", r.first_name, r.last_name).trim()),
            ("email", &r.email),
            ("company_name", &r.company_name),
            ("city", &r.city),
            ("country", &r.country),
            ("company", s.i18n.t(&locale, "app.name")),
            ("unsubscribe_url", &url),
        ]);
        let footer = s.i18n.t(&locale, "campaign.footer");
        let text = format!("{}\n\n--\n{footer}\n{url}", render(&c.body, &v, false));
        let email = pelty_notifications::Email {
            to: r.email.clone(),
            subject: render(&c.subject, &v, false).replace(['\r', '\n'], " "),
            html: text_to_html(&text),
            text,
            list_unsubscribe: Some(format!("{}/api/v1/unsubscribe/{}", s.cfg.public_url, r.unsub_token)),
        };
        let res = m.send(&email).await;
        cp::mark(&s.pool, id, r.customer_id, res.as_ref().err().map(String::as_str)).await.map_err(|e| e.to_string())?;
        if res.is_ok() {
            sent += 1;
        }
    }
    if cp::finish_if_done(&s.pool, id).await.map_err(|e| e.to_string())? {
        return Ok(json!({ "sent": sent, "done": true }));
    }
    enqueue(s, id).await.map_err(|e| e.to_string())?;
    Ok(json!({ "sent": sent, "done": false }))
}
