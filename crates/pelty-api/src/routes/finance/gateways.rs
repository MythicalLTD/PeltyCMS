//! Payment gateway administration (enable/disable, settings rendered from
//! each gateway's schema, custom HTTP gateways, test-webhook tool, delivery
//! log) and the client-side gateway picker / return-time completion.
use crate::routes::customers::customer_of;
use crate::routes::webhooks::{apply_event, kind_str, resolve_config};
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::extensions::GatewayRow;
use pelty_database::gateway_admin::{self, NewDelivery};
use pelty_payments::{CompleteStatus, GatewayDescriptor, GatewayError, WebhookEvent, WebhookKind, COMMON_SETTINGS, CUSTOM_PREFIX};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/gateways", get(list_gateways))
        .route("/admin/gateways/custom", post(create_custom))
        .route("/admin/gateways/deliveries", get(list_deliveries))
        .route("/admin/gateways/{id}", put(update_gateway).delete(delete_custom))
        .route("/admin/gateways/{id}/test-webhook", post(test_webhook))
        .route("/client/invoices/{id}/gateways", get(client_invoice_gateways))
        .route("/client/payments/{id}/complete", post(client_complete))
}

fn env_name_ok(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= 64 && b[0].is_ascii_uppercase() && b.iter().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

fn secret_status(d: &GatewayDescriptor, row: Option<&GatewayRow>) -> Value {
    let refs = row.map(|r| r.secret_refs.clone()).unwrap_or(json!({}));
    let mut names: Vec<String> = d.secrets.iter().map(|s| s.to_string()).collect();
    if let Some(m) = refs.as_object() {
        for k in m.keys() {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
    }
    names
        .into_iter()
        .map(|k| {
            let env = refs.get(&k).and_then(|v| v.as_str()).map(str::to_string);
            let set = env.as_deref().and_then(|e| std::env::var(e).ok()).is_some_and(|v| !v.is_empty());
            (k, json!({ "env": env, "configured": set }))
        })
        .collect::<Map<_, _>>()
        .into()
}

fn item(d: &GatewayDescriptor, id: &str, row: Option<&GatewayRow>) -> Value {
    let settings: Vec<_> = COMMON_SETTINGS.iter().chain(d.settings.iter()).collect();
    json!({
        "id": id, "kind": d.id, "name": d.name, "capabilities": d.capabilities,
        "custom": id.starts_with(CUSTOM_PREFIX),
        "enabled": row.is_some_and(|r| r.enabled),
        "config": row.map(|r| r.config.clone()).unwrap_or(json!({})),
        "secrets": secret_status(d, row),
        "secret_names": d.secrets,
        "settings": settings,
        "currencies": d.currencies,
        "webhook_path": format!("/api/v1/webhooks/{id}"),
    })
}

/// Built-in gateways plus operator-defined custom ones; never secret values.
#[utoipa::path(get, path = "/api/v1/admin/gateways", responses((status = 200), (status = 403)))]
pub async fn list_gateways(State(s): State<AppState>, _p: RequirePermission<perms::GatewaysManage>) -> ApiResult<Json<Value>> {
    let rows = pelty_database::extensions::gateways(&s.pool).await?;
    let mut items: Vec<Value> = s
        .gateways
        .descriptors()
        .into_iter()
        .filter(|d| d.id != pelty_payments::CUSTOM_ID)
        .map(|d| item(&d, d.id, rows.iter().find(|r| r.id == d.id)))
        .collect();
    for r in rows.iter().filter(|r| r.id.starts_with(CUSTOM_PREFIX)) {
        if let Some(d) = s.gateways.descriptor(&r.id) {
            items.push(item(&d, &r.id, Some(r)));
        }
    }
    Ok(Json(json!({ "items": items })))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct UpdateGateway {
    pub enabled: bool,
    #[schema(value_type = Object)]
    pub config: Value,
    /// Secret name → environment variable NAME (never the secret value).
    #[schema(value_type = Object)]
    #[serde(default)]
    pub secret_refs: Value,
}

fn validate_config(gw: &pelty_payments::GatewayRegistry, d: &GatewayDescriptor, id: &str, cfg: &Value) -> Result<(), AppError> {
    let m = cfg.as_object().ok_or(AppError::Validation("error.gateway_config"))?;
    let schema: Vec<_> = COMMON_SETTINGS.iter().chain(d.settings.iter()).collect();
    for (k, v) in m {
        let f = schema.iter().find(|f| f.key == k).ok_or(AppError::Validation("error.gateway_config"))?;
        let ok = match f.kind {
            "bool" => v.is_boolean(),
            "json" => v.is_object() || v.is_null(),
            "number" => v.is_null() || v.as_str().is_some_and(|x| x.is_empty() || Decimal::from_str(x).is_ok_and(|d| d >= Decimal::ZERO)),
            "url" => v.as_str().is_some_and(|x| x.is_empty() || pelty_payments::http::api_base(&json!({"api_base": x}), "").is_ok()),
            "select" => v.as_str().is_some_and(|x| f.options.contains(&x)),
            _ => v.is_string() && v.as_str().is_some_and(|x| x.len() <= 4000),
        };
        if !ok {
            return Err(AppError::Validation("error.gateway_config"));
        }
    }
    for f in schema.iter().filter(|f| f.required) {
        if m.get(f.key).is_none_or(|v| v.is_null() || v.as_str() == Some("")) {
            return Err(AppError::Validation("error.gateway_config"));
        }
    }
    if let Some(c) = m.get("currencies").and_then(|v| v.as_str()) {
        if !c.split(',').map(str::trim).filter(|x| !x.is_empty()).all(|x| pelty_core::money::Currency::parse(x).is_some()) {
            return Err(AppError::Validation("error.invalid_currency"));
        }
    }
    if id.starts_with(CUSTOM_PREFIX) {
        gw.build(id, &pelty_payments::GatewayConfig { id: id.to_string(), config: cfg.clone(), ..Default::default() }).map_err(|_| AppError::Validation("error.gateway_config"))?;
    }
    Ok(())
}

#[utoipa::path(put, path = "/api/v1/admin/gateways/{id}", request_body = UpdateGateway, responses((status = 204), (status = 404), (status = 422)))]
pub async fn update_gateway(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GatewaysManage>,
    Path(id): Path<String>,
    Json(b): Json<UpdateGateway>,
) -> ApiResult<StatusCode> {
    let Some(d) = s.gateways.descriptor(&id) else {
        if crate::addons::provided(&s, "gateway", &id) {
            return Err(AppError::Validation("error.addon_not_installed").into());
        }
        return Err(AppError::NotFound.into());
    };
    let custom = id.starts_with(CUSTOM_PREFIX);
    if custom && pelty_database::extensions::gateway(&s.pool, &id).await?.is_none() {
        return Err(AppError::NotFound.into());
    }
    validate_config(&s.gateways, &d, &id, &b.config)?;
    let refs = if b.secret_refs.is_null() { json!({}) } else { b.secret_refs.clone() };
    let rm = refs.as_object().ok_or(AppError::Validation("error.gateway_secret_ref"))?;
    for (k, v) in rm {
        let name_ok = if custom {
            !k.is_empty() && k.len() <= 64 && k.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        } else {
            d.secrets.contains(&k.as_str())
        };
        if !name_ok || !v.as_str().is_some_and(env_name_ok) {
            return Err(AppError::Validation("error.gateway_secret_ref").into());
        }
    }
    if b.enabled && !crate::addons::gateway_enabled(&s, &id).await? {
        return Err(AppError::Validation("error.addon_not_installed").into());
    }
    if b.enabled {
        // Must build with the given config (secrets resolved from env).
        let row = GatewayRow { id: id.clone(), enabled: true, config: b.config.clone(), secret_refs: refs.clone() };
        if let Err(e) = s.gateways.build(&id, &resolve_config(&row)) {
            tracing::info!(gateway = %id, error = %e, "gateway not ready");
            return Err(AppError::Validation("error.gateway_not_ready").into());
        }
    }
    gateway_admin::upsert(&s.pool, &id, b.enabled, &b.config, &refs).await?;
    pelty_database::audit::record(&s.pool, Some(u.user_id), "gateway.updated", Some(&id), u.ip.as_deref(),
        json!({ "enabled": b.enabled, "config_keys": b.config.as_object().map(|m| m.keys().cloned().collect::<Vec<_>>()) })).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct NewCustom {
    /// Slug; the gateway id becomes `custom-<slug>`.
    pub slug: String,
    pub name: String,
}

#[utoipa::path(post, path = "/api/v1/admin/gateways/custom", request_body = NewCustom, responses((status = 200), (status = 409), (status = 422)))]
pub async fn create_custom(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GatewaysManage>,
    Json(b): Json<NewCustom>,
) -> ApiResult<Json<Value>> {
    let slug = b.slug.trim().to_ascii_lowercase();
    let name = b.name.trim();
    if slug.is_empty() || slug.len() > 40 || !slug.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-') || name.is_empty() || name.len() > 80 {
        return Err(AppError::Validation("error.gateway_config").into());
    }
    let id = format!("{CUSTOM_PREFIX}{slug}");
    if pelty_database::extensions::gateway(&s.pool, &id).await?.is_some() {
        return Err(AppError::Conflict("error.gateway_exists").into());
    }
    // Disabled skeleton; the operator fills in the templates, then enables.
    let cfg = json!({
        "name": name,
        "create": {"method": "POST", "url": "https://api.example.com/payments", "headers": {"Authorization": "Bearer {{secret.api_key}}"},
                   "body": "{\"amount\":\"{{amount}}\",\"currency\":\"{{currency}}\",\"reference\":\"{{payment_id}}\",\"return_url\":\"{{success_url}}\",\"notify_url\":\"{{webhook_url}}\"}",
                   "redirect_url_path": "checkout_url", "ref_path": "id"},
        "webhook": {"signature_header": "X-Signature", "encoding": "hex", "secret": "webhook_secret",
                    "event_id_path": "id", "event_type_path": "type", "ref_path": "data.id",
                    "success_types": ["payment.succeeded"], "failed_types": ["payment.failed"], "refunded_types": [], "disputed_types": []}
    });
    gateway_admin::upsert(&s.pool, &id, false, &cfg, &json!({})).await?;
    pelty_database::audit::record(&s.pool, Some(u.user_id), "gateway.custom_created", Some(&id), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(delete, path = "/api/v1/admin/gateways/{id}", responses((status = 204), (status = 404), (status = 409)))]
pub async fn delete_custom(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::GatewaysManage>,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !id.starts_with(CUSTOM_PREFIX) {
        return Err(AppError::Conflict("error.gateway_builtin").into());
    }
    if !gateway_admin::delete(&s.pool, &id).await? {
        return Err(AppError::NotFound.into());
    }
    pelty_database::audit::record(&s.pool, Some(u.user_id), "gateway.custom_deleted", Some(&id), u.ip.as_deref(), json!({})).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct TestWebhook {
    #[serde(default)]
    pub headers: HashMap<String, String>,
    pub body: String,
    /// Unix seconds used for timestamp checks (default: now).
    pub now: Option<i64>,
}

/// Verify + parse a pasted webhook without applying it (logged as a test).
#[utoipa::path(post, path = "/api/v1/admin/gateways/{id}/test-webhook", request_body = TestWebhook, responses((status = 200), (status = 404)))]
pub async fn test_webhook(
    State(s): State<AppState>,
    _p: RequirePermission<perms::GatewaysManage>,
    Path(id): Path<String>,
    Json(b): Json<TestWebhook>,
) -> ApiResult<Json<Value>> {
    let row = pelty_database::extensions::gateway(&s.pool, &id).await?.ok_or(AppError::NotFound)?;
    let gw = match s.gateways.build(&id, &resolve_config(&row)) {
        Ok(g) => g,
        Err(e) => return Ok(Json(json!({ "ok": false, "stage": "config", "error": e.to_string() }))),
    };
    let hdrs = b.headers.iter().map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let now = b.now.unwrap_or_else(|| time::OffsetDateTime::now_utc().unix_timestamp());
    let res: Result<WebhookEvent, GatewayError> = gw.handle_webhook(&hdrs, b.body.as_bytes(), now).await;
    let mut log = NewDelivery { gateway: &id, is_test: true, body_bytes: b.body.len().min(i32::MAX as usize) as i32, ..Default::default() };
    let out = match &res {
        Ok(ev) => {
            log.signature_ok = true;
            log.status_code = 200;
            log.event_id = Some(&ev.event_id);
            log.event_type = Some(&ev.event_type);
            log.kind = Some(kind_str(&ev.kind));
            log.provider_ref = ev.provider_ref.as_deref();
            json!({ "ok": true, "event": ev })
        }
        Err(e) => {
            log.status_code = if *e == GatewayError::Unsupported { 404 } else { 400 };
            json!({ "ok": false, "stage": if *e == GatewayError::InvalidSignature { "signature" } else { "parse" }, "error": e.to_string() })
        }
    };
    let err = res.as_ref().err().map(|e| e.to_string());
    log.error = err.as_deref();
    gateway_admin::log_delivery(&s.pool, &log).await?;
    Ok(Json(out))
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct DeliveryQuery {
    pub gateway: Option<String>,
    pub limit: Option<i64>,
}

#[utoipa::path(get, path = "/api/v1/admin/gateways/deliveries", params(DeliveryQuery), responses((status = 200)))]
pub async fn list_deliveries(
    State(s): State<AppState>,
    _p: RequirePermission<perms::GatewaysManage>,
    Query(q): Query<DeliveryQuery>,
) -> ApiResult<Json<Value>> {
    let items = gateway_admin::deliveries(&s.pool, q.gateway.as_deref().filter(|g| !g.is_empty()), q.limit.unwrap_or(100).clamp(1, 500)).await?;
    Ok(Json(json!({ "items": items })))
}

/// Whether a gateway can take this currency/amount (descriptor + operator limits).
pub fn eligible(d: &GatewayDescriptor, row: &GatewayRow, currency: &str, amount: Decimal) -> bool {
    if !d.currencies.is_empty() && !d.currencies.contains(&currency) {
        return false;
    }
    let c = &row.config;
    if let Some(list) = c.get("currencies").and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()) {
        if !list.split(',').any(|x| x.trim().eq_ignore_ascii_case(currency)) {
            return false;
        }
    }
    let lim = |k: &str| c.get(k).and_then(|v| v.as_str()).and_then(|v| Decimal::from_str(v).ok());
    if lim("min_amount").is_some_and(|m| amount < m) || lim("max_amount").is_some_and(|m| m > Decimal::ZERO && amount > m) {
        return false;
    }
    d.capabilities.checkout
}

pub async fn eligible_for(s: &AppState, currency: &str, amount: Decimal) -> ApiResult<Vec<Value>> {
    let mut out = vec![];
    for r in pelty_database::extensions::gateways(&s.pool).await?.into_iter().filter(|g| g.enabled) {
        let Some(d) = s.gateways.descriptor(&r.id) else { continue };
        if !crate::addons::gateway_enabled(s, &r.id).await? {
            continue;
        }
        if !eligible(&d, &r, currency, amount) {
            continue;
        }
        let rules = pelty_database::gateway_rules::get(&s.pool, &r.id).await?;
        if rules.check(currency, amount).is_err() {
            continue;
        }
        let exp = pelty_database::branding::exponent(&s.pool, currency).await?;
        let fee = rules.fee(currency, amount, exp);
        let name = r.config.get("display_name").and_then(|v| v.as_str()).filter(|x| !x.is_empty())
            .or_else(|| r.config.get("name").and_then(|v| v.as_str()).filter(|_| r.id.starts_with(CUSTOM_PREFIX)))
            .unwrap_or(d.name);
        out.push(json!({ "id": r.id, "kind": d.id, "name": name, "fee": fee }));
    }
    Ok(out)
}

/// Gateways usable for this invoice (currency + amount limits). Ids/names only.
#[utoipa::path(get, path = "/api/v1/client/invoices/{id}/gateways", responses((status = 200), (status = 404)))]
pub async fn client_invoice_gateways(State(s): State<AppState>, u: CurrentUser, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let inv = s.invoices().get_owned(id, cid).await?.ok_or(AppError::NotFound)?;
    let balance = inv.balance();
    let items = if inv.status == "unpaid" && balance > Decimal::ZERO { eligible_for(&s, &inv.currency, balance).await? } else { vec![] };
    Ok(Json(json!({ "items": items, "currency": inv.currency, "amount": balance, "status": inv.status })))
}

#[derive(Deserialize, utoipa::ToSchema, Default)]
pub struct CompleteBody {
    /// Query parameters the provider appended to the return URL.
    #[serde(default)]
    pub query: HashMap<String, String>,
}

/// Payer returned from the provider: capture/check and apply (idempotent).
#[utoipa::path(post, path = "/api/v1/client/payments/{id}/complete", request_body = CompleteBody, responses((status = 200), (status = 404)))]
pub async fn client_complete(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<CompleteBody>,
) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    let p = gateway_admin::owned_payment(&s.pool, id, cid).await?.ok_or(AppError::NotFound)?;
    if p.status != "pending" {
        return Ok(Json(json!({ "status": p.status, "invoice_id": p.invoice_id })));
    }
    let Some(pref) = p.provider_ref.clone() else {
        return Ok(Json(json!({ "status": "pending", "invoice_id": p.invoice_id })));
    };
    let row = pelty_database::extensions::gateway(&s.pool, &p.gateway).await?.ok_or(AppError::NotFound)?;
    let gw = s.gateways.build(&p.gateway, &resolve_config(&row)).map_err(|_| AppError::BadRequest("error.gateway"))?;
    let st = match gw.complete_payment(&pref, &b.query).await {
        Ok(st) => st,
        // Webhook-only gateways: report pending; the webhook will settle it.
        Err(GatewayError::Unsupported) => CompleteStatus::Pending,
        Err(e) => {
            tracing::warn!(gateway = %p.gateway, error = %e, "complete_payment failed");
            return Err(AppError::BadRequest("error.gateway").into());
        }
    };
    let kind = match st {
        CompleteStatus::Succeeded => WebhookKind::PaymentSucceeded,
        CompleteStatus::Failed => WebhookKind::PaymentFailed,
        CompleteStatus::Pending => return Ok(Json(json!({ "status": "pending", "invoice_id": p.invoice_id }))),
    };
    let ev = WebhookEvent::new(format!("return:{id}"), "return", kind, Some(pref));
    apply_event(&s, &p.gateway, &ev).await?;
    let now = gateway_admin::owned_payment(&s.pool, id, cid).await?.map(|x| x.status).unwrap_or_default();
    Ok(Json(json!({ "status": now, "invoice_id": p.invoice_id })))
}

/// Gateway chosen in the setup wizard (`pelty-server setup`).
pub struct SetupGateway {
    /// Addon package id (`gateway-stripe`).
    pub addon: String,
    /// Secret name → environment variable NAME holding it (written to `.env`).
    pub secret_refs: Map<String, Value>,
    /// Prefer the provider's test / sandbox mode when it has a setting for it.
    pub test_mode: bool,
}

/// Install and enable the gateway addon (as the console would, approving its
/// declared capabilities since the operator picked it), then save the gateway
/// row like `PUT /admin/gateways/{id}`: secrets stay env-var references, never
/// values. A gateway that still needs settings is saved disabled. Returns a
/// one-line result for the console.
pub async fn configure_from_setup(s: &AppState, g: &SetupGateway) -> Result<String, AppError> {
    let actor = crate::addons::Actor::CONSOLE;
    if pelty_database::addon_registry::get(&s.pool, &g.addon).await?.is_none() {
        crate::wasm_addons::seed_bundled(s).await?;
        let e = crate::addons::find(s, &g.addon)?;
        if e.manifest.kind != "gateway" {
            return Err(AppError::Validation("error.addon_not_installed"));
        }
        let approve = crate::addons::dangerous_caps(&e.manifest);
        crate::addons::install_addon(s, &g.addon, &json!({ "approve": approve }), actor).await?;
    }
    crate::addons::set_addon_enabled(s, &g.addon, true, actor).await?;
    let e = crate::addons::find(s, &g.addon)?;
    let id = e.manifest.engine_id().to_string();
    let d = s.gateways.descriptor(&id).ok_or(AppError::Validation("error.addon_not_installed"))?;
    let mut config = Map::new();
    if let Some(f) = d.settings.iter().find(|f| ["sandbox", "test_mode", "testmode", "test"].contains(&f.key) && f.kind == "bool") {
        config.insert(f.key.into(), Value::Bool(g.test_mode));
    } else if let Some(f) = d.settings.iter().find(|f| f.kind == "select" && ["mode", "environment", "env"].contains(&f.key)) {
        let want: &[&str] = if g.test_mode { &["sandbox", "test"] } else { &["live", "production", "prod"] };
        if let Some(o) = f.options.iter().find(|o| want.contains(o)) {
            config.insert(f.key.into(), Value::String((*o).into()));
        }
    }
    let refs: Map<String, Value> = g.secret_refs.iter().filter(|(k, v)| d.secrets.contains(&k.as_str()) && v.as_str().is_some_and(env_name_ok)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let config = Value::Object(config);
    let row = GatewayRow { id: id.clone(), enabled: true, config: config.clone(), secret_refs: Value::Object(refs.clone()) };
    let ready = validate_config(&s.gateways, &d, &id, &config).is_ok() && s.gateways.build(&id, &resolve_config(&row)).is_ok();
    gateway_admin::upsert(&s.pool, &id, ready, &config, &Value::Object(refs)).await?;
    pelty_database::audit::record(&s.pool, None, "gateway.updated", Some(&id), None, json!({ "enabled": ready, "via": "setup" })).await?;
    Ok(if ready { format!("{} enabled", d.name) } else { format!("{} saved but disabled: finish its settings in Admin > Gateways", d.name) })
}
