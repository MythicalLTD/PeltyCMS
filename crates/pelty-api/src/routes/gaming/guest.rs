//! Guest checkout (module `guest_checkout`): on stores that allow it, a
//! buyer pays with only a player name / game identifier and an email. The
//! order belongs to a shadow account keyed by the email; the buyer claims
//! it by setting a password through the emailed link. The email is required:
//! it receives the receipt, the invoices and the claim link.
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::routes::store::{maybe_customer, place_cart, CheckoutInput};
use crate::AppState;
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderMap};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::guest_checkout::{self as gc, Settings};
use pelty_database::{audit, games, storefront as sf};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// How long a claim link stays valid.
pub const CLAIM_TTL_DAYS: i64 = 30;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/store/guest-checkout", post(guest_checkout))
        .route("/store/guest-checkout/{cart_id}", get(availability))
}

/// Request context without a session: client IP, user agent, geo signals.
pub struct GuestCtx {
    ip: Option<String>,
    user_agent: Option<String>,
    signals: crate::trusted_proxy::RequestSignals,
}

impl FromRequestParts<AppState> for GuestCtx {
    type Rejection = crate::error::ApiError;
    async fn from_request_parts(parts: &mut Parts, _: &AppState) -> Result<Self, Self::Rejection> {
        Ok(GuestCtx {
            ip: crate::extract::client_ip(parts),
            user_agent: parts
                .headers
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.chars().take(300).collect()),
            signals: parts.extensions.get().cloned().unwrap_or_default(),
        })
    }
}

/// Identity fields a guest can enter for `game` (Discord is linked later).
fn guest_fields(game: &str) -> Vec<&'static str> {
    games::game(game)
        .map(|g| {
            g.fields
                .iter()
                .copied()
                .filter(|f| *f != "discord_id")
                .collect()
        })
        .unwrap_or_default()
}

