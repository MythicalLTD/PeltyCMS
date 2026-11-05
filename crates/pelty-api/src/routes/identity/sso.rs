//! Client SSO into the provisioned panel via a signed one-time token.
//! `POST /client/services/{id}/sso` returns the redirect URL;
//! `POST /sso/consume` lets the panel burn the token exactly once.
use crate::routes::customers::customer_of;
use crate::error::ApiResult;
use crate::extract::CurrentUser;
use crate::AppState;
use axum::extract::{Path, State};
use axum::routing::post;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::provisioning as pdb;
use pelty_database::{audit, sso as db};
use pelty_provisioning::sso::{self, Claims};
use pelty_provisioning::{ServerConn, ServiceCtx};
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;
use uuid::Uuid;

/// Token lifetime.
pub const TTL_SECS: i64 = 60;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/client/services/{id}/sso", post(start))
        .route("/sso/consume", post(consume))
}

/// Server connection + service context, or a stable error code.
async fn load(s: &AppState, sid: Uuid) -> ApiResult<(ServerConn, ServiceCtx, String)> {
    let unsupported = || AppError::Conflict("error.sso_unsupported");
    let t = pdb::target(&s.pool, sid).await?.ok_or(AppError::NotFound)?;
    let module = t.module.clone().ok_or_else(unsupported)?;
    let server_id = t.server_id.ok_or_else(unsupported)?;
    let (srv_module, base_url, sealed, config, enabled) = pdb::server_secret(&s.pool, server_id).await?.ok_or_else(unsupported)?;
    if srv_module != module || !enabled {
        return Err(unsupported().into());
    }
    let key = s.secret_box.as_ref().ok_or_else(unsupported)?;
    let api_key = key.open(&sealed).and_then(|k| String::from_utf8(k).ok()).ok_or_else(unsupported)?;
    let ctx = ServiceCtx {
        service_id: sid.to_string(),
        customer_email: t.email,
        customer_first_name: t.first_name,
        customer_last_name: t.last_name,
        product_config: t.product_config,
        custom_fields: t.custom_fields,
        remote_id: t.remote_id,
        module_data: t.module_data,
    };
    Ok((ServerConn { base_url, api_key, config }, ctx, module))
}

#[utoipa::path(post, path = "/api/v1/client/services/{id}/sso", responses((status = 200), (status = 404), (status = 409)))]
pub async fn start(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    let svc = s.hosting().get(id).await?.filter(|x| x.customer_id == c).ok_or(AppError::NotFound)?;
    if svc.status != "active" {
        return Err(AppError::Conflict("error.service_not_active").into());
    }
    let (conn, ctx, module) = load(&s, id).await?;
    let m = s.provisioning.get(&module).ok_or(AppError::Conflict("error.sso_unsupported"))?;
    if !crate::addons::provisioning_enabled(&s, &module).await? {
        return Err(AppError::Conflict("error.sso_unsupported").into());
    }
    let now = OffsetDateTime::now_utc();
    let claims = Claims { sid: id.to_string(), rid: ctx.remote_id.clone(), email: ctx.customer_email.clone(), exp: now.unix_timestamp() + TTL_SECS, nonce: pelty_auth::token::generate() };
    let token = sso::sign(&conn.api_key, &claims);
    let url = m
        .sso_url(&conn, &ctx, &token)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "sso_url failed");
            AppError::Conflict("error.sso_failed")
        })?
        .ok_or(AppError::Conflict("error.sso_unsupported"))?;
    db::issue(&s.pool, &claims.nonce, id, u.user_id, now + time::Duration::seconds(TTL_SECS)).await?;
    audit::record(&s.pool, Some(u.user_id), "service.sso", Some(&id.to_string()), u.ip.as_deref(), json!({ "module": module })).await?;
    Ok(Json(json!({ "url": url, "expires_in": TTL_SECS })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ConsumeBody {
    pub token: String,
}

/// Called by the panel. 200 with the claims the first time; 401 otherwise.
#[utoipa::path(post, path = "/api/v1/sso/consume", request_body = ConsumeBody, responses((status = 200), (status = 401)))]
pub async fn consume(State(s): State<AppState>, Json(b): Json<ConsumeBody>) -> ApiResult<Json<Value>> {
    let bad = || AppError::Unauthenticated;
    if b.token.len() > 4096 {
        return Err(bad().into());
    }
    let peek = sso::peek(&b.token).ok_or_else(bad)?;
    let sid: Uuid = peek.sid.parse().map_err(|_| bad())?;
    let (conn, _, _) = load(&s, sid).await.map_err(|_| bad())?;
    let c = sso::verify(&conn.api_key, &b.token, OffsetDateTime::now_utc().unix_timestamp()).ok_or_else(bad)?;
    if !db::consume(&s.pool, &c.nonce, sid).await? {
        return Err(bad().into());
    }
    Ok(Json(json!({ "service_id": c.sid, "remote_id": c.rid, "email": c.email })))
}
