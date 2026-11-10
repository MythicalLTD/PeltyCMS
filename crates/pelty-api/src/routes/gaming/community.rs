//! Server community (module `server_community`): live server status, vote
//! sites with verified HTTP vote callbacks (rewards go through the game
//! delivery queue), monthly top voters and donors, and a monthly goal.
use super::query;
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::community::{self as cm, VoteSite, VoteSiteInput};
use pelty_database::{audit, game_delivery as gd, games};
use pelty_extensions::events::DomainEvent;
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Value};
use std::net::SocketAddr;
use uuid::Uuid;

pub const JOB_KIND: &str = "community.status";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/community", get(public_overview))
        .route(
            "/public/community/vote/{site}",
            get(vote_callback).post(vote_callback),
        )
        .route("/admin/community", get(admin_overview))
        .route("/admin/community/refresh", post(admin_refresh))
        .route("/admin/community/sites", post(create_site))
        .route(
            "/admin/community/sites/{id}",
            put(update_site).delete(delete_site),
        )
        .route("/admin/community/sites/{id}/token", post(site_token))
}

/// The monthly goal (`null` when no goal is set). Shared with the storefront.
pub async fn goal(pool: &sqlx::PgPool, cfg: &cm::Config) -> pelty_core::AppResult<Option<Value>> {
    Ok(if cfg.goal_amount > Decimal::ZERO {
        let raised = cm::month_total(pool, &cfg.goal_currency).await?;
        let pct = (raised * Decimal::from(100) / cfg.goal_amount)
            .round()
            .min(Decimal::from(100));
        Some(json!({
            "label": cfg.goal_label, "amount": cfg.goal_amount.to_string(), "currency": cfg.goal_currency,
            "raised": raised.round_dp(2).to_string(), "percent": pct.to_string().parse::<i64>().unwrap_or(0),
        }))
    } else {
        None
    })
}

