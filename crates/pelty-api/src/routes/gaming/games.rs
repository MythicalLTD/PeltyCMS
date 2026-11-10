//! Game profiles (module `game_profiles`): supported games, game servers
//! with a store each (`servers.manage`), and the game accounts a customer
//! links (Minecraft name/UUID, Steam ID, FiveM license, Discord ID).
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::games::{self, AccountInput, GameAccount, Server, ServerInput};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/game/games", get(list_games))
        .route("/public/game/servers", get(public_servers))
        .route("/public/game/servers/{slug}", get(public_server))
        .route("/client/game/accounts", get(my_accounts).post(link))
        .route("/client/game/accounts/{id}", delete(unlink))
        .route(
            "/admin/game/servers",
            get(admin_servers).post(create_server),
        )
        .route(
            "/admin/game/servers/{id}",
            get(admin_server).put(update_server).delete(delete_server),
        )
        .route("/admin/game/servers/{id}/products", put(set_products))
        .route("/admin/game/servers/{id}/agent-token", post(agent_token))
        .route(
            "/admin/game/customers/{id}/accounts",
            get(customer_accounts),
        )
}

/// Supported games and their identity fields.
#[utoipa::path(get, path = "/api/v1/public/game/games", responses((status = 200, body = Vec<games::GameDef>), (status = 404)))]
pub async fn list_games() -> Json<Value> {
    Json(json!({ "items": games::GAMES }))
}

#[utoipa::path(get, path = "/api/v1/public/game/servers", responses((status = 200), (status = 404)))]
pub async fn public_servers(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let items: Vec<Value> = games::servers(&s.pool, true)
        .await?
        .iter()
        .map(games::public_view)
        .collect();
    Ok(Json(json!({ "items": items })))
}

/// One server's store: public server fields and its active packages.
#[utoipa::path(get, path = "/api/v1/public/game/servers/{slug}", responses((status = 200), (status = 404)))]
pub async fn public_server(
    State(s): State<AppState>,
    Path(slug): Path<String>,
) -> ApiResult<Json<Value>> {
    let srv = games::server_by_slug(&s.pool, &slug).await?;
    let packages = games::packages(&s.pool, srv.id).await?;
    Ok(Json(
        json!({ "server": games::public_view(&srv), "packages": packages }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/game/accounts", responses((status = 200, body = Vec<GameAccount>), (status = 404)))]
pub async fn my_accounts(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    Ok(Json(json!({ "items": games::accounts(&s.pool, c).await? })))
}

/// Link (or replace) one identity field for a game.
#[utoipa::path(post, path = "/api/v1/client/game/accounts", request_body = AccountInput, responses((status = 200, body = GameAccount), (status = 422)))]
pub async fn link(
    State(s): State<AppState>,
    u: CurrentUser,
    Json(b): Json<AccountInput>,
) -> ApiResult<Json<GameAccount>> {
    let c = customer_of(&s, &u).await?;
    let a = games::link_account(&s.pool, c, &b).await?;
    // Deliveries that waited for this identity can go now.
    if pelty_database::feature_modules::is_enabled(&s.pool, super::delivery::MODULE).await?
        && pelty_database::game_delivery::release_waiting(&s.pool, c).await? > 0
    {
        super::delivery::kick(&s);
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_account.linked",
        Some(&c.to_string()),
        u.ip.as_deref(),
        json!({ "game": a.game, "field": a.field }),
    )
    .await?;
    Ok(Json(a))
}

#[utoipa::path(delete, path = "/api/v1/client/game/accounts/{id}", responses((status = 200), (status = 404)))]
pub async fn unlink(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    games::unlink_account(&s.pool, c, id).await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/admin/game/servers", responses((status = 200, body = Vec<Server>), (status = 403)))]
pub async fn admin_servers(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": games::servers(&s.pool, false).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/game/servers/{id}", responses((status = 200, body = Server), (status = 404)))]
pub async fn admin_server(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Server>> {
    Ok(Json(games::server(&s.pool, id).await?))
}

fn seal_password(s: &AppState, b: &ServerInput) -> ApiResult<Option<Option<Vec<u8>>>> {
    Ok(match b.rcon_password.as_deref() {
        None => None,
        Some("") => Some(None),
        Some(p) => {
            let sb = s
                .secret_box
                .as_ref()
                .ok_or(AppError::Conflict("error.encryption_key_missing"))?;
            Some(Some(sb.seal(p.as_bytes())))
        }
    })
}

#[utoipa::path(post, path = "/api/v1/admin/game/servers", request_body = ServerInput, responses((status = 200, body = Server), (status = 409), (status = 422)))]
pub async fn create_server(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Json(b): Json<ServerInput>,
) -> ApiResult<Json<Server>> {
    let pw = seal_password(&s, &b)?;
    let srv = games::save_server(&s.pool, None, &b, pw).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_server.created",
        Some(&srv.id.to_string()),
        u.ip.as_deref(),
        json!({ "game": srv.game, "transport": srv.transport }),
    )
    .await?;
    Ok(Json(srv))
}

#[utoipa::path(put, path = "/api/v1/admin/game/servers/{id}", request_body = ServerInput, responses((status = 200, body = Server), (status = 404), (status = 422)))]
pub async fn update_server(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ServerInput>,
) -> ApiResult<Json<Server>> {
    let pw = seal_password(&s, &b)?;
    let srv = games::save_server(&s.pool, Some(id), &b, pw).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_server.updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "rcon_password_changed": b.rcon_password.is_some() }),
    )
    .await?;
    Ok(Json(srv))
}

#[utoipa::path(delete, path = "/api/v1/admin/game/servers/{id}", responses((status = 200), (status = 404)))]
pub async fn delete_server(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    games::delete_server(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_server.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ProductsBody {
    pub product_ids: Vec<Uuid>,
}

/// Replace the packages sold on a server's store (in display order).
#[utoipa::path(put, path = "/api/v1/admin/game/servers/{id}/products", request_body = ProductsBody, responses((status = 200, body = Server), (status = 404)))]
pub async fn set_products(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ProductsBody>,
) -> ApiResult<Json<Server>> {
    games::server(&s.pool, id).await?;
    Ok(Json(
        games::set_products(&s.pool, id, &b.product_ids).await?,
    ))
}

/// Issue a new agent token for the server plugin/resource (shown once;
/// replaces the previous one).
#[utoipa::path(post, path = "/api/v1/admin/game/servers/{id}/agent-token", responses((status = 200), (status = 404)))]
pub async fn agent_token(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::ServersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let raw = format!("pga_{}", pelty_auth::token::generate());
    games::set_agent_token(&s.pool, id, &pelty_auth::token::hash(&raw)).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "game_server.agent_token",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "token": raw })))
}

/// A customer's linked game accounts (staff view).
#[utoipa::path(get, path = "/api/v1/admin/game/customers/{id}/accounts", responses((status = 200, body = Vec<GameAccount>), (status = 403)))]
pub async fn customer_accounts(
    State(s): State<AppState>,
    RequirePermission(_u, _): RequirePermission<perms::CustomersManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": games::accounts(&s.pool, id).await? }),
    ))
}

/// Opens a sealed RCON password (base64 of the sealed bytes is never exposed).
pub fn open_secret(s: &AppState, sealed: &[u8]) -> Option<String> {
    let sb = s.secret_box.as_ref()?;
    String::from_utf8(sb.open(sealed)?).ok()
}
