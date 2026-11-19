//! PeltyCMS Premium client: 404 while the module is off, MythicCloud link
//! required, signed entitlement from MyMythical (mock transport), white-label
//! on the public site, soft verification (failures only warn) and switch-off
//! when MyMythical signs "inactive".
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use common::*;
use ring::signature::KeyPair;
use serde_json::{json, Value};

const SEED: [u8; 32] = [42u8; 32];

fn pair() -> ring::signature::Ed25519KeyPair {
    ring::signature::Ed25519KeyPair::from_seed_unchecked(&SEED).unwrap()
}

fn token(install_id: &str, active: bool, exp_in: i64) -> String {
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let claims = json!({
        "v": 1, "kid": "it-test", "iss": "mymythical", "product": "peltycms_premium",
        "install_id": install_id, "team_uuid": "team-1", "team_name": "Acme",
        "active": active, "plan": "premium", "reason": if active { "" } else { "no_subscription" },
        "entitlements": { "white_label": active, "premium_addons": active, "priority_support": active, "higher_limits": active },
        "seats": 1, "seats_used": 1,
        "manage_url": "https://my.mythicalsystems.org/acme/peltycms-premium",
        "support_url": "https://my.mythicalsystems.org/acme/support",
        "iat": now, "exp": now + exp_in,
    });
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    let sig = pair().sign(payload.as_bytes());
    format!("{payload}.{}", URL_SAFE_NO_PAD.encode(sig.as_ref()))
}

async fn del(st: &pelty_api::AppState, key: &str) {
    sqlx::query("DELETE FROM system_settings WHERE key = $1")
        .bind(key)
        .execute(&st.pool)
        .await
        .unwrap();
}

