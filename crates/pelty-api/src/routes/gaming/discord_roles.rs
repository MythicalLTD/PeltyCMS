//! Discord roles (module `discord_roles`): give buyers Discord server roles
//! for what they bought and take them back on expiry, refund or chargeback.
//! The bot token is entered by an admin and stored sealed (write-only).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::discord_roles::{self as dr, DiscordRoleRule, DiscordRoleRuleInput};
use pelty_database::game_delivery as gd;
use pelty_database::{audit, feature_modules};
use pelty_extensions::events::DomainEvent;
use pelty_payments::http::HttpRequestSpec;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const MODULE: &str = dr::MODULE;
pub const JOB_KIND: &str = "discord.roles";
const DISCORD_API: &str = "https://discord.com/api/v10";
/// Settings field carrying the bot token (never echoed back).
const TOKEN_FIELD: &str = "bot_token";

type Perm = RequirePermission<perms::SettingsManage>;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/discord-roles", get(admin_get).put(admin_put))
        .route(
            "/admin/discord-roles/rules/{product_id}",
            put(rule_put).delete(rule_delete),
        )
        .route("/admin/discord-roles/jobs", get(jobs_list))
        .route("/admin/discord-roles/jobs/{id}/retry", post(job_retry))
        .route("/admin/discord-roles/run", post(run_now))
}

// ---- events ----

/// Event bus hook: purchase/renewal add roles; expiry, refund and chargeback remove them.
pub async fn on_event(s: &AppState, e: &DomainEvent) {
    if let Err(err) = handle_event(s, e).await {
        tracing::warn!(event = e.name, error = %err, "discord roles hook failed");
    }
}

async fn handle_event(s: &AppState, e: &DomainEvent) -> pelty_core::AppResult<()> {
    let id = |k: &str| e.payload[k].as_str().and_then(|v| v.parse::<Uuid>().ok());
    // (action, services, occurrence, is_expiry)
    let (action, services, occurrence, expiry): (&str, Vec<Uuid>, String, bool) = match e.name {
        "service.created" => (
            "add",
            id("service_id").into_iter().collect(),
            "initial".into(),
            false,
        ),
        "service.unsuspended" => (
            "add",
            id("service_id").into_iter().collect(),
            format!("unsuspend:{}", Uuid::new_v4()),
            false,
        ),
        "invoice.paid" => match id("invoice_id") {
            Some(inv) => (
                "add",
                gd::renewed_services(&s.pool, inv).await?,
                format!("invoice:{inv}"),
                false,
            ),
            None => return Ok(()),
        },
        "service.suspended" | "service.terminated" | "service.cancelled" => (
            "remove",
            id("service_id").into_iter().collect(),
            format!("expiry:{}", Uuid::new_v4()),
            true,
        ),
        "order.refunded" => match id("order_id") {
            Some(o) => (
                "remove",
                gd::services_for(&s.pool, "order", o).await?,
                format!("order:{o}"),
                false,
            ),
            None => return Ok(()),
        },
        "invoice.refunded" => match id("invoice_id") {
            Some(i) => (
                "remove",
                gd::services_for(&s.pool, "invoice", i).await?,
                format!("invoice:{i}"),
                false,
            ),
            None => return Ok(()),
        },
        "payment.disputed" => match id("payment_id") {
            Some(p) => (
                "remove",
                gd::services_for(&s.pool, "payment", p).await?,
                format!("payment:{p}"),
                false,
            ),
            None => return Ok(()),
        },
        _ => return Ok(()),
    };
    if services.is_empty() || !feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(());
    }
    let mut n = 0;
    for sid in services {
        n += dr::enqueue(&s.pool, sid, action, &occurrence, expiry).await?;
    }
    if n > 0 {
        kick(s);
    }
    Ok(())
}

// ---- worker ----

/// Run the queue in the background right away.
pub(crate) fn kick(s: &AppState) {
    let s = s.clone();
    tokio::spawn(async move {
        if let Err(e) = process(&s).await {
            tracing::warn!(error = %e, "discord roles run failed");
        }
    });
}

