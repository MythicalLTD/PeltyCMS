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
            "/client/game/checkout-identity/{cart_id}",
            get(checkout_identity),
        )
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
#[utoipa::path(get, path = "/api/v1/public/game/games", responses((status = 200, body = super::Items<games::GameDef>), (status = 404)))]
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

#[utoipa::path(get, path = "/api/v1/client/game/accounts", responses((status = 200, body = super::Items<GameAccount>), (status = 404)))]
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

/// Player details a cart needs at checkout, per game: every identity field
/// with the value already on the buyer's game account (filled by Login with
/// FiveM / Steam / Discord or an earlier purchase) and whether the
/// package's delivery commands need it. Missing required fields are asked.
#[utoipa::path(get, path = "/api/v1/client/game/checkout-identity/{cart_id}", responses((status = 200), (status = 404)))]
pub async fn checkout_identity(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(cart_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let cart = pelty_database::storefront::get_cart(&s.pool, cart_id)
        .await?
        .filter(|k| k.order_id.is_none() && k.customer_id.is_none_or(|x| x == c))
        .ok_or(AppError::NotFound)?;
    let items = pelty_database::storefront::parse_items(&cart.items);
    Ok(Json(
        json!({ "games": identity_view(&s, c, &items).await? }),
    ))
}

/// `[{game, name, fields: [{field, value, required}]}]` for a cart.
pub(crate) async fn identity_view(
    s: &AppState,
    customer: Uuid,
    items: &[pelty_database::orders::CartItem],
) -> ApiResult<Vec<Value>> {
    let needs = cart_needs(s, items).await?;
    let mut out = vec![];
    for (g, req) in needs {
        let Some(def) = games::game(&g) else { continue };
        let have = games::identity(&s.pool, customer, &g).await?;
        let fields: Vec<Value> = def
            .fields
            .iter()
            .map(|f| json!({ "field": f, "value": have.get(*f), "required": req.contains(*f) }))
            .collect();
        out.push(json!({ "game": g, "name": def.name, "fields": fields }));
    }
    Ok(out)
}

/// Identity needs of a cart's packages (empty when game profiles are off).
pub(crate) async fn cart_needs(
    s: &AppState,
    items: &[pelty_database::orders::CartItem],
) -> ApiResult<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, "game_profiles").await? {
        return Ok(Default::default());
    }
    let lines: Vec<(Uuid, bool)> = items
        .iter()
        .map(|i| {
            (
                i.product_id,
                i.gift_to.as_deref().is_some_and(|g| !g.trim().is_empty()),
            )
        })
        .collect();
    let mut needs = games::cart_needs(&s.pool, &lines).await?;
    // Commands only run with delivery on: without it nothing is required.
    if !pelty_database::feature_modules::is_enabled(&s.pool, super::delivery::MODULE).await? {
        needs.values_mut().for_each(|r| r.clear());
    }
    Ok(needs)
}

/// Signed-in checkout: save the player details the buyer typed (only into
/// empty fields; a linked value is never replaced from here), then refuse
/// when a field the package's delivery needs is still missing.
pub(crate) async fn apply_checkout_identity(
    s: &AppState,
    u: &CurrentUser,
    customer: Uuid,
    items: &[pelty_database::orders::CartItem],
    typed: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
) -> ApiResult<()> {
    let needs = cart_needs(s, items).await?;
    if typed.keys().any(|g| !needs.contains_key(g)) {
        return Err(AppError::Validation("error.validation").into());
    }
    let mut saved = false;
    for (g, req) in &needs {
        let def = games::game(g).ok_or(AppError::Validation("error.validation"))?;
        let have = games::identity(&s.pool, customer, g).await?;
        for (f, v) in typed.get(g).into_iter().flatten() {
            if v.trim().is_empty() {
                continue;
            }
            if !def.fields.contains(&f.as_str()) {
                return Err(AppError::Validation("error.validation").into());
            }
            let n = games::normalize_identity(f, v)
                .ok_or(AppError::Validation("error.game_identity_invalid"))?;
            if have.contains_key(f) {
                continue;
            }
            games::fill_missing(&s.pool, customer, &[g.as_str()], f, &n).await?;
            audit::record(
                &s.pool,
                Some(u.user_id),
                "game_account.linked",
                Some(&customer.to_string()),
                u.ip.as_deref(),
                json!({ "game": g, "field": f, "via": "checkout" }),
            )
            .await?;
            saved = true;
        }
        if !req.is_empty() {
            let now = games::identity(&s.pool, customer, g).await?;
            if req.iter().any(|f| !now.contains_key(f)) {
                return Err(AppError::Validation("error.game_identity_required").into());
            }
        }
    }
    if saved
        && pelty_database::feature_modules::is_enabled(&s.pool, super::delivery::MODULE).await?
        && pelty_database::game_delivery::release_waiting(&s.pool, customer).await? > 0
    {
        super::delivery::kick(s);
    }
    Ok(())
}

/// How an identity value reads in a checkout field: FiveM identifiers keep
/// their `license:` / `fivem:` prefix.
pub(crate) fn identity_display(field: &str, v: &str) -> String {
    match field {
        "license" | "fivem" if !v.starts_with(&format!("{field}:")) => format!("{field}:{v}"),
        _ => v.to_string(),
    }
}

/// Package checkout fields mapped to a player identity field: an empty one
/// is filled from the buyer's game account (games of the stores selling the
/// package); a typed value is saved to the account when it had none.
pub(crate) async fn fill_identity_fields(
    s: &AppState,
    customer: Uuid,
    items: &mut [pelty_database::orders::CartItem],
) -> ApiResult<()> {
    if !pelty_database::feature_modules::is_enabled(&s.pool, "game_profiles").await? {
        return Ok(());
    }
    for it in items.iter_mut() {
        let keyed: Vec<(Uuid, String)> = sqlx::query_as(
            "SELECT id, identity_key FROM product_custom_fields WHERE product_id = $1 AND identity_key IS NOT NULL AND NOT admin_only",
        )
        .bind(it.product_id)
        .fetch_all(&s.pool)
        .await
        .map_err(pelty_database::db_err)?;
        if keyed.is_empty() {
            continue;
        }
        let gift = it.gift_to.as_deref().is_some_and(|g| !g.trim().is_empty());
        let mut store_games: Vec<String> = vec![];
        for sid in games::servers_for_product(&s.pool, it.product_id).await? {
            let g = games::server(&s.pool, sid).await?.game;
            if !store_games.contains(&g) {
                store_games.push(g);
            }
        }
        let refs: Vec<&str> = store_games.iter().map(String::as_str).collect();
        for (fid, key) in keyed {
            let typed = it
                .fields
                .get(&fid)
                .map(|v| v.trim().to_string())
                .unwrap_or_default();
            if typed.is_empty() {
                if gift {
                    continue;
                }
                for g in &refs {
                    if let Some(v) = games::identity(&s.pool, customer, g).await?.get(&key) {
                        it.fields.insert(fid, identity_display(&key, v));
                        break;
                    }
                }
            } else if !gift {
                games::fill_missing(&s.pool, customer, &refs, &key, &typed).await?;
            }
        }
    }
    Ok(())
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

#[utoipa::path(get, path = "/api/v1/admin/game/servers", responses((status = 200, body = super::Items<Server>), (status = 403)))]
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
#[utoipa::path(get, path = "/api/v1/admin/game/customers/{id}/accounts", responses((status = 200, body = super::Items<GameAccount>), (status = 403)))]
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