async fn status(app: &axum::Router, admin: &Sess) -> Value {
    let r = call(app, "GET", "/api/v1/admin/premium", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

async fn check(app: &axum::Router, admin: &Sess) -> Value {
    let r = call(app, "POST", "/api/v1/admin/premium/check", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body
}

async fn white_label(app: &axum::Router) -> Value {
    let r = call(app, "GET", "/api/v1/public/site", None, &[]).await;
    assert_eq!(r.status, StatusCode::OK);
    r.body["white_label"].clone()
}

#[tokio::test]
async fn premium_entitlement_soft_verification() {
    let pk = STANDARD.encode(pair().public_key().as_ref());
    std::env::set_var("PELTY_PREMIUM_PUBLIC_KEYS", format!("it-test={pk}"));
    std::env::set_var("PELTY_PREMIUM_URL", "https://mythic.test");
    let (app, st) = app().await;
    reset_modules(&st).await;
    for k in ["premium.state", "premium.prefs", "premium.addons"] {
        del(&st, k).await;
    }
    let prev_registry = pelty_database::settings::get(&st.pool, "addons.registry")
        .await
        .unwrap();
    let admin = sess(&app, &st, "admin").await;

    // Module off: API hidden, public site not white-labelled.
    let r = call(&app, "GET", "/api/v1/admin/premium", None, &admin.h()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(white_label(&app).await["hide_powered_by"], false);

    enable(&app, &admin, &["premium"]).await;
    let s = status(&app, &admin).await;
    assert_eq!(s["enabled"], true);
    assert_eq!(s["active"], false);
    assert_eq!(s["linked"], false);
    let install = s["install_id"].as_str().unwrap().to_string();

    // Not linked to MyMythical yet.
    let s = check(&app, &admin).await;
    assert_eq!(s["error"], "error.premium_not_linked");
    assert_eq!(s["active"], false);

    // White-label needs the entitlement.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/premium/settings",
        Some(json!({ "hide_powered_by": true, "source_url": "" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(
        r.body["error"]["code"], "error.premium_required",
        "{:?}",
        r.body
    );

    pelty_database::settings::set(
        &st.pool,
        "addons.registry",
        json!({ "url": "https://market.example.test/api/pelty/addons/registry.json",
                "panel_public_key": "pub-key", "panel_private_key": "priv-key",
                "team_uuid": "team-1", "team_name": "Acme" }),
    )
    .await
    .unwrap();

    let mock = pelty_payments::MockTransport::new();
    st.set_http(std::sync::Arc::new(mock.clone()));

    // A token for another install is refused (soft: still inactive, error kept).
    mock.push(
        200,
        json!({ "success": true, "data": { "token": token("other", true, 3600) } }).to_string(),
    );
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], false);
    assert_eq!(s["error"], "error.premium_install_mismatch");

    // Valid signed entitlement.
    mock.push(
        200,
        json!({ "success": true, "data": { "token": token(&install, true, 7 * 86400) } })
            .to_string(),
    );
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], true, "{s}");
    assert!(s["warning"].is_null());
    assert_eq!(s["entitlements"]["white_label"], true);
    assert_eq!(s["entitlements"]["premium_addons"], true);
    assert_eq!(
        s["support_url"],
        "https://my.mythicalsystems.org/acme/support"
    );
    assert_eq!(
        s["buy_url"],
        "https://my.mythicalsystems.org/acme/peltycms-premium"
    );
    let req = mock.requests().last().cloned().unwrap();
    assert_eq!(
        req.url,
        "https://mythic.test/api/pelty/cloud/premium/entitlement"
    );
    assert_eq!(req.header("x-panel-public-key"), Some("pub-key"));
    assert_eq!(req.header("x-panel-private-key"), Some("priv-key"));
    let body: Value = serde_json::from_str(&req.body).unwrap();
    assert_eq!(body["install_id"], install.as_str());

    // White-label on.
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/premium/settings",
        Some(json!({ "hide_powered_by": true, "source_url": "https://git.example.test/pelty" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let wl = white_label(&app).await;
    assert_eq!(wl["hide_powered_by"], true);
    assert_eq!(wl["source_url"], "https://git.example.test/pelty");
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/premium/settings",
        Some(json!({ "hide_powered_by": true, "source_url": "javascript:alert(1)" })),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY);

    // MyMythical down: Premium keeps working, warning only.
    mock.push(503, "unavailable");
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], true);
    assert_eq!(s["warning"], "check_failed");
    assert_eq!(s["error"], "error.premium_unavailable");
    assert_eq!(white_label(&app).await["hide_powered_by"], true);
    // A forged token is ignored the same way.
    mock.push(200, json!({ "data": { "token": "e30.AAAA" } }).to_string());
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], true);
    assert_eq!(s["warning"], "check_failed");

    // Expired cached token: still active, warning only.
    mock.push(
        200,
        json!({ "data": { "token": token(&install, true, -60) } }).to_string(),
    );
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], true);
    assert_eq!(s["warning"], "expired");

    // MyMythical signs "no subscription": Premium switches off.
    mock.push(
        200,
        json!({ "data": { "token": token(&install, false, 3600) } }).to_string(),
    );
    let s = check(&app, &admin).await;
    assert_eq!(s["active"], false);
    assert_eq!(s["reason"], "no_subscription");
    assert_eq!(white_label(&app).await["hide_powered_by"], false);

    // Customers cannot read the Premium status.
    let cust = sess(&app, &st, "customer").await;
    let r = call(&app, "GET", "/api/v1/admin/premium", None, &cust.h()).await;
    assert!(
        matches!(r.status, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED),
        "{:?}",
        r.status
    );

    // Cleanup.
    st.set_http(std::sync::Arc::new(pelty_payments::NoTransport));
    match prev_registry {
        Some(v) => pelty_database::settings::set(&st.pool, "addons.registry", v)
            .await
            .unwrap(),
        None => {
            del(&st, "addons.registry").await;
        }
    }
    for k in ["premium.state", "premium.prefs", "premium.addons"] {
        del(&st, k).await;
    }
    reset_modules(&st).await;
}