/// Scheduler: one job per minute while the module is on (retries, late links).
pub async fn schedule(s: &AppState) -> pelty_core::AppResult<()> {
    if !feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(());
    }
    let minute = time::OffsetDateTime::now_utc().unix_timestamp() / 60;
    pelty_database::jobs::enqueue(
        &s.pool,
        JOB_KIND,
        json!({}),
        None,
        Some(&format!("{JOB_KIND}:{minute}")),
        3,
    )
    .await?;
    Ok(())
}

/// What to do with one Discord API answer: (status, note, retry seconds).
fn outcome(code: u16, body: &str, attempts: i32) -> (&'static str, String, i64) {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["message"]
        .as_str()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect::<String>();
    match code {
        200..=299 => ("done", String::new(), 0),
        // 10007 Unknown Member: not in the server yet; try again later.
        404 if v["code"] == 10007 => ("pending", "not_in_server".into(), 3600),
        404 => ("failed", format!("HTTP 404: {msg}"), 0),
        401 => ("failed", "bot token rejected (HTTP 401)".into(), 0),
        403 => (
            "failed",
            format!("missing permission or role above the bot (HTTP 403): {msg}"),
            0,
        ),
        429 => {
            let wait = v["retry_after"].as_f64().unwrap_or(5.0).ceil() as i64;
            ("pending", "rate_limited".into(), wait.clamp(1, 600))
        }
        _ => (
            "pending",
            format!("HTTP {code}: {msg}"),
            60 * (1 << attempts.clamp(0, 6)),
        ),
    }
}

/// Apply due role changes. Without a guild or token nothing is sent and the
/// jobs stay queued.
pub async fn process(s: &AppState) -> pelty_core::AppResult<Value> {
    if !feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(json!({ "skipped": "module disabled" }));
    }
    let (guild, sealed) = dr::config(&s.pool).await?;
    let token = sealed.and_then(|b| crate::routes::games::open_secret(s, &b));
    let Some(token) = token.filter(|_| dr::is_snowflake(&guild)) else {
        return Ok(json!({ "skipped": "not configured" }));
    };
    let http = s.http();
    let (mut done, mut retry, mut other) = (0, 0, 0);
    for j in dr::claim(&s.pool, 50).await? {
        let Some(user) = j.discord_user.as_deref().filter(|u| dr::is_snowflake(u)) else {
            dr::finish(&s.pool, j.id, "waiting", "no_discord_link", 0).await?;
            other += 1;
            continue;
        };
        if j.action == "remove" && dr::still_granted(&s.pool, &j).await? {
            dr::finish(&s.pool, j.id, "skipped", "still_granted", 0).await?;
            other += 1;
            continue;
        }
        let req = HttpRequestSpec {
            method: if j.action == "add" { "PUT" } else { "DELETE" },
            url: format!(
                "{DISCORD_API}/guilds/{guild}/members/{user}/roles/{}",
                j.role_id
            ),
            headers: vec![
                ("Authorization".into(), format!("Bot {token}")),
                ("X-Audit-Log-Reason".into(), "PeltyCMS purchase".into()),
            ],
            body: String::new(),
        };
        let (status, note, wait) = match http.send(req).await {
            Ok((code, body)) => outcome(code, &body, j.attempts),
            Err(e) => (
                "pending",
                e.chars().take(200).collect(),
                60 * (1 << j.attempts.clamp(0, 6)),
            ),
        };
        dr::finish(&s.pool, j.id, status, &note, wait).await?;
        match status {
            "done" => done += 1,
            "pending" => retry += 1,
            _ => other += 1,
        }
    }
    Ok(json!({ "done": done, "retry": retry, "other": other }))
}

// ---- admin ----

#[utoipa::path(get, path = "/api/v1/admin/discord-roles", responses((status = 200), (status = 403), (status = 404)))]
pub async fn admin_get(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    view(&s).await
}

