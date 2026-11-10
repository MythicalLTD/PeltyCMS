//! In-game delivery (module `game_delivery`): package commands, the
//! delivery queue (`servers.manage`), the client's delivery log, the agent
//! API game-server plugins poll, and the event hook + worker that send
//! commands over RCON.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::game_delivery::{self as gd, CommandInput, Delivery, Filter};
use pelty_database::games;
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub const JOB_KIND: &str = "game.delivery";
pub const MODULE: &str = "game_delivery";

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/game/delivery/products/{id}/commands",
            get(product_commands).put(put_product_commands),
        )
        .route(
            "/admin/game/delivery/global-commands",
            get(global_commands).put(put_global_commands),
        )
        .route("/admin/game/delivery/queue", get(queue))
        .route("/admin/game/delivery/queue/{id}/retry", post(retry))
        .route("/admin/game/delivery/queue/{id}/cancel", post(cancel))
        .route("/admin/game/delivery/services/{id}/resend", post(resend))
        .route("/admin/game/delivery/servers/{id}/test", post(test_server))
        .route("/admin/game/delivery/run", post(run_now))
        .route("/client/game/deliveries", get(my_deliveries))
        .route("/game-agent/poll", post(agent_poll))
        .route("/game-agent/ack", post(agent_ack))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CommandsBody {
    pub commands: Vec<CommandInput>,
}

#[utoipa::path(get, path = "/api/v1/admin/game/delivery/products/{id}/commands", responses((status = 200, body = super::Items<gd::Command>)))]
pub async fn product_commands(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": gd::commands(&s.pool, Some(id)).await?, "placeholders": gd::PLACEHOLDERS, "triggers": gd::TRIGGERS }),
    ))
}

/// Replace a package's commands (per trigger, optional server, require online, delay).
#[utoipa::path(put, path = "/api/v1/admin/game/delivery/products/{id}/commands", request_body = CommandsBody, responses((status = 200), (status = 422)))]
pub async fn put_product_commands(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<CommandsBody>,
) -> ApiResult<Json<Value>> {
    let items = gd::replace_commands(&s.pool, Some(id), &b.commands).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_commands.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "count": items.len() }),
    )
    .await?;
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(get, path = "/api/v1/admin/game/delivery/global-commands", responses((status = 200, body = super::Items<gd::Command>)))]
pub async fn global_commands(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": gd::commands(&s.pool, None).await?, "placeholders": gd::PLACEHOLDERS, "triggers": gd::TRIGGERS }),
    ))
}

/// Global commands run for every package (e.g. a broadcast on any purchase).
#[utoipa::path(put, path = "/api/v1/admin/game/delivery/global-commands", request_body = CommandsBody, responses((status = 200), (status = 422)))]
pub async fn put_global_commands(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Json(b): Json<CommandsBody>,
) -> ApiResult<Json<Value>> {
    let items = gd::replace_commands(&s.pool, None, &b.commands).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_commands.updated",
        Some("global"),
        u.ip.as_deref(),
        json!({ "count": items.len() }),
    )
    .await?;
    Ok(Json(json!({ "items": items })))
}

/// Delivery log / queue, newest first (filter by status, server, customer, service).
#[utoipa::path(get, path = "/api/v1/admin/game/delivery/queue", params(Filter), responses((status = 200, body = super::Items<Delivery>)))]
pub async fn queue(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
    Query(f): Query<Filter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": gd::list(&s.pool, &f, 200).await?, "stats": gd::stats(&s.pool).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/game/delivery/queue/{id}/retry", responses((status = 200), (status = 409)))]
