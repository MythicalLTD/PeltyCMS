//! SAML 2.0 Service Provider (addon `sso-saml`).
//!
//! Any number of IdPs (`saml_idps`), each with its own SP endpoints:
//! metadata, ACS (HTTP-POST), SLO (HTTP-Redirect) and an SP-initiated login
//! (HTTP-Redirect AuthnRequest, optionally signed). Validation lives in the
//! pure-Rust `pelty-saml` crate; this module adds request tracking
//! (InResponseTo), the replay cache, account linking/provisioning, attribute
//! mapping and sessions. See `make docs`.
use crate::addons::Manifest;
use crate::error::ApiResult;
use crate::extract::{perms::SettingsManage, CurrentUser, RequirePermission, SESSION_COOKIE};
use crate::routes::auth::AuthGuard;
use crate::AppState;
use axum::extract::{Path, Query, RawQuery, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use pelty_core::{AppError, AppResult};
use pelty_database::saml::{self as db, IdpInput, SamlIdp};
use pelty_database::{addon_registry as reg, audit, client_groups, security, users};
use pelty_saml::dsig::{cert_der, cert_pem, PublicKey};
use pelty_saml::keys::SigningKey;
use pelty_saml::sp::{self, Assertion, Expect, Idp, Sp};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub const ADDON_ID: &str = "sso-saml";
pub const ENGINE: &str = "saml";

pub fn builtin_manifests() -> Vec<Manifest> {
    vec![Manifest {
        id: ADDON_ID.into(),
        name: "SAML 2.0".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        kind: "sso".into(),
        engine: ENGINE.into(),
        description: "addons.desc.sso.saml".into(),
        author: "MythicalSystems".into(),
        permissions: vec!["network.outbound".into()],
        settings: vec![],
        migrations: vec![],
        files: BTreeMap::new(),
        ..Default::default()
    }]
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/saml/providers", get(providers))
        .route("/sso/saml/{id}/metadata", get(metadata))
        .route("/sso/saml/{id}/login", get(login))
        .route("/sso/saml/{id}/acs", post(acs))
        .route("/sso/saml/{id}/slo", get(slo))
        .route("/auth/saml/logout", post(logout))
        .route("/admin/saml/idps", get(admin_list).post(admin_create))
        .route(
            "/admin/saml/idps/{id}",
            get(admin_get).put(admin_update).delete(admin_delete),
        )
        .route("/admin/saml/idps/{id}/rotate-key", post(admin_rotate_key))
        .route("/admin/saml/idps/{id}/validate", post(admin_validate))
        .route("/admin/saml/metadata", post(admin_parse_metadata))
}

struct Urls {
    entity: String,
    acs: String,
    slo: String,
    metadata: String,
    login: String,
}

fn urls(s: &AppState, idp: &SamlIdp) -> Urls {
    let base = format!(
        "{}/api/v1/sso/saml/{}",
        s.cfg.public_url.trim_end_matches('/'),
        idp.id
    );
    let metadata = format!("{base}/metadata");
    Urls {
        entity: if idp.sp_entity_id.trim().is_empty() {
            metadata.clone()
        } else {
            idp.sp_entity_id.clone()
        },
        acs: format!("{base}/acs"),
        slo: format!("{base}/slo"),
        login: format!("/api/v1/sso/saml/{}/login", idp.id),
        metadata,
    }
}

async fn addon_on(s: &AppState) -> AppResult<bool> {
    Ok(reg::get(&s.pool, ADDON_ID)
        .await?
        .is_some_and(|a| a.enabled))
}

/// An enabled IdP while the addon is enabled; otherwise 404.
async fn load(s: &AppState, id: Uuid) -> AppResult<SamlIdp> {
    if !addon_on(s).await? {
        return Err(AppError::NotFound);
    }
    db::get(&s.pool, id)
        .await?
        .filter(|i| i.enabled)
        .ok_or(AppError::NotFound)
}

fn idp_keys(idp: &SamlIdp) -> Vec<PublicKey> {
    idp.idp_certs
        .iter()
        .filter_map(|c| PublicKey::from_cert(c).ok())
        .collect()
}

fn sp_key(s: &AppState, idp: &SamlIdp) -> Option<SigningKey> {
    if !idp.sign_requests {
        return None;
    }
    let sealed = idp.sp_key_sealed.as_ref()?;
    let pem = s
        .secret_box
        .as_ref()?
        .open(sealed)
        .and_then(|p| String::from_utf8(p).ok())?;
    SigningKey::rsa_from_pkcs8_pem(&pem).ok()
}

/// Generate (or rotate) the SP signing key + self-signed certificate.
async fn ensure_sp_key(s: &AppState, id: Uuid, rotate: bool) -> AppResult<()> {
    let idp = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    if idp.sp_key_sealed.is_some() && !rotate {
        return Ok(());
    }
    let sb = s
        .secret_box
        .clone()
        .ok_or(AppError::Conflict("error.saml_no_encryption_key"))?;
    let name = format!("pelty-sp-{}", &id.to_string()[..8]);
    let (pem, cert) =
        tokio::task::spawn_blocking(move || -> Result<(String, String), pelty_saml::Error> {
            let k = SigningKey::generate_rsa()?;
            Ok((k.rsa_pkcs8_pem()?, k.certificate(&name, 3650)?))
        })
        .await
        .map_err(|_| AppError::Internal("saml keygen".into()))?
        .map_err(|_| AppError::Internal("saml keygen".into()))?;
    db::set_sp_key(&s.pool, id, &sb.seal(pem.as_bytes()), &cert).await
}

fn see_other(to: &str) -> Response {
    let mut r = StatusCode::SEE_OTHER.into_response();
    if let Ok(v) = HeaderValue::from_str(to) {
        r.headers_mut().insert(header::LOCATION, v);
    }
    r
}

// ----- public -----

#[utoipa::path(get, path = "/api/v1/auth/saml/providers", responses((status = 200)))]
pub async fn providers(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let mut out = vec![];
    if addon_on(&s).await? {
        for i in db::list(&s.pool)
            .await?
            .into_iter()
            .filter(|i| i.enabled && !i.idp_certs.is_empty())
        {
            let label = if i.button_label.trim().is_empty() {
                i.name.clone()
            } else {
                i.button_label.clone()
            };
            out.push(json!({ "id": i.id, "label": label, "url": urls(&s, &i).login, "allow_register": i.allow_register, "staff": i.allow_staff }));
        }
    }
    Ok(Json(json!({ "providers": out })))
}

#[utoipa::path(get, path = "/api/v1/sso/saml/{id}/metadata", params(("id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn metadata(State(s): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let idp = load(&s, id).await?;
    let u = urls(&s, &idp);
    let cert = if idp.sign_requests {
        idp.sp_cert_pem.as_deref()
    } else {
        None
    };
    let xml = sp::sp_metadata(
        &Sp {
            entity_id: &u.entity,
            acs_url: &u.acs,
            slo_url: &u.slo,
        },
        cert,
        idp.want_assertions_signed,
    );
    Ok((
        [(
            header::CONTENT_TYPE,
            "application/samlmetadata+xml; charset=utf-8",
        )],
        xml,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct LoginQuery {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub format: Option<String>,
}

async fn session_user(s: &AppState, headers: &HeaderMap) -> AppResult<Uuid> {
    let tok = crate::extract::cookie(headers, SESSION_COOKIE).ok_or(AppError::Unauthenticated)?;
    Ok(s.auth
        .authenticate(&tok)
        .await?
        .ok_or(AppError::Unauthenticated)?
        .user_id)
}

/// SP-initiated login: redirect to the IdP with an AuthnRequest.
#[utoipa::path(get, path = "/api/v1/sso/saml/{id}/login", params(("id" = Uuid, Path)), responses((status = 303), (status = 404)))]
pub async fn login(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Query(q): Query<LoginQuery>,
) -> ApiResult<Response> {
    let idp = load(&s, id).await?;
    let mode = q.mode.as_deref().unwrap_or("login");
    let user = match mode {
        "login" => None,
        "link" => Some(session_user(&s, &headers).await?),
        "test" => {
            let u = session_user(&s, &headers).await?;
            if !users::permissions(&s.pool, u)
                .await?
                .iter()
                .any(|p| p == "settings.manage")
            {
                return Err(AppError::Forbidden.into());
            }
            Some(u)
        }
        _ => return Err(AppError::Validation("error.oauth_mode").into()),
    };
    if idp.idp_certs.is_empty() || idp.sso_url.is_empty() {
        return Err(AppError::Conflict("error.saml_not_configured").into());
    }
    let u = urls(&s, &idp);
    let req_id = sp::new_id();
    db::put_request(&s.pool, &req_id, idp.id, mode, user).await?;
    let xml = sp::authn_request(
        &req_id,
        &Sp {
            entity_id: &u.entity,
            acs_url: &u.acs,
            slo_url: &u.slo,
        },
        &idp.sso_url,
        OffsetDateTime::now_utc(),
    );
    let key = sp_key(&s, &idp);
    if idp.sign_requests && key.is_none() {
        return Err(AppError::Conflict("error.saml_no_sp_key").into());
    }
    let url = sp::redirect_url(&idp.sso_url, "SAMLRequest", &xml, None, key.as_ref())
        .map_err(|_| AppError::Internal("saml request".into()))?;
    if q.format.as_deref() == Some("json") {
        return Ok(Json(json!({ "url": url, "request_id": req_id })).into_response());
    }
    Ok(see_other(&url))
}

#[derive(Deserialize, utoipa::ToSchema)]
pub struct AcsForm {
    #[serde(rename = "SAMLResponse")]
    pub saml_response: String,
    #[serde(rename = "RelayState", default)]
    pub relay_state: Option<String>,
}

struct Mapped {
    email: String,
    name: String,
    groups: Vec<String>,
}

fn map_attrs(idp: &SamlIdp, a: &Assertion) -> Mapped {
    let mut email = a.first(&idp.attr_email).unwrap_or("").trim().to_lowercase();
    if email.is_empty() && idp.nameid_is_email {
        email = a.name_id.trim().to_lowercase();
    }
    if !(email.contains('@') && email.len() <= 254) {
        email.clear();
    }
    let mut name = a.first(&idp.attr_name).unwrap_or("").trim().to_string();
    if name.is_empty() {
        name = format!(
            "{} {}",
            a.first(&idp.attr_first_name).unwrap_or(""),
            a.first(&idp.attr_last_name).unwrap_or("")
        )
        .trim()
        .to_string();
    }
    Mapped {
        email,
        name: name.chars().take(100).collect(),
        groups: a.all(&idp.attr_groups).to_vec(),
    }
}

fn validate(
    s: &AppState,
    idp: &SamlIdp,
    xml: &str,
    request_id: Option<&str>,
) -> Result<Assertion, pelty_saml::Error> {
    let u = urls(s, idp);
    let keys = idp_keys(idp);
    if keys.is_empty() {
        return Err(pelty_saml::Error::Cert("none_configured"));
    }
    sp::validate_response(
        xml,
        &Sp {
            entity_id: &u.entity,
            acs_url: &u.acs,
            slo_url: &u.slo,
        },
        &Idp {
            entity_id: &idp.idp_entity_id,
            keys: &keys,
        },
        &Expect {
            request_id,
            allow_unsolicited: idp.allow_idp_initiated,
            require_signed_assertion: idp.want_assertions_signed,
            now: OffsetDateTime::now_utc(),
            skew: Duration::seconds(i64::from(idp.clock_skew_secs.clamp(0, 600))),
        },
    )
}

fn diag(idp: &SamlIdp, a: &Assertion) -> Value {
    let m = map_attrs(idp, a);
    json!({
        "ok": true,
        "name_id": a.name_id,
        "name_id_format": a.name_id_format,
        "session_index": a.session_index,
        "attributes": a.attributes,
        "mapped": { "email": m.email, "name": m.name, "groups": m.groups },
    })
}

/// Assertion Consumer Service (HTTP-POST binding).
#[utoipa::path(post, path = "/api/v1/sso/saml/{id}/acs", params(("id" = Uuid, Path)), responses((status = 303)))]
pub async fn acs(
    State(s): State<AppState>,
    g: AuthGuard,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Form(f): Form<AcsForm>,
) -> ApiResult<Response> {
    let _ = &f.relay_state;
    let idp = match load(&s, id).await {
        Ok(i) => i,
        Err(_) => return Ok(see_other("/login?saml_error=saml_failed")),
    };
    let mut mode = String::from("login");
    match finish(&s, &g, &idp, &headers, &f.saml_response, &mut mode).await {
        Ok(r) => Ok(r),
        Err(code) => {
            tracing::info!(idp = %idp.id, code, "saml response rejected");
            let _ = audit::record(
                &s.pool,
                None,
                "auth.saml_rejected",
                Some(&idp.id.to_string()),
                g.ip(),
                json!({ "code": code }),
            )
            .await;
            if mode == "test" {
                db::set_last_test(&s.pool, idp.id, &json!({ "ok": false, "error": code })).await?;
                return Ok(see_other(&format!("/admin/saml?tested={}", idp.id)));
            }
            let public = if code.starts_with("account.") {
                code.trim_start_matches("account.")
            } else {
                "saml_failed"
            };
            Ok(see_other(&format!("/login?saml_error={public}")))
        }
    }
}

async fn finish(
    s: &AppState,
    g: &AuthGuard,
    idp: &SamlIdp,
    headers: &HeaderMap,
    b64: &str,
    mode: &mut String,
) -> Result<Response, String> {
    let internal = |e: AppError| {
        format!("internal.{e:?}")
            .chars()
            .take(60)
            .collect::<String>()
    };
    let xml = sp::decode_post(b64).map_err(|e| e.code())?;
    let irt = sp::peek_in_response_to(&xml);
    let mut link_user = None;
    if let Some(r) = &irt {
        let (m, u) = db::take_request(&s.pool, r, idp.id)
            .await
            .map_err(internal)?
            .ok_or("invalid.unknown_request")?;
        *mode = m;
        link_user = u;
    }
    let a = validate(s, idp, &xml, irt.as_deref().filter(|_| !mode.is_empty()))
        .map_err(|e| e.code())?;
    if !db::remember_assertion(&s.pool, idp.id, &a.id, a.replay_until)
        .await
        .map_err(internal)?
    {
        return Err("invalid.replay".into());
    }
    if mode == "test" {
        db::set_last_test(&s.pool, idp.id, &diag(idp, &a))
            .await
            .map_err(internal)?;
        return Ok(see_other(&format!("/admin/saml?tested={}", idp.id)));
    }
    login_user(s, g, idp, headers, &a, mode, link_user)
        .await
        .map_err(|e| match e {
            AppError::Validation(c) | AppError::Conflict(c) | AppError::Denied(c) => {
                format!("account.{}", c.trim_start_matches("error."))
            }
            other => internal(other),
        })
}

async fn is_staff(s: &AppState, user: Uuid) -> AppResult<bool> {
    Ok(users::roles(&s.pool, user)
        .await?
        .iter()
        .any(|r| r != "customer"))
}

async fn login_user(
    s: &AppState,
    g: &AuthGuard,
    idp: &SamlIdp,
    headers: &HeaderMap,
    a: &Assertion,
    mode: &str,
    link_user: Option<Uuid>,
) -> AppResult<Response> {
    let provider = format!("saml:{}", idp.id);
    let m = map_attrs(idp, a);
    let ip = g.ip();
    if mode == "link" {
        let uid = link_user.ok_or(AppError::Unauthenticated)?;
        security::link_identity(&s.pool, uid, &provider, &a.name_id, &m.email).await?;
        audit::record(
            &s.pool,
            Some(uid),
            "auth.identity_linked",
            Some(&provider),
            ip,
            json!({ "provider": provider }),
        )
        .await?;
        return Ok(see_other("/sessions?linked=saml"));
    }
    let user = match security::find_identity(&s.pool, &provider, &a.name_id).await? {
        Some(i) => {
            security::touch_identity(&s.pool, i.id).await?;
            users::find(&s.pool, i.user_id)
                .await?
                .ok_or(AppError::Unauthenticated)?
        }
        None => {
            let existing = if m.email.is_empty() {
                None
            } else {
                users::find_by_email(&s.pool, &m.email).await?
            };
            match existing {
                // Only a local account whose email is itself verified is linked automatically.
                Some(u)
                    if idp.link_by_verified_email
                        && u.email_verified_at.is_some()
                        && !is_staff(s, u.id).await? =>
                {
                    security::link_identity(&s.pool, u.id, &provider, &a.name_id, &m.email).await?;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.identity_linked",
                        Some(&provider),
                        ip,
                        json!({ "provider": provider, "by": "verified_email" }),
                    )
                    .await?;
                    u
                }
                Some(_) => return Err(AppError::Conflict("error.oauth_email_in_use")),
                None if idp.allow_register && !m.email.is_empty() => {
                    let pw = format!("{}Aa1!", pelty_auth::token::generate());
                    let name = if m.name.is_empty() {
                        m.email.split('@').next().unwrap_or("").to_string()
                    } else {
                        m.name.clone()
                    };
                    let u = s
                        .auth
                        .register(
                            pelty_auth::RegisterInput {
                                email: &m.email,
                                password: &pw,
                                display_name: &name,
                            },
                            "customer",
                            ip,
                        )
                        .await?;
                    // The admin trusts this IdP for its users' addresses.
                    users::mark_verified(&s.pool, u.id).await?;
                    security::link_identity(&s.pool, u.id, &provider, &a.name_id, &m.email).await?;
                    audit::record(
                        &s.pool,
                        Some(u.id),
                        "auth.registered_via_sso",
                        Some(&provider),
                        ip,
                        json!({ "provider": provider }),
                    )
                    .await?;
                    s.emit(pelty_extensions::events::DomainEvent::new(
                        "user.registered",
                        json!({ "user_id": u.id, "via": provider }),
                    ));
                    users::find(&s.pool, u.id)
                        .await?
                        .ok_or(AppError::Unauthenticated)?
                }
                None => return Err(AppError::Validation("error.oauth_not_linked")),
            }
        }
    };
    if !user.is_active {
        return Err(AppError::Validation("error.oauth_failed"));
    }
    apply_groups(s, idp, &user, &m.groups).await?;
    let staff = is_staff(s, user.id).await?;
    if staff && !idp.allow_staff {
        return Err(AppError::Denied("error.oauth_staff_disabled"));
    }
    if users::is_locked(&s.pool, user.id).await? {
        return Err(AppError::Validation("error.account_locked"));
    }
    if crate::duo::applies(s, user.id).await? {
        let ch = crate::routes::mfa::start_challenge(s, user.id)
            .await
            .map_err(|_| AppError::Validation("error.oauth_failed"))?;
        return Ok(see_other(&format!("/login?mfa_challenge={ch}&duo=1")));
    }
    if pelty_database::mfa::is_enabled(&s.pool, user.id).await? {
        let ch = crate::routes::mfa::start_challenge(s, user.id)
            .await
            .map_err(|_| AppError::Validation("error.oauth_failed"))?;
        return Ok(see_other(&format!("/login?mfa_challenge={ch}")));
    }
    audit::record(
        &s.pool,
        Some(user.id),
        "auth.login_sso",
        Some(&provider),
        ip,
        json!({ "provider": provider }),
    )
    .await?;
    let uid = user.id;
    let r = crate::routes::auth::session_response(s, g, headers, user)
        .await
        .map_err(|_| AppError::Validation("error.oauth_failed"))?;
    let mut out = see_other(if staff { "/admin" } else { "/dashboard" });
    for c in r.headers().get_all(header::SET_COOKIE) {
        if let Some(tok) = c
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix(&format!("{SESSION_COOKIE}=")))
            .and_then(|v| v.split(';').next())
        {
            if let Ok(Some(sess)) = s.auth.authenticate(tok).await {
                db::link_session(
                    &s.pool,
                    sess.id,
                    idp.id,
                    uid,
                    &a.name_id,
                    a.session_index.as_deref(),
                )
                .await?;
            }
        }
        out.headers_mut().append(header::SET_COOKIE, c.clone());
    }
    Ok(out)
}

/// Group mapping: IdP group -> client group (customers) and, only when staff
/// sign-in is allowed, IdP group -> role. Roles are only ever added.
async fn apply_groups(
    s: &AppState,
    idp: &SamlIdp,
    user: &users::User,
    groups: &[String],
) -> AppResult<()> {
    if groups.is_empty() {
        return Ok(());
    }
    if let Some(map) = idp.group_map.as_object() {
        if let Some(gid) = groups
            .iter()
            .find_map(|g| map.get(g).and_then(Value::as_str))
            .and_then(|v| v.parse::<Uuid>().ok())
        {
            if let Some(cid) = client_groups::customer_by_email(&s.pool, &user.email).await? {
                if client_groups::of_customer(&s.pool, cid)
                    .await?
                    .map(|g| g.id)
                    != Some(gid)
                    && client_groups::assign(&s.pool, cid, Some(gid)).await?
                {
                    audit::record(
                        &s.pool,
                        Some(user.id),
                        "auth.saml_group_mapped",
                        Some(&gid.to_string()),
                        None,
                        json!({ "idp": idp.id }),
                    )
                    .await?;
                }
            }
        }
    }
    if idp.allow_staff {
        if let Some(map) = idp.role_map.as_object() {
            for g in groups {
                if let Some(role) = map.get(g).and_then(Value::as_str) {
                    if db::grant_role(&s.pool, user.id, role).await? {
                        audit::record(
                            &s.pool,
                            Some(user.id),
                            "auth.saml_role_granted",
                            Some(role),
                            None,
                            json!({ "idp": idp.id, "group": g }),
                        )
                        .await?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// SLO endpoint (HTTP-Redirect): IdP LogoutRequest (signature required) or
/// the IdP's LogoutResponse to our request.
#[utoipa::path(get, path = "/api/v1/sso/saml/{id}/slo", params(("id" = Uuid, Path)), responses((status = 303), (status = 400)))]
pub async fn slo(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    RawQuery(raw): RawQuery,
) -> ApiResult<Response> {
    let idp = load(&s, id).await?;
    let raw = raw.unwrap_or_default();
    if raw.contains("SAMLResponse=") {
        // Our session is already gone; the response carries no authority.
        return Ok(see_other("/login?logged_out=1"));
    }
    let bad = || AppError::Validation("error.saml_logout_invalid");
    let msg = sp::parse_redirect(&raw, &idp_keys(&idp)).map_err(|_| bad())?;
    let req = sp::parse_logout_request(
        &msg.xml,
        &idp.idp_entity_id,
        OffsetDateTime::now_utc(),
        Duration::seconds(i64::from(idp.clock_skew_secs)),
    )
    .map_err(|_| bad())?;
    let n = db::revoke_by_name_id(&s.pool, idp.id, &req.name_id).await?;
    audit::record(
        &s.pool,
        None,
        "auth.saml_logout",
        Some(&idp.id.to_string()),
        None,
        json!({ "sessions": n }),
    )
    .await?;
    if idp.slo_url.is_empty() {
        return Ok(Json(json!({ "ok": true, "revoked": n })).into_response());
    }
    let u = urls(&s, &idp);
    let xml = sp::logout_response(
        &req.id,
        &Sp {
            entity_id: &u.entity,
            acs_url: &u.acs,
            slo_url: &u.slo,
        },
        &idp.slo_url,
        OffsetDateTime::now_utc(),
    );
    let url = sp::redirect_url(
        &idp.slo_url,
        "SAMLResponse",
        &xml,
        msg.relay.as_deref(),
        sp_key(&s, &idp).as_ref(),
    )
    .map_err(|_| bad())?;
    Ok(see_other(&url))
}

/// End the current session; when it came from a SAML IdP with SLO, return
/// the IdP logout URL for the browser to visit.
#[utoipa::path(post, path = "/api/v1/auth/saml/logout", responses((status = 200), (status = 401)))]
pub async fn logout(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let link = db::session(&s.pool, u.session_id).await?;
    s.auth
        .revoke(u.user_id, u.session_id, u.ip.as_deref(), "auth.logout")
        .await?;
    let Some(link) = link else {
        return Ok(Json(json!({ "url": "/login" })));
    };
    let Ok(idp) = load(&s, link.idp_id).await else {
        return Ok(Json(json!({ "url": "/login" })));
    };
    if idp.slo_url.is_empty() {
        return Ok(Json(json!({ "url": "/login" })));
    }
    let urls = urls(&s, &idp);
    let xml = sp::logout_request(
        &sp::new_id(),
        &Sp {
            entity_id: &urls.entity,
            acs_url: &urls.acs,
            slo_url: &urls.slo,
        },
        &idp.slo_url,
        &link.name_id,
        link.session_index.as_deref(),
        OffsetDateTime::now_utc(),
    );
    let url = sp::redirect_url(
        &idp.slo_url,
        "SAMLRequest",
        &xml,
        None,
        sp_key(&s, &idp).as_ref(),
    )
    .map_err(|_| AppError::Internal("saml logout".into()))?;
    Ok(Json(json!({ "url": url })))
}

// ----- admin -----

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct IdpBody {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub idp_entity_id: String,
    #[serde(default)]
    pub sso_url: String,
    #[serde(default)]
    pub slo_url: String,
    /// PEM or bare base64 certificates.
    #[serde(default)]
    pub idp_certs: Vec<String>,
    #[serde(default)]
    pub metadata_url: String,
    /// Raw IdP metadata; fills entity id, URLs and certificates.
    #[serde(default)]
    pub metadata_xml: String,
    #[serde(default)]
    pub sp_entity_id: String,
    #[serde(default)]
    pub sign_requests: bool,
    #[serde(default = "yes")]
    pub want_assertions_signed: bool,
    #[serde(default)]
    pub allow_idp_initiated: bool,
    #[serde(default = "skew")]
    pub clock_skew_secs: i32,
    #[serde(default = "a_email")]
    pub attr_email: String,
    #[serde(default = "a_name")]
    pub attr_name: String,
    #[serde(default = "a_first")]
    pub attr_first_name: String,
    #[serde(default = "a_last")]
    pub attr_last_name: String,
    #[serde(default = "a_groups")]
    pub attr_groups: String,
    #[serde(default = "obj")]
    #[schema(value_type = Object)]
    pub group_map: Value,
    #[serde(default = "obj")]
    #[schema(value_type = Object)]
    pub role_map: Value,
    #[serde(default)]
    pub nameid_is_email: bool,
    #[serde(default)]
    pub allow_register: bool,
    #[serde(default)]
    pub link_by_verified_email: bool,
    #[serde(default)]
    pub allow_staff: bool,
    #[serde(default)]
    pub button_label: String,
}
fn yes() -> bool {
    true
}
fn skew() -> i32 {
    60
}
fn a_email() -> String {
    "email".into()
}
fn a_name() -> String {
    "displayName".into()
}
fn a_first() -> String {
    "givenName".into()
}
fn a_last() -> String {
    "sn".into()
}
fn a_groups() -> String {
    "groups".into()
}
fn obj() -> Value {
    json!({})
}

fn url_ok(u: &str) -> bool {
    u.len() <= 2048
        && (u.starts_with("https://") || u.starts_with("http://"))
        && !u.contains(char::is_whitespace)
}

async fn fetch_metadata(s: &AppState, url: &str) -> AppResult<String> {
    if !url_ok(url) {
        return Err(AppError::Validation("error.saml_metadata_url"));
    }
    let (st, body) = s
        .http()
        .send(pelty_payments::HttpRequestSpec {
            method: "GET",
            url: url.into(),
            headers: vec![(
                "accept".into(),
                "application/samlmetadata+xml, application/xml".into(),
            )],
            body: String::new(),
        })
        .await
        .map_err(|_| AppError::Validation("error.saml_metadata_fetch"))?;
    if !(200..300).contains(&st) {
        return Err(AppError::Validation("error.saml_metadata_fetch"));
    }
    Ok(body)
}

async fn to_input(s: &AppState, mut b: IdpBody) -> AppResult<IdpInput> {
    let v = |c: &'static str| AppError::Validation(c);
    if b.metadata_xml.trim().is_empty()
        && !b.metadata_url.trim().is_empty()
        && b.idp_certs.is_empty()
    {
        b.metadata_xml = fetch_metadata(s, b.metadata_url.trim()).await?;
    }
    if !b.metadata_xml.trim().is_empty() {
        let md = sp::parse_idp_metadata(&b.metadata_xml).map_err(|_| v("error.saml_metadata"))?;
        if b.idp_entity_id.trim().is_empty() {
            b.idp_entity_id = md.entity_id;
        }
        if b.sso_url.trim().is_empty() {
            b.sso_url = md.sso_url;
        }
        if b.slo_url.trim().is_empty() {
            b.slo_url = md.slo_url;
        }
        if b.idp_certs.is_empty() {
            b.idp_certs = md.certs;
        }
    }
    let name = b.name.trim().to_string();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(v("error.saml_name"));
    }
    if b.idp_entity_id.trim().is_empty() || b.idp_entity_id.len() > 1024 {
        return Err(v("error.saml_entity_id"));
    }
    if !url_ok(b.sso_url.trim()) || (!b.slo_url.trim().is_empty() && !url_ok(b.slo_url.trim())) {
        return Err(v("error.saml_url"));
    }
    let mut certs = vec![];
    for c in b.idp_certs.iter().filter(|c| !c.trim().is_empty()) {
        let der = cert_der(c).map_err(|_| v("error.saml_cert"))?;
        let pem = cert_pem(&der);
        PublicKey::from_cert(&pem).map_err(|_| v("error.saml_cert"))?;
        certs.push(pem);
    }
    if certs.is_empty() || certs.len() > 4 {
        return Err(v("error.saml_cert"));
    }
    if !(0..=600).contains(&b.clock_skew_secs) {
        return Err(v("error.saml_skew"));
    }
    let gm = b.group_map.as_object().ok_or(v("error.saml_map"))?;
    if gm
        .values()
        .any(|x| x.as_str().and_then(|u| u.parse::<Uuid>().ok()).is_none())
    {
        return Err(v("error.saml_map"));
    }
    let rm = b.role_map.as_object().ok_or(v("error.saml_map"))?;
    if rm.values().any(|x| !x.is_string()) {
        return Err(v("error.saml_map"));
    }
    let attr = |x: String| x.trim().chars().take(255).collect::<String>();
    Ok(IdpInput {
        name,
        enabled: b.enabled,
        idp_entity_id: b.idp_entity_id.trim().into(),
        sso_url: b.sso_url.trim().into(),
        slo_url: b.slo_url.trim().into(),
        idp_certs: certs,
        metadata_url: b.metadata_url.trim().into(),
        sp_entity_id: b.sp_entity_id.trim().chars().take(1024).collect(),
        sign_requests: b.sign_requests,
        want_assertions_signed: b.want_assertions_signed,
        allow_idp_initiated: b.allow_idp_initiated,
        clock_skew_secs: b.clock_skew_secs,
        attr_email: attr(b.attr_email),
        attr_name: attr(b.attr_name),
        attr_first_name: attr(b.attr_first_name),
        attr_last_name: attr(b.attr_last_name),
        attr_groups: attr(b.attr_groups),
        group_map: b.group_map,
        role_map: b.role_map,
        nameid_is_email: b.nameid_is_email,
        allow_register: b.allow_register,
        link_by_verified_email: b.link_by_verified_email,
        allow_staff: b.allow_staff,
        button_label: b.button_label.trim().chars().take(100).collect(),
    })
}

fn idp_json(s: &AppState, i: &SamlIdp) -> Value {
    let u = urls(s, i);
    let mut v = serde_json::to_value(i).unwrap_or_default();
    v["sp"] = json!({ "entity_id": u.entity, "acs_url": u.acs, "slo_url": u.slo, "metadata_url": u.metadata, "login_url": u.login });
    v["has_sp_key"] = json!(i.sp_key_sealed.is_some());
    v
}

#[utoipa::path(get, path = "/api/v1/admin/saml/idps", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<SettingsManage>,
) -> ApiResult<Json<Value>> {
    let rows = db::list(&s.pool).await?;
    Ok(Json(
        json!({ "addon_enabled": addon_on(&s).await?, "idps": rows.iter().map(|i| idp_json(&s, i)).collect::<Vec<_>>() }),
    ))
}

#[utoipa::path(get, path = "/api/v1/admin/saml/idps/{id}", params(("id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn admin_get(
    State(s): State<AppState>,
    _p: RequirePermission<SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    let i = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(idp_json(&s, &i)))
}

#[utoipa::path(post, path = "/api/v1/admin/saml/idps", request_body = IdpBody, responses((status = 201), (status = 422)))]
pub async fn admin_create(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<SettingsManage>,
    Json(b): Json<IdpBody>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let input = to_input(&s, b).await?;
    let id = db::save(&s.pool, None, &input)
        .await?
        .ok_or(AppError::NotFound)?;
    if input.sign_requests {
        ensure_sp_key(&s, id, false).await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "saml.idp_created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "name": input.name }),
    )
    .await?;
    let i = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(idp_json(&s, &i))))
}

#[utoipa::path(put, path = "/api/v1/admin/saml/idps/{id}", params(("id" = Uuid, Path)), request_body = IdpBody, responses((status = 200), (status = 404), (status = 422)))]
pub async fn admin_update(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<SettingsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<IdpBody>,
) -> ApiResult<Json<Value>> {
    let input = to_input(&s, b).await?;
    db::save(&s.pool, Some(id), &input)
        .await?
        .ok_or(AppError::NotFound)?;
    if input.sign_requests {
        ensure_sp_key(&s, id, false).await?;
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "saml.idp_updated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    let i = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(idp_json(&s, &i)))
}

#[utoipa::path(delete, path = "/api/v1/admin/saml/idps/{id}", params(("id" = Uuid, Path)), responses((status = 204), (status = 404)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<StatusCode> {
    if !db::delete(&s.pool, id).await? {
        return Err(AppError::NotFound.into());
    }
    audit::record(
        &s.pool,
        Some(u.user_id),
        "saml.idp_deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(post, path = "/api/v1/admin/saml/idps/{id}/rotate-key", params(("id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn admin_rotate_key(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<SettingsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    ensure_sp_key(&s, id, true).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "saml.sp_key_rotated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    let i = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(idp_json(&s, &i)))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ValidateBody {
    /// Base64 `SAMLResponse` or raw XML.
    pub saml_response: String,
}

/// Dry run: validate a pasted response (signature, conditions, audience,
/// destination; InResponseTo is taken from the message) and show the mapped
/// attributes. Never logs anyone in and does not touch the replay cache.
#[utoipa::path(post, path = "/api/v1/admin/saml/idps/{id}/validate", params(("id" = Uuid, Path)), request_body = ValidateBody, responses((status = 200)))]
pub async fn admin_validate(
    State(s): State<AppState>,
    _p: RequirePermission<SettingsManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<ValidateBody>,
) -> ApiResult<Json<Value>> {
    let mut idp = db::get(&s.pool, id).await?.ok_or(AppError::NotFound)?;
    let raw = b.saml_response.trim();
    let xml = if raw.starts_with('<') {
        Ok(raw.to_string())
    } else {
        sp::decode_post(raw)
    };
    let res = xml.and_then(|x| {
        let irt = sp::peek_in_response_to(&x);
        idp.allow_idp_initiated = true;
        validate(&s, &idp, &x, irt.as_deref())
    });
    Ok(Json(match res {
        Ok(a) => diag(&idp, &a),
        Err(e) => json!({ "ok": false, "error": e.code() }),
    }))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct MetadataBody {
    #[serde(default)]
    pub xml: String,
    #[serde(default)]
    pub url: String,
}

#[utoipa::path(post, path = "/api/v1/admin/saml/metadata", request_body = MetadataBody, responses((status = 200), (status = 422)))]
pub async fn admin_parse_metadata(
    State(s): State<AppState>,
    _p: RequirePermission<SettingsManage>,
    Json(b): Json<MetadataBody>,
) -> ApiResult<Json<Value>> {
    let xml = if b.xml.trim().is_empty() {
        fetch_metadata(&s, b.url.trim()).await?
    } else {
        b.xml
    };
    let md =
        sp::parse_idp_metadata(&xml).map_err(|_| AppError::Validation("error.saml_metadata"))?;
    Ok(Json(
        json!({ "entity_id": md.entity_id, "sso_url": md.sso_url, "slo_url": md.slo_url, "certs": md.certs.iter().filter_map(|c| cert_der(c).ok()).map(|d| cert_pem(&d)).collect::<Vec<_>>() }),
    ))
}