/// Whether this cart can be bought as a guest, for which game and with
/// which identity fields.
#[utoipa::path(get, path = "/api/v1/store/guest-checkout/{cart_id}", responses((status = 200), (status = 404)))]
pub async fn availability(
    State(s): State<AppState>,
    Path(cart_id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let set = gc::enabled(&s.pool).await?.ok_or(AppError::NotFound)?;
    let cart = sf::get_cart(&s.pool, cart_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let ids: Vec<Uuid> = sf::parse_items(&cart.items)
        .iter()
        .map(|i| i.product_id)
        .collect();
    let game = if cart.order_id.is_none() && cart.customer_id.is_none() {
        gc::cart_game(&s.pool, &ids).await?
    } else {
        None
    };
    Ok(Json(json!({
        "available": game.is_some(),
        "game": game,
        "fields": game.as_deref().map(guest_fields).unwrap_or_default(),
        "verify_minecraft": set.verify_minecraft,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct GuestBody {
    pub cart_id: Uuid,
    /// Receipts, invoices and the claim link go here (required).
    pub email: String,
    /// Identity field → value, e.g. `{"username": "Notch"}` or
    /// `{"license": "license:..."}`. At least one field of the game.
    #[schema(value_type = Object)]
    pub identity: BTreeMap<String, String>,
    #[serde(default)]
    #[schema(value_type = Object)]
    pub fields: HashMap<usize, HashMap<Uuid, String>>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

/// Mojang lookup: canonical name and UUID of a Minecraft Java account.
async fn mojang(s: &AppState, name: &str) -> ApiResult<(String, String)> {
    if !gc::minecraft_name_ok(name) {
        return Err(AppError::Validation("error.game_identity_invalid").into());
    }
    let req = pelty_payments::http::HttpRequestSpec {
        method: "GET",
        url: format!("{}/users/profiles/minecraft/{name}", gc::MOJANG_API),
        headers: vec![("Accept".into(), "application/json".into())],
        body: String::new(),
    };
    match s.http().send(req).await {
        Ok((200, body)) => serde_json::from_str::<Value>(&body)
            .ok()
            .as_ref()
            .and_then(gc::parse_mojang)
            .ok_or(AppError::Conflict("error.guest_mc_lookup_failed").into()),
        Ok((204 | 404, _)) => Err(AppError::Validation("error.guest_mc_unknown").into()),
        Ok((code, _)) => {
            tracing::warn!(code, "mojang lookup failed");
            Err(AppError::Conflict("error.guest_mc_lookup_failed").into())
        }
        Err(e) => {
            tracing::warn!(error = %e, "mojang lookup failed");
            Err(AppError::Conflict("error.guest_mc_lookup_failed").into())
        }
    }
}

/// Validate the entered identity for `game`: every field must belong to the
/// game and pass its format check; Minecraft Java names are looked up.
async fn identity(
    s: &AppState,
    set: &Settings,
    game: &str,
    raw: &BTreeMap<String, String>,
) -> ApiResult<BTreeMap<String, String>> {
    let allowed = guest_fields(game);
    let mut out = BTreeMap::new();
    for (f, v) in raw {
        if v.trim().is_empty() {
            continue;
        }
        if !allowed.contains(&f.as_str()) {
            return Err(AppError::Validation("error.validation").into());
        }
        let v = games::normalize_identity(f, v)
            .ok_or(AppError::Validation("error.game_identity_invalid"))?;
        out.insert(f.clone(), v);
    }
    if game == "minecraft_java" {
        let name = out
            .get("username")
            .cloned()
            .ok_or(AppError::Validation("error.guest_identity_required"))?;
        if set.verify_minecraft {
            let (name, uuid) = mojang(s, &name).await?;
            out.insert("username".into(), name);
            out.insert("uuid".into(), uuid);
        } else if !gc::minecraft_name_ok(&name) {
            return Err(AppError::Validation("error.game_identity_invalid").into());
        }
    }
    if out.is_empty() {
        return Err(AppError::Validation("error.guest_identity_required").into());
    }
    Ok(out)
}

pub(crate) fn email_ok(e: &str) -> bool {
    let Some((local, host)) = e.rsplit_once('@') else {
        return false;
    };
    e.len() <= 254
        && !local.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !e.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Place a cart as a guest. A new email gets a shadow account and a
/// session to pay with; an unclaimed shadow account is reused without a
/// session (the order waits in the account, the buyer gets the link). An
/// email that belongs to a full account must sign in instead.
#[utoipa::path(post, path = "/api/v1/store/guest-checkout", request_body = GuestBody,
    responses((status = 200), (status = 404), (status = 409), (status = 422), (status = 429)))]
pub async fn guest_checkout(
    State(s): State<AppState>,
    g: GuestCtx,
    headers: HeaderMap,
    Json(b): Json<GuestBody>,
) -> ApiResult<Response> {
    let set = gc::enabled(&s.pool).await?.ok_or(AppError::NotFound)?;
    crate::captcha::require(&s, &headers, "checkout", g.ip.as_deref()).await?;
    if maybe_customer(&s, &headers).await.is_some() {
        return Err(AppError::Conflict("error.guest_signed_in").into());
    }
    let email = b.email.trim().to_string();
    if !email_ok(&email) {
        return Err(AppError::Validation("error.invalid_email").into());
    }
    let (per_ip, per_email) =
        gc::hit(&s.pool, g.ip.as_deref().unwrap_or("unknown"), &email).await?;
    if per_ip > set.ip_per_hour || per_email > set.email_per_hour {
        return Err(AppError::RateLimited.into());
    }
    if pelty_database::banned_emails::is_banned(&s.pool, &email).await? {
        return Err(AppError::Validation("error.email_banned").into());
    }
    let cart = sf::get_cart(&s.pool, b.cart_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if cart.order_id.is_some() {
        return Err(AppError::Conflict("error.cart_closed").into());
    }
    if cart.customer_id.is_some() {
        // Saved by a signed-in customer: not a guest cart.
        return Err(AppError::NotFound.into());
    }
    let items = sf::parse_items(&cart.items);
    if items.is_empty() {
        return Err(AppError::Validation("error.cart_invalid").into());
    }
    let ids: Vec<Uuid> = items.iter().map(|i| i.product_id).collect();
    let game = gc::cart_game(&s.pool, &ids)
        .await?
        .ok_or(AppError::Validation("error.guest_not_available"))?;
    // A full account must sign in (checked before any Mojang lookup).
    let existing = gc::account(&s.pool, &email).await?;
    if matches!(existing, Some((_, false))) {
        return Err(AppError::Conflict("error.guest_account_exists").into());
    }
    let ident = identity(&s, &set, &game, &b.identity).await?;
    let names: Vec<String> = ident.get("username").cloned().into_iter().collect();
    if pelty_database::store_rules::banned_guest(&s.pool, &email, &names).await? {
        return Err(AppError::Validation("error.store_banned").into());
    }

    // Shadow account keyed by the email.
    let (user_id, created) = match existing {
        Some((_, false)) => return Err(AppError::Conflict("error.guest_account_exists").into()),
        Some((uid, true)) => (uid, false),
        None => {
            // Never revealed or mailed; replaced when the buyer claims it.
            let throwaway = format!("Aa1!{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
            let display = ident
                .get("username")
                .cloned()
                .unwrap_or_else(|| email.split('@').next().unwrap_or("").to_string());
            let user = s
                .auth
                .register(
                    pelty_auth::RegisterInput {
                        email: &email,
                        password: &throwaway,
                        display_name: &display,
                    },
                    "customer",
                    g.ip.as_deref(),
                )
                .await?;
            gc::mark_shadow(&s.pool, user.id).await?;
            (user.id, true)
        }
    };
    let account = pelty_database::users::find(&s.pool, user_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let cid = s.clients().id_for_user(user_id).await?;
    // A reused shadow account keeps its player: a different one is refused so
    // nobody can repoint someone else's deliveries.
    let have = games::identity(&s.pool, cid, &game).await?;
    if ident
        .iter()
        .any(|(f, v)| have.get(f).is_some_and(|h| !h.eq_ignore_ascii_case(v)))
    {
        return Err(AppError::Conflict("error.guest_identity_mismatch").into());
    }
    for (f, v) in &ident {
        if !have.contains_key(f) {
            games::link_account(
                &s.pool,
                cid,
                &games::AccountInput {
                    game: game.clone(),
                    field: f.clone(),
                    value: v.clone(),
                },
            )
            .await?;
        }
    }

    // Only a brand-new shadow account gets a session (to pay right away).
    let to = account.email.to_string();
    let issued = if created {
        Some(
            s.auth
                .issue_session(account, g.ip.as_deref(), g.user_agent.as_deref())
                .await?,
        )
    } else {
        None
    };
    let u = CurrentUser {
        user_id,
        session_id: issued.as_ref().map(|i| i.session.id).unwrap_or_default(),
        csrf_token: issued
            .as_ref()
            .map(|i| i.csrf_token.clone())
            .unwrap_or_default(),
        roles: vec!["customer".into()],
        permissions: vec![],
        ip: g.ip.clone(),
        api_key: false,
        signals: g.signals.clone(),
    };
    let input = CheckoutInput {
        fields: b.fields,
        country: b.country,
    };
    let placed = match place_cart(&s, &u, cid, &cart, items, input).await {
        Ok(v) => v,
        Err(e) => {
            if let Some(i) = &issued {
                let _ = s
                    .auth
                    .revoke(user_id, i.session.id, g.ip.as_deref(), "auth.logout")
                    .await;
            }
            return Err(e);
        }
    };
    let order_id = placed["order"]["id"].clone();
    let locale = b
        .locale
        .filter(|l| s.i18n.has(l))
        .unwrap_or_else(|| s.cfg.default_locale.clone());
    send_claim(&s, user_id, &to, &locale, &ident, &placed).await?;
    audit::record(
        &s.pool,
        Some(user_id),
        "store.guest_checkout",
        order_id.as_str(),
        g.ip.as_deref(),
        json!({ "customer_id": cid, "game": game, "new_account": created }),
    )
    .await?;
    s.emit(pelty_extensions::events::DomainEvent::new(
        "store.guest_checkout",
        json!({ "customer_id": cid, "order_id": order_id, "game": game, "new_account": created }),
    ));
    let body = json!({
        "order": { "id": placed["order"]["id"], "number": placed["order"]["number"] },
        "invoice": { "id": placed["invoice"]["id"], "status": placed["invoice"]["status"], "total": placed["invoice"]["total"], "currency": placed["invoice"]["currency"] },
        "session": issued.is_some(),
        "csrf_token": issued.as_ref().map(|i| i.csrf_token.clone()),
    });
    let mut r = Json(body).into_response();
    if let Some(i) = &issued {
        let max_age = s.cfg.session_absolute_hours * 3600;
        r.headers_mut().append(
            header::SET_COOKIE,
            crate::routes::auth::cookie_header(
                &s,
                crate::extract::SESSION_COOKIE,
                &i.token,
                max_age,
                true,
            ),
        );
    }
    Ok(r)
}

/// Mail the claim link (a one-time set-password link) with the order.
async fn send_claim(
    s: &AppState,
    user: Uuid,
    email: &str,
    locale: &str,
    ident: &BTreeMap<String, String>,
    placed: &Value,
) -> ApiResult<()> {
    use pelty_auth::token;
    let tok = token::generate();
    pelty_database::user_tokens::issue(
        &s.pool,
        user,
        pelty_database::user_tokens::PASSWORD_RESET,
        &token::hash(&tok),
        time::Duration::days(CLAIM_TTL_DAYS),
    )
    .await?;
    let link = format!("{}/reset-password?token={tok}", s.cfg.public_url);
    let player = ident
        .get("username")
        .or_else(|| ident.values().next())
        .cloned()
        .unwrap_or_default();
    let order = placed["order"]["number"]
        .as_str()
        .map(String::from)
        .or_else(|| placed["order"]["number"].as_i64().map(|n| n.to_string()))
        .unwrap_or_default();
    let days = CLAIM_TTL_DAYS.to_string();
    crate::mail::enqueue(
        s,
        "guest_claim",
        locale,
        email,
        crate::mail::vars(&[
            ("player", &player),
            ("order", &order),
            ("link", &link),
            ("expires_days", &days),
        ]),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emails_and_fields() {
        assert!(email_ok("a@b.co"));
        for bad in ["", "a", "a@b", "@b.co", "a b@c.de", "a@.co", "a@co."] {
            assert!(!email_ok(bad), "{bad}");
        }
        assert_eq!(guest_fields("minecraft_java"), vec!["username", "uuid"]);
        assert_eq!(
            guest_fields("fivem"),
            vec!["username", "license", "steamid"]
        );
        assert!(guest_fields("nope").is_empty());
    }
}