/// Public page data: server status, vote links, leaderboards and the goal.
#[utoipa::path(get, path = "/api/v1/public/community", responses((status = 200), (status = 404)))]
pub async fn public_overview(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = cm::config(&s.pool).await?;
    let sites: Vec<Value> = cm::sites(&s.pool, true)
        .await?
        .into_iter()
        .map(|v| json!({ "id": v.id, "name": v.name, "url": v.url }))
        .collect();
    let donors = if cfg.show_donors {
        let list = cm::top_donors(&s.pool, &cfg.goal_currency, cfg.leaderboard_size).await?;
        Some(
            list.into_iter()
                .map(|d| {
                    if cfg.show_donor_amounts {
                        json!({ "name": d.name, "total": d.total.to_string(), "currency": cfg.goal_currency })
                    } else {
                        json!({ "name": d.name })
                    }
                })
                .collect::<Vec<_>>(),
        )
    } else {
        None
    };
    let goal = goal(&s.pool, &cfg).await?;
    Ok(Json(json!({
        "servers": cm::statuses(&s.pool).await?,
        "sites": sites,
        "top_voters": cm::top_voters(&s.pool, cfg.leaderboard_size).await?,
        "top_donors": donors,
        "goal": goal,
    })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct VoteParams {
    /// Player name (`username`, `user` or `player`).
    #[serde(alias = "user", alias = "player", alias = "name")]
    pub username: Option<String>,
    /// Site token (`token`, `key` or `secret`); may also be sent as `X-Vote-Token`.
    #[serde(alias = "key", alias = "secret")]
    pub token: Option<String>,
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Vote callback for a vote site. Verifies the site token, applies the
/// cooldown and queues the reward command on the game servers.
#[utoipa::path(method(get, post), path = "/api/v1/public/community/vote/{site}", params(("site" = Uuid, Path), VoteParams), responses((status = 200), (status = 403), (status = 404), (status = 409), (status = 422), (status = 429)))]
pub async fn vote_callback(
    State(s): State<AppState>,
    Path(site_id): Path<Uuid>,
    ext: axum::http::Extensions,
    headers: axum::http::HeaderMap,
    Query(p): Query<VoteParams>,
) -> ApiResult<Json<Value>> {
    let ip = ext
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_default();
    let token = p
        .token
        .clone()
        .or_else(|| {
            headers
                .get("x-vote-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
        .unwrap_or_default();
    let stored = cm::token_hash(&s.pool, site_id).await?;
    let ok = stored
        .as_deref()
        .is_some_and(|h| !token.is_empty() && same(h, &pelty_auth::token::hash(&token)));
    if !ok {
        if !s.auth_limiter.check(&format!("vote:{ip}")) {
            return Err(AppError::RateLimited.into());
        }
        return Err(AppError::Forbidden.into());
    }
    let username = p.username.unwrap_or_default();
    if !cm::username_ok(username.trim()) {
        return Err(AppError::Validation("error.vote_username").into());
    }
    let site = cm::site(&s.pool, site_id).await?;
    let Some((vote, customer)) = cm::record_vote(&s.pool, &site, &username, &ip).await? else {
        return Err(AppError::Conflict("error.vote_cooldown").into());
    };
    let rewards = match reward(&s, &site, vote, username.trim(), customer).await {
        Ok(n) => n,
        Err(e) => match cm::delete_vote(&s.pool, vote).await? {
            // Nothing queued: undo the vote so the site's retry is not
            // refused by the cooldown.
            None => return Err(e),
            // Part of the reward is queued: keep the vote and answer OK, as a
            // retry would queue the reward again under a new vote id.
            Some(n) => {
                tracing::warn!(%vote, error = ?e.0, queued = n, "vote reward partly queued");
                super::delivery::kick(&s);
                usize::try_from(n).unwrap_or(0)
            }
        },
    };
    s.emit(DomainEvent::new(
        "community.voted",
        json!({ "vote_id": vote, "site_id": site.id, "username": username.trim(), "customer_id": customer, "rewards": rewards }),
    ));
    Ok(Json(json!({ "accepted": true, "rewards": rewards })))
}

async fn reward(
    s: &AppState,
    site: &VoteSite,
    vote: Uuid,
    username: &str,
    customer: Option<Uuid>,
) -> ApiResult<usize> {
    if site.reward_command.is_empty()
        || !pelty_database::feature_modules::is_enabled(&s.pool, super::delivery::MODULE).await?
    {
        return Ok(0);
    }
    let n = gd::enqueue_vote(
        &s.pool,
        &gd::VoteReward {
            vote_id: vote,
            site: &site.name,
            username,
            customer,
            template: &site.reward_command,
            server: site.server_id,
            require_online: site.require_online,
        },
    )
    .await?;
    cm::set_rewarded(&s.pool, vote, n as i32).await?;
    if n > 0 {
        super::delivery::kick(s);
    }
    Ok(n)
}

// ------------------------------------------------------------------ status

/// Query every enabled server with a host and store the result.
pub async fn refresh(s: &AppState) -> pelty_core::AppResult<Value> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, cm::MODULE).await? {
        return Ok(json!({ "skipped": "module disabled" }));
    }
    let mut set = tokio::task::JoinSet::new();
    for (id, game, host, port, qport) in cm::targets(&s.pool).await? {
        let Some(def) = games::game(&game) else {
            continue;
        };
        let proto = def.query;
        if proto == "none" {
            continue;
        }
        let port = qport.or(port).map_or(def.default_port, |p| p as u16);
        set.spawn(async move { (id, query::query(proto, &host, port).await) });
    }
    let (mut online, mut offline) = (0, 0);
    while let Some(r) = set.join_next().await {
        let Ok((id, r)) = r else { continue };
        match &r {
            Ok(st) => {
                online += 1;
                cm::record_status(
                    &s.pool,
                    id,
                    Ok(cm::Observed {
                        players: st.players,
                        max_players: st.max_players,
                        version: &st.version,
                        motd: &st.motd,
                        sample: &st.sample,
                        latency_ms: st.latency_ms,
                    }),
                )
                .await?;
            }
            Err(e) => {
                offline += 1;
                cm::record_status(&s.pool, id, Err(e)).await?;
            }
        }
    }
    Ok(json!({ "online": online, "offline": offline }))
}

/// Scheduler: a status refresh each minute while the module is on.
pub async fn schedule(s: &AppState) -> pelty_core::AppResult<()> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, cm::MODULE).await? {
        return Ok(());
    }
    let minute = time::OffsetDateTime::now_utc().unix_timestamp() / 60;
    pelty_database::jobs::enqueue(
        &s.pool,
        JOB_KIND,
        json!({}),
        None,
        Some(&format!("{JOB_KIND}:{minute}")),
        1,
    )
    .await?;
    Ok(())
}

// ------------------------------------------------------------------ admin

#[utoipa::path(get, path = "/api/v1/admin/community", responses((status = 200), (status = 403)))]
pub async fn admin_overview(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    let errors: serde_json::Map<String, Value> = cm::errors(&s.pool)
        .await?
        .into_iter()
        .map(|(id, e)| (id.to_string(), json!(e)))
        .collect();
    Ok(Json(json!({
        "sites": cm::sites(&s.pool, false).await?,
        "votes": cm::recent_votes(&s.pool, 100).await?,
        "servers": cm::statuses(&s.pool).await?,
        "errors": errors,
    })))
}

#[utoipa::path(post, path = "/api/v1/admin/community/refresh", responses((status = 200), (status = 403)))]
pub async fn admin_refresh(
    State(s): State<AppState>,
    _p: RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(refresh(&s).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/community/sites", request_body = VoteSiteInput, responses((status = 200, body = VoteSite), (status = 403), (status = 422)))]
pub async fn create_site(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Json(b): Json<VoteSiteInput>,
) -> ApiResult<Json<VoteSite>> {
    let v = cm::save_site(&s.pool, None, &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vote_site.created",
        Some(&v.id.to_string()),
        u.ip.as_deref(),
        json!({ "name": v.name }),
    )
    .await?;
    Ok(Json(v))
}

#[utoipa::path(put, path = "/api/v1/admin/community/sites/{id}", request_body = VoteSiteInput, responses((status = 200, body = VoteSite), (status = 403), (status = 404), (status = 422)))]
pub async fn update_site(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<VoteSiteInput>,
) -> ApiResult<Json<VoteSite>> {
    let v = cm::save_site(&s.pool, Some(id), &b).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vote_site.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": v.name }),
    )
    .await?;
    Ok(Json(v))
}

#[utoipa::path(delete, path = "/api/v1/admin/community/sites/{id}", responses((status = 204), (status = 403), (status = 404)))]
pub async fn delete_site(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    cm::delete_site(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vote_site.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

/// New callback token for a vote site (shown once; replaces the old one).
#[utoipa::path(post, path = "/api/v1/admin/community/sites/{id}/token", responses((status = 200), (status = 403), (status = 404)))]
pub async fn site_token(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let raw = format!("pgv_{}", pelty_auth::token::generate());
    cm::set_token(&s.pool, id, &pelty_auth::token::hash(&raw)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "vote_site.token",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(
        json!({ "token": raw, "callback": format!("/api/v1/public/community/vote/{id}") }),
    ))
}