async fn view(s: &AppState) -> ApiResult<Json<Value>> {
    let (guild, sealed) = dr::config(&s.pool).await?;
    Ok(Json(json!({
        "guild_id": guild,
        "token_set": sealed.is_some(),
        "rules": dr::rules(&s.pool).await?,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscordConfigInput {
    #[serde(default)]
    pub guild_id: String,
    /// New bot token; omit to keep, empty string to remove.
    #[serde(default)]
    pub bot_token: Option<String>,
}

#[utoipa::path(put, path = "/api/v1/admin/discord-roles", request_body = DiscordConfigInput, responses((status = 200), (status = 409), (status = 422)))]
pub async fn admin_put(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Json(b): Json<DiscordConfigInput>,
) -> ApiResult<Json<Value>> {
    let token = match b.bot_token.as_deref().map(str::trim) {
        None => None,
        Some("") => Some(None),
        Some(t) if t.len() > 200 || t.chars().any(char::is_whitespace) => {
            return Err(AppError::Validation("error.discord_token_invalid").into())
        }
        Some(t) => {
            let sb = s
                .secret_box
                .as_ref()
                .ok_or(AppError::Conflict("error.encryption_key_missing"))?;
            Some(Some(sb.seal(t.as_bytes())))
        }
    };
    let changed_token = token.is_some();
    dr::save_config(&s.pool, &b.guild_id, token).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "discord_roles.config_updated",
        None,
        u.ip.as_deref(),
        json!({ "guild_id": b.guild_id.trim(), (TOKEN_FIELD): if changed_token { "changed" } else { "kept" } }),
    )
    .await?;
    view(&s).await
}

#[utoipa::path(put, path = "/api/v1/admin/discord-roles/rules/{product_id}", request_body = DiscordRoleRuleInput, params(("product_id" = Uuid, Path)), responses((status = 200), (status = 404), (status = 422)))]
pub async fn rule_put(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(product): Path<Uuid>,
    Json(i): Json<DiscordRoleRuleInput>,
) -> ApiResult<Json<Vec<DiscordRoleRule>>> {
    dr::set_rule(&s.pool, product, &i).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "discord_roles.rule_set",
        Some(&product.to_string()),
        u.ip.as_deref(),
        json!({ "role_ids": i.role_ids, "remove_on_expiry": i.remove_on_expiry }),
    )
    .await?;
    Ok(Json(dr::rules(&s.pool).await?))
}

#[utoipa::path(delete, path = "/api/v1/admin/discord-roles/rules/{product_id}", params(("product_id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn rule_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(product): Path<Uuid>,
) -> ApiResult<Json<Vec<DiscordRoleRule>>> {
    if !dr::delete_rule(&s.pool, product).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "discord_roles.rule_deleted",
        Some(&product.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(dr::rules(&s.pool).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/discord-roles/jobs", responses((status = 200), (status = 403)))]
pub async fn jobs_list(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "items": dr::jobs(&s.pool, 200).await? })))
}

#[utoipa::path(post, path = "/api/v1/admin/discord-roles/jobs/{id}/retry", params(("id" = Uuid, Path)), responses((status = 200), (status = 409)))]
pub async fn job_retry(
    State(s): State<AppState>,
    RequirePermission(u, _): Perm,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    dr::retry(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "discord_roles.job_retried",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    kick(&s);
    Ok(Json(json!({ "ok": true })))
}

/// Process due jobs now and report what happened.
#[utoipa::path(post, path = "/api/v1/admin/discord-roles/run", responses((status = 200), (status = 403)))]
pub async fn run_now(State(s): State<AppState>, _p: Perm) -> ApiResult<Json<Value>> {
    Ok(Json(process(&s).await?))
}

#[cfg(test)]
mod tests {
    use super::outcome;

    #[test]
    fn discord_answers() {
        assert_eq!(outcome(204, "", 1).0, "done");
        assert_eq!(
            outcome(404, r#"{"code":10007,"message":"Unknown Member"}"#, 1),
            ("pending", "not_in_server".into(), 3600)
        );
        assert_eq!(
            outcome(404, r#"{"code":10011,"message":"Unknown Role"}"#, 1).0,
            "failed"
        );
        assert_eq!(
            outcome(403, r#"{"message":"Missing Permissions"}"#, 1).0,
            "failed"
        );
        assert_eq!(
            outcome(429, r#"{"retry_after":2.5}"#, 1),
            ("pending", "rate_limited".into(), 3)
        );
        assert_eq!(outcome(502, "", 2), ("pending", "HTTP 502: ".into(), 240));
    }
}
