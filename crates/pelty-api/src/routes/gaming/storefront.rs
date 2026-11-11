//! Gaming storefront (module `gaming_storefront`): the public layout with
//! resolved sidebar widgets, and its admin settings.
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use pelty_database::game_storefront::{self as sf, Config, Widget};
use pelty_database::{audit, community as cm, feature_modules, games as gp};
use serde_json::{json, Value};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/storefront", get(public_layout))
        .route("/admin/storefront", get(admin_get).put(admin_put))
}

/// Store layout and sidebar widgets with their live data.
#[utoipa::path(get, path = "/api/v1/public/storefront", responses((status = 200), (status = 404)))]
pub async fn public_layout(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let cfg = sf::config(&s.pool).await?;
    let community = feature_modules::is_enabled(&s.pool, cm::MODULE).await?;
    let mut widgets = Vec::with_capacity(cfg.widgets.len());
    // Looked up once, and only when a top-customer widget needs it.
    let mut default_currency: Option<String> = None;
    for w in &cfg.widgets {
        let v = match w {
            Widget::Text { title, body } => json!({ "type": "text", "title": title, "body": body }),
            Widget::TopCustomer {
                period,
                show_amount,
            } => {
                let currency = match &default_currency {
                    Some(c) => c.clone(),
                    None => {
                        let c = sf::default_currency(&s.pool).await?;
                        default_currency = Some(c.clone());
                        c
                    }
                };
                let top = sf::top_customer(&s.pool, period, &currency).await?;
                json!({
                    "type": "top_customer", "period": period, "currency": currency,
                    "customer": top.map(|t| if *show_amount {
                        json!({ "name": t.name, "total": t.total.round_dp(2).to_string() })
                    } else {
                        json!({ "name": t.name })
                    }),
                })
            }
            Widget::RecentPayments { limit } => json!({
                "type": "recent_payments",
                "items": sf::recent_payments(&s.pool, *limit).await?,
            }),
            Widget::FeaturedPackage { product_id } => {
                match sf::featured(&s.pool, *product_id).await? {
                    Some(p) => json!({ "type": "featured_package", "product": p }),
                    None => continue,
                }
            }
            Widget::ServerStatus { server_id } => {
                if !community {
                    continue;
                }
                let servers: Vec<_> = cm::statuses(&s.pool)
                    .await?
                    .into_iter()
                    .filter(|x| server_id.is_none_or(|id| x.server_id == id))
                    .collect();
                json!({ "type": "server_status", "servers": servers })
            }
            Widget::Goal {} => {
                if !community {
                    continue;
                }
                let c = cm::config(&s.pool).await?;
                match super::community::goal(&s.pool, &c).await? {
                    Some(g) => json!({ "type": "goal", "goal": g }),
                    None => continue,
                }
            }
            Widget::Discord { invite_url, label } => {
                json!({ "type": "discord", "invite_url": invite_url, "label": label })
            }
        };
        widgets.push(v);
    }
    let (server, package_ids) = hero_server(&s, &cfg, community).await?;
    Ok(Json(json!({
        "theme": cfg.theme,
        "discord_url": cfg.discord_url,
        "logo_url": cfg.logo_url,
        "server": server,
        "package_ids": package_ids,
        "hero_title": cfg.hero_title,
        "hero_subtitle": cfg.hero_subtitle,
        "hero_image": cfg.hero_image,
        "accent": cfg.accent,
        "layout": cfg.layout,
        "sidebar": cfg.sidebar,
        "widgets": widgets,
        "announcement": cfg.announcement,
        "announcement_link": cfg.announcement_link,
        "announcement_tone": cfg.announcement_tone,
        "group_layouts": cfg.group_layouts,
    })))
}

/// The hero's server: the chosen one (or the first enabled), with the live
/// status when `server_community` checks it (`null` with no enabled server),
/// and the packages sold on enabled servers (the store shows only those when
/// any are linked; empty = the whole catalog).
async fn hero_server(
    s: &AppState,
    cfg: &sf::Config,
    community: bool,
) -> ApiResult<(Value, Vec<uuid::Uuid>)> {
    if !feature_modules::is_enabled(&s.pool, "game_profiles").await? {
        return Ok((Value::Null, Vec::new()));
    }
    let servers = gp::servers(&s.pool, true).await?;
    let mut package_ids: Vec<uuid::Uuid> = Vec::new();
    for id in servers.iter().flat_map(|x| x.product_ids.iter()) {
        if !package_ids.contains(id) {
            package_ids.push(*id);
        }
    }
    let Some(srv) = cfg
        .server_id
        .and_then(|id| servers.iter().find(|x| x.id == id))
        .or_else(|| servers.first())
    else {
        return Ok((Value::Null, package_ids));
    };
    let status = if community {
        cm::statuses(&s.pool)
            .await?
            .into_iter()
            .find(|x| x.server_id == srv.id)
            .map(|x| json!({ "online": x.online, "players": x.players, "max_players": x.max_players }))
    } else {
        None
    };
    // Minecraft Java clients add 25565 themselves; other games need the port.
    let port = srv.port.or_else(|| {
        gp::game(&srv.game)
            .filter(|g| g.id != "minecraft_java")
            .map(|g| i32::from(g.default_port))
    });
    let address = match port {
        Some(p) if !srv.host.is_empty() => format!("{}:{p}", srv.host),
        _ => srv.host.clone(),
    };
    Ok((
        json!({
            "name": srv.name, "slug": srv.slug, "game": srv.game, "address": address, "status": status,
        }),
        package_ids,
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/storefront", responses((status = 200, body = Config), (status = 403)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Config>> {
    Ok(Json(sf::config(&s.pool).await?))
}

#[utoipa::path(put, path = "/api/v1/admin/storefront", request_body = Config, responses((status = 200, body = Config), (status = 403), (status = 422)))]
pub async fn admin_put(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(c): Json<Config>,
) -> ApiResult<Json<Config>> {
    sf::save(&s.pool, &c, u.user_id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "storefront.updated",
        None,
        u.ip.as_deref(),
        json!({ "widgets": c.widgets.len(), "accent": c.accent, "layout": c.layout, "theme": c.theme }),
    )
    .await?;
    Ok(Json(c))
}