pub async fn retry(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    gd::retry(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_delivery.retried",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    kick(&s);
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(post, path = "/api/v1/admin/game/delivery/queue/{id}/cancel", responses((status = 200), (status = 409)))]
pub async fn cancel(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    gd::cancel(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_delivery.cancelled",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResendBody {
    #[serde(default = "purchase")]
    pub trigger: String,
}
fn purchase() -> String {
    "purchase".into()
}

/// Queue a service's commands again (a new occurrence, like Tebex "resend").
#[utoipa::path(post, path = "/api/v1/admin/game/delivery/services/{id}/resend", request_body = ResendBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn resend(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ResendBody>,
) -> ApiResult<Json<Value>> {
    if !gd::TRIGGERS.contains(&b.trigger.as_str()) {
        return Err(AppError::Validation("error.validation").into());
    }
    let ctx = gd::service_ctx(&s.pool, id)
        .await?
        .ok_or(AppError::NotFound)?;
    let n = gd::enqueue(
        &s.pool,
        &ctx,
        &b.trigger,
        &format!("resend:{}", Uuid::new_v4()),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_delivery.resent",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "trigger": b.trigger, "queued": n }),
    )
    .await?;
    kick(&s);
    Ok(Json(json!({ "queued": n })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TestBody {
    #[serde(default = "list")]
    pub command: String,
}
fn list() -> String {
    "list".into()
}

/// Connect over RCON and run one command (connection test).
#[utoipa::path(post, path = "/api/v1/admin/game/delivery/servers/{id}/test", request_body = TestBody, responses((status = 200), (status = 409)))]
pub async fn test_server(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TestBody>,
) -> ApiResult<Json<Value>> {
    let srv = games::server(&s.pool, id).await?;
    if srv.transport != "rcon" {
        return Err(AppError::Conflict("error.game_not_rcon").into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_server.rcon_test",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(match rcon_exec(&s, &srv, &[b.command]).await {
        Ok(out) => json!({ "ok": true, "response": out.first().cloned().unwrap_or_default() }),
        Err(e) => json!({ "ok": false, "error": e }),
    }))
}

/// Process the RCON queue now (otherwise the worker does it every tick).
#[utoipa::path(post, path = "/api/v1/admin/game/delivery/run", responses((status = 200)))]
pub async fn run_now(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(process_rcon(&s).await?))
}

/// The client's deliveries (status per server and command).
#[utoipa::path(get, path = "/api/v1/client/game/deliveries", responses((status = 200, body = super::Items<Delivery>)))]
pub async fn my_deliveries(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let f = Filter {
        customer_id: Some(c),
        ..Default::default()
    };
    let items: Vec<Value> = gd::list(&s.pool, &f, 100)
        .await?
        .into_iter()
        .map(|d| json!({
            "id": d.id, "server_name": d.server_name, "package": d.package, "trigger": d.trigger,
            "player": d.player, "status": d.status, "created_at": d.created_at, "delivered_at": d.delivered_at,
        }))
        .collect();
    Ok(Json(json!({ "items": items })))
}

async fn agent_server(s: &AppState, h: &HeaderMap) -> ApiResult<games::Server> {
    let tok = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|t| t.starts_with("pga_"))
        .ok_or(AppError::Unauthenticated)?;
    let srv = games::server_for_agent(&s.pool, &pelty_auth::token::hash(tok))
        .await?
        .ok_or(AppError::Unauthenticated)?;
    if srv.transport != "agent" {
        return Err(AppError::Conflict("error.game_not_agent").into());
    }
    Ok(srv)
}

#[derive(Debug, Default, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PollBody {
    /// Identifiers of players online now (names, UUIDs, Steam IDs, licenses).
    /// Commands marked "require online" are only handed out for these players.
    #[serde(default)]
    pub online: Option<Vec<String>>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// Agent API: claim due commands for this server (`Authorization: Bearer pga_...`).
/// Each command is leased for 2 minutes; acknowledge it with `/game-agent/ack`.
/// Agent poll answer: the server's slug and the commands to run.
#[derive(Debug, serde::Serialize, utoipa::ToSchema)]
pub struct AgentPoll {
    pub server: String,
    pub commands: Vec<gd::Claimed>,
}

#[utoipa::path(post, path = "/api/v1/game-agent/poll", request_body = PollBody, responses((status = 200, body = AgentPoll), (status = 401)))]
pub async fn agent_poll(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(b): Json<PollBody>,
) -> ApiResult<Json<Value>> {
    let srv = agent_server(&s, &h).await?;
    let online = b
        .online
        .as_ref()
        .map(|o| o.iter().take(1000).cloned().collect::<Vec<_>>());
    let items = gd::claim_agent(
        &s.pool,
        srv.id,
        online.as_deref(),
        b.limit.unwrap_or(50).clamp(1, 200),
    )
    .await?;
    Ok(Json(json!({ "server": srv.slug, "commands": items })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct AckFailure {
    pub id: Uuid,
    pub error: String,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AckBody {
    #[serde(default)]
    pub delivered: Vec<Uuid>,
    #[serde(default)]
    pub failed: Vec<AckFailure>,
}

/// Agent API: report executed (and failed) commands.
#[utoipa::path(post, path = "/api/v1/game-agent/ack", request_body = AckBody, responses((status = 200), (status = 401)))]
pub async fn agent_ack(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(b): Json<AckBody>,
) -> ApiResult<Json<Value>> {
    let srv = agent_server(&s, &h).await?;
    let (mut ok, mut failed) = (0, 0);
    for id in b.delivered.iter().take(500) {
        ok += gd::mark_delivered(&s.pool, *id, Some(srv.id), "").await? as usize;
    }
    for f in b.failed.iter().take(500) {
        failed += gd::mark_retry(&s.pool, f.id, Some(srv.id), &f.error).await? as usize;
    }
    Ok(Json(json!({ "delivered": ok, "failed": failed })))
}

async fn rcon_exec(
    s: &AppState,
    srv: &games::Server,
    cmds: &[String],
) -> Result<Vec<String>, String> {
    let host = if srv.rcon_host.is_empty() {
        &srv.host
    } else {
        &srv.rcon_host
    };
    let port = srv.rcon_port.unwrap_or(25575) as u16;
    let sealed = games::rcon_secret(&s.pool, srv.id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("rcon password not set")?;
    let pw =
        crate::routes::games::open_secret(s, &sealed).ok_or("rcon password cannot be decrypted")?;
    let mut c = super::rcon::Rcon::connect(host, port, &pw).await?;
    let mut out = vec![];
    for cmd in cmds {
        out.push(c.exec(cmd).await?);
    }
    Ok(out)
}

/// Send due RCON deliveries, grouped per server (one connection each).
pub async fn process_rcon(s: &AppState) -> pelty_core::AppResult<Value> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(json!({ "skipped": "module disabled" }));
    }
    gd::sweep_exhausted(&s.pool).await?;
    let claimed = gd::claim_rcon(&s.pool, 200).await?;
    let mut by_server: std::collections::BTreeMap<Uuid, Vec<gd::Claimed>> = Default::default();
    for c in claimed {
        by_server.entry(c.server_id).or_default().push(c);
    }
    let (mut ok, mut failed) = (0, 0);
    for (sid, items) in by_server {
        let srv = match games::server(&s.pool, sid).await {
            Ok(srv) => srv,
            Err(_) => continue,
        };
        let host = if srv.rcon_host.is_empty() {
            srv.host.clone()
        } else {
            srv.rcon_host.clone()
        };
        let conn = async {
            let sealed = games::rcon_secret(&s.pool, sid)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("rcon password not set")?;
            let pw = crate::routes::games::open_secret(s, &sealed)
                .ok_or("rcon password cannot be decrypted")?;
            super::rcon::Rcon::connect(&host, srv.rcon_port.unwrap_or(25575) as u16, &pw).await
        }
        .await;
        match conn {
            Ok(mut c) => {
                // Players online, read once when a command needs it.
                let mut online: Option<String> = None;
                let mut broken = false;
                for d in items {
                    if broken {
                        // The connection is unusable (a late reply could be
                        // matched to the next command): retry later, free.
                        gd::release(&s.pool, d.id, 30, "").await?;
                        continue;
                    }
                    if d.require_online {
                        if online.is_none() {
                            match c.exec(online_command(&srv.game)).await {
                                Ok(r) => online = Some(r),
                                Err(e) => {
                                    gd::release(&s.pool, d.id, 30, &e).await?;
                                    broken = true;
                                    continue;
                                }
                            }
                        }
                        if !player_listed(online.as_deref().unwrap_or(""), &d.player) {
                            gd::release(&s.pool, d.id, 60, "waiting for the player to be online")
                                .await?;
                            continue;
                        }
                    }
                    match c.exec(&d.command).await {
                        Ok(resp) => {
                            gd::mark_delivered(&s.pool, d.id, None, &resp).await?;
                            ok += 1;
                        }
                        Err(e) => {
                            gd::mark_retry(&s.pool, d.id, None, &e).await?;
                            failed += 1;
                            broken = true;
                        }
                    }
                }
            }
            Err(e) => {
                for d in items {
                    gd::mark_retry(&s.pool, d.id, None, &e).await?;
                    failed += 1;
                }
            }
        }
    }
    Ok(json!({ "delivered": ok, "failed": failed }))
}

/// RCON command that lists online players for a game.
fn online_command(game: &str) -> &'static str {
    if game.starts_with("minecraft") {
        "list"
    } else {
        "status"
    }
}

/// Whole-word, case-insensitive match of `player` in a player list reply.
pub(crate) fn player_listed(reply: &str, player: &str) -> bool {
    let p = player.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    reply
        .to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.' || c == '-'))
        .any(|w| w == p)
}

/// Run the RCON queue in the background right away.
pub(crate) fn kick(s: &AppState) {
    let s = s.clone();
    tokio::spawn(async move {
        if let Err(e) = process_rcon(&s).await {
            tracing::warn!(error = %e, "game delivery run failed");
        }
    });
}

/// Scheduler: one job per tick while the module is on.
pub async fn schedule(s: &AppState) -> pelty_core::AppResult<()> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, MODULE).await? {
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

/// Event bus hook: purchase, renewal, expiry, refund and chargeback.
pub async fn on_event(s: &AppState, e: &DomainEvent) {
    if let Err(err) = handle_event(s, e).await {
        tracing::warn!(event = e.name, error = %err, "game delivery hook failed");
    }
}

async fn handle_event(s: &AppState, e: &DomainEvent) -> pelty_core::AppResult<()> {
    let id = |k: &str| e.payload[k].as_str().and_then(|v| v.parse::<Uuid>().ok());
    let (trigger, services, occurrence): (&str, Vec<Uuid>, String) = match e.name {
        "service.created" => (
            "purchase",
            id("service_id").into_iter().collect(),
            "initial".into(),
        ),
        "service.unsuspended" => (
            "renewal",
            id("service_id").into_iter().collect(),
            format!("unsuspend:{}", Uuid::new_v4()),
        ),
        "service.suspended" | "service.terminated" | "service.cancelled" => (
            "expiry",
            id("service_id").into_iter().collect(),
            format!("expiry:{}", time::OffsetDateTime::now_utc().date()),
        ),
        "invoice.paid" => match id("invoice_id") {
            Some(inv) => (
                "renewal",
                gd::renewed_services(&s.pool, inv).await?,
                format!("invoice:{inv}"),
            ),
            None => return Ok(()),
        },
        "order.refunded" => match id("order_id") {
            Some(o) => (
                "refund",
                gd::services_for(&s.pool, "order", o).await?,
                format!("order:{o}"),
            ),
            None => return Ok(()),
        },
        "invoice.refunded" => match id("invoice_id") {
            Some(i) => (
                "refund",
                gd::services_for(&s.pool, "invoice", i).await?,
                format!("invoice:{i}"),
            ),
            None => return Ok(()),
        },
        "payment.disputed" => match id("payment_id") {
            Some(p) => (
                "chargeback",
                gd::services_for(&s.pool, "payment", p).await?,
                format!("payment:{p}"),
            ),
            None => return Ok(()),
        },
        _ => return Ok(()),
    };
    if services.is_empty() || !pelty_database::feature_modules::is_enabled(&s.pool, MODULE).await? {
        return Ok(());
    }
    let mut n = 0;
    for sid in services {
        if let Some(ctx) = gd::service_ctx(&s.pool, sid).await? {
            n += gd::enqueue(&s.pool, &ctx, trigger, &occurrence).await?;
        }
    }
    if n > 0 {
        kick(s);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::player_listed;

    #[test]
    fn online_list_matching() {
        let mc = "There are 2 of a max of 20 players online: Steve, alex_99";
        assert!(player_listed(mc, "steve"));
        assert!(player_listed(mc, "Alex_99"));
        assert!(!player_listed(mc, "alex"));
        assert!(!player_listed(mc, ""));
        assert!(!player_listed(
            "There are 0 of a max of 20 players online:",
            "Steve"
        ));
    }
}
