//! Game-store sign-in: FiveM players through their Cfx.re forum account
//! (Discourse User API keys, `sso-cfx`) and Steam OpenID 2.0 (`sso-steam`),
//! with forum.cfx.re and steamcommunity.com mocked. Covers hidden-until-
//! enabled, the browser binding, payload/nonce/return_to tampering, the
//! "enter your e-mail" sign-up (neither provider shares one), login of a
//! known identity, linking from an account, and same-site `return_to`.
use crate::common;
use aws_lc_rs::rsa::{Pkcs1PublicEncryptingKey, PublicEncryptingKey};
use axum::http::StatusCode;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use common::*;
use serde_json::{json, Value};

const STEAM_OP: &str = "https://steamcommunity.com/openid/login";

fn query_of(url: &str) -> Vec<(String, String)> {
    form_urlencoded::parse(url.split_once('?').map(|x| x.1).unwrap_or("").as_bytes())
        .into_owned()
        .collect()
}
fn q(url: &str, k: &str) -> String {
    query_of(url)
        .into_iter()
        .find(|(n, _)| n == k)
        .unwrap_or_else(|| panic!("{k} in {url}"))
        .1
}
fn bind_of(r: &Resp) -> String {
    r.cookies
        .iter()
        .find(|c| c.starts_with("pelty_sso_bind="))
        .map(|c| c.split(';').next().unwrap().to_string())
        .expect("bind cookie")
}
fn session_of(r: &Resp) -> Option<String> {
    r.cookies
        .iter()
        .find(|c| c.starts_with("pelty_session=") && !c.starts_with("pelty_session=;"))
        .map(|c| c.split(';').next().unwrap().to_string())
}
fn err_of(r: &Resp) -> String {
    let l = r.location.clone().unwrap_or_default();
    l.split("oauth_error=").nth(1).unwrap_or("").to_string()
}
fn path_of(callback: &str) -> String {
    // Absolute public URL → path for the in-process router.
    let i = callback.find("/api/v1/").expect("api path");
    callback[i..].to_string()
}

async fn settings(app: &axum::Router, admin: &Sess, id: &str, v: Value) {
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/addons/{id}/settings"),
        Some(v),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
}

async fn provider_ids(app: &axum::Router) -> Vec<Value> {
    call(app, "GET", "/api/v1/auth/oauth/providers", None, &[])
        .await
        .body["providers"]
        .as_array()
        .unwrap()
        .clone()
}

/// Start a Cfx.re attempt: (forum URL, bind cookie).
async fn cfx_start(app: &axum::Router, extra: &str, h: &[(&str, &str)]) -> (String, String) {
    let r = call(
        app,
        "GET",
        &format!("/api/v1/auth/oauth/sso-cfx/start?format=json{extra}"),
        None,
        h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    (r.body["url"].as_str().unwrap().to_string(), bind_of(&r))
}

/// What forum.cfx.re does: encrypt `{key, nonce}` with the public key from
/// the URL (PKCS#1 v1.5), base64 with line breaks, append `&payload=`.
fn forum_redirect(url: &str, key: &str, nonce: &str) -> String {
    let pem = q(url, "public_key");
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    let der = STANDARD.decode(b64).unwrap();
    let pk = Pkcs1PublicEncryptingKey::new(PublicEncryptingKey::from_der(&der).unwrap()).unwrap();
    let plain = json!({"key": key, "nonce": nonce, "push": false, "api": 4}).to_string();
    let mut out = vec![0u8; pk.ciphertext_size()];
    let ct = pk.encrypt(plain.as_bytes(), &mut out).unwrap();
    let enc = STANDARD.encode(ct);
    // Ruby's Base64.encode64 breaks lines every 60 characters.
    let wrapped: String = enc
        .as_bytes()
        .chunks(60)
        .map(|c| format!("{}\n", std::str::from_utf8(c).unwrap()))
        .collect();
    let payload: String = form_urlencoded::byte_serialize(wrapped.as_bytes()).collect();
    format!("{}&payload={payload}", path_of(&q(url, "auth_redirect")))
}

fn steam_assertion(url: &str, steam_id: &str) -> String {
    let return_to = q(url, "openid.return_to");
    let claimed = format!("https://steamcommunity.com/openid/id/{steam_id}");
    let mut s = form_urlencoded::Serializer::new(String::new());
    s.append_pair("openid.ns", "http://specs.openid.net/auth/2.0")
        .append_pair("openid.mode", "id_res")
        .append_pair("openid.op_endpoint", STEAM_OP)
        .append_pair("openid.claimed_id", &claimed)
        .append_pair("openid.identity", &claimed)
        .append_pair("openid.return_to", &return_to)
        .append_pair("openid.response_nonce", "2026-11-11T22:00:00Zx1")
        .append_pair("openid.assoc_handle", "1234567890")
        .append_pair(
            "openid.signed",
            "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle",
        )
        .append_pair("openid.sig", "c2lnbmF0dXJl");
    // Steam appends its fields to return_to (which carries ?state=…).
    format!("{}&{}", path_of(&return_to), s.finish())
}

fn steam_id() -> String {
    format!(
        "7656119{:010}",
        uuid::Uuid::new_v4().as_u128() % 10_000_000_000
    )
}

#[tokio::test]
async fn game_sso() {
    let (app, state) = app().await;
    let mock = pelty_payments::MockTransport::new();
    state.set_http(std::sync::Arc::new(mock.clone()));
    let admin = sess(&app, &state, "admin").await;

    // Off until the admin opts in: no client id exists to "configure".
    for id in ["sso-cfx", "sso-steam"] {
        settings(&app, &admin, id, json!({"show_button": false})).await;
        assert!(provider_ids(&app).await.iter().all(|p| p["id"] != id));
        let r = call(
            &app,
            "GET",
            &format!("/api/v1/auth/oauth/{id}/start?format=json"),
            None,
            &[],
        )
        .await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{id} hidden");
    }

    // Game stores (FiveM, RedM): sign-ins fill the IDs they prove.
    crate::modules_flow::enable(&app, &admin, &["game_profiles"]).await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    for game in ["fivem", "redm"] {
        let r = call(&app, "POST", "/api/v1/admin/game/servers", Some(json!({"game": game, "name": format!("{game} {tag}"), "slug": format!("{game}-sso-{tag}"), "transport": "agent"})), &admin.h()).await;
        assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    }
    let accounts = |h: Vec<(String, String)>| {
        let app = app.clone();
        async move {
            let h: Vec<(&str, &str)> = h.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let r = call(&app, "GET", "/api/v1/client/game/accounts", None, &h).await;
            assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
            r.body["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    (
                        format!(
                            "{}.{}",
                            a["game"].as_str().unwrap(),
                            a["field"].as_str().unwrap()
                        ),
                        a["value"].as_str().unwrap().to_string(),
                    )
                })
                .collect::<std::collections::BTreeMap<String, String>>()
        }
    };
    let own = |h: &[(&str, &str)]| -> Vec<(String, String)> {
        h.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    };

    // ----- Cfx.re (FiveM) -----
    settings(
        &app,
        &admin,
        "sso-cfx",
        json!({"show_button": true, "application_name": "Pelty Test Store", "allow_register": true}),
    )
    .await;
    let prov = provider_ids(&app).await;
    let cfx = prov
        .iter()
        .find(|p| p["id"] == "sso-cfx")
        .expect("cfx shown");
    assert_eq!(
        (
            cfx["engine"].as_str(),
            cfx["featured"].as_bool(),
            cfx["custom_label"].as_bool()
        ),
        (Some("cfx"), Some(true), Some(false)),
        "{cfx:?}"
    );

    let (url, bind) = cfx_start(&app, "&return_to=/checkout", &[]).await;
    assert!(
        url.starts_with("https://forum.cfx.re/user-api-key/new?"),
        "{url}"
    );
    assert_eq!(q(&url, "scopes"), "session_info");
    assert_eq!(q(&url, "application_name"), "Pelty Test Store");
    assert!(q(&url, "public_key").starts_with("-----BEGIN PUBLIC KEY-----"));
    assert!(q(&url, "client_id").starts_with("pelty-"));
    let redirect = q(&url, "auth_redirect");
    assert!(
        redirect.starts_with(&format!(
            "{}/api/v1/auth/oauth/sso-cfx/callback?state=",
            state.cfg.public_url.trim_end_matches('/')
        )),
        "{redirect}"
    );
    // The key pair persists: a second attempt reuses client id + key.
    let (url_b, bind_b) = cfx_start(&app, "", &[]).await;
    assert_eq!(q(&url, "client_id"), q(&url_b, "client_id"));
    assert_eq!(q(&url, "public_key"), q(&url_b, "public_key"));

    // Callback without the browser-binding cookie: refused (and the state is spent).
    let nonce_b = q(&url_b, "nonce");
    let r = call(
        &app,
        "GET",
        &forum_redirect(&url_b, "uak-x", &nonce_b),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    assert_eq!(err_of(&r), "oauth_state");
    let r = call(
        &app,
        "GET",
        &forum_redirect(&url_b, "uak-x", &nonce_b),
        None,
        &[("cookie", bind_b.as_str())],
    )
    .await;
    assert_eq!(err_of(&r), "oauth_state", "state is single use");

    // Garbage payload / payload for another nonce: refused, no forum call.
    let before = mock.requests().len();
    let (u2, b2) = cfx_start(&app, "", &[]).await;
    let garbage = format!(
        "{}&payload={}",
        path_of(&q(&u2, "auth_redirect")),
        STANDARD.encode([7u8; 256])
    );
    let r = call(&app, "GET", &garbage, None, &[("cookie", b2.as_str())]).await;
    assert_eq!(err_of(&r), "oauth_payload");
    let (u3, b3) = cfx_start(&app, "", &[]).await;
    let r = call(
        &app,
        "GET",
        &forum_redirect(&u3, "uak-x", "not-the-nonce"),
        None,
        &[("cookie", b3.as_str())],
    )
    .await;
    assert_eq!(err_of(&r), "oauth_state");
    // Cancelled on the forum (no payload).
    let (u4, b4) = cfx_start(&app, "", &[]).await;
    let r = call(
        &app,
        "GET",
        &path_of(&q(&u4, "auth_redirect")),
        None,
        &[("cookie", b4.as_str())],
    )
    .await;
    assert_eq!(err_of(&r), "oauth_cancelled");
    assert_eq!(mock.requests().len(), before, "forum never asked");

    // Genuine: profile read with the key, then the key is revoked.
    let cfx_id = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as u64 + 1;
    mock.push(
        200,
        json!({"current_user": {"id": cfx_id, "username": "fivem_rp", "name": "RP Player"}})
            .to_string(),
    );
    mock.push(200, r#"{"success":"OK"}"#);
    let nonce = q(&url, "nonce");
    let r = call(
        &app,
        "GET",
        &forum_redirect(&url, "uak-1", &nonce),
        None,
        &[("cookie", bind.as_str())],
    )
    .await;
    assert_eq!(r.status, StatusCode::SEE_OTHER);
    let loc = r.location.clone().unwrap();
    assert!(
        loc.starts_with("/login?sso_signup="),
        "no e-mail from the forum: {loc}"
    );
    assert!(
        r.cookies.iter().any(|c| c.starts_with("pelty_sso_bind=;")),
        "bind cookie cleared: {:?}",
        r.cookies
    );
    let sent = &mock.requests()[before..];
    assert_eq!(sent[0].url, "https://forum.cfx.re/session/current.json");
    assert_eq!(sent[0].header("user-api-key"), Some("uak-1"));
    assert_eq!(
        (sent[1].method, sent[1].url.as_str()),
        ("POST", "https://forum.cfx.re/user-api-key/revoke")
    );
    let token = loc.split("sso_signup=").nth(1).unwrap().to_string();
    let info = call(
        &app,
        "GET",
        &format!("/api/v1/auth/oauth/signup/{token}"),
        None,
        &[],
    )
    .await;
    assert_eq!(info.status, StatusCode::OK, "{:?}", info.body);
    assert_eq!(info.body["name"], "RP Player");
    assert_eq!(info.body["provider"], "sso-cfx");

    // E-mail of an existing account: refused (sign in there and link).
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/oauth/signup",
        Some(json!({"token": token, "email": admin.email})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{:?}", r.body);
    assert_eq!(r.body["error"]["code"], "error.oauth_email_in_use");
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/oauth/signup",
        Some(json!({"token": token, "email": "not-an-email"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);

    let player = email();
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/oauth/signup",
        Some(json!({"token": token, "email": player})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["redirect"], "/checkout", "back to the store");
    let ck = session_of(&r).expect("signed in");
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("cookie", ck.as_str())],
    )
    .await;
    assert_eq!(
        me.body["user"]["email"]
            .as_str()
            .or(me.body["email"].as_str()),
        Some(player.as_str()),
        "{:?}",
        me.body
    );
    let ids = call(
        &app,
        "GET",
        "/api/v1/account/identities",
        None,
        &[("cookie", ck.as_str())],
    )
    .await;
    let idn = ids.body["identities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["provider"] == "sso-cfx")
        .cloned()
        .expect("cfx identity");
    assert_eq!(idn["subject"], cfx_id.to_string());
    assert_eq!(idn["username"], "fivem_rp");
    // Login with FiveM wrote the Cfx.re id (`fivem:`) on both stores' game
    // accounts; no Rockstar licence is invented (checkout asks for it).
    let acc = accounts(own(&[("cookie", ck.as_str())])).await;
    assert_eq!(acc.get("fivem.fivem"), Some(&cfx_id.to_string()), "{acc:?}");
    assert_eq!(acc.get("redm.fivem"), Some(&cfx_id.to_string()), "{acc:?}");
    assert!(!acc.contains_key("fivem.license"), "{acc:?}");
    // Sign-up token is single use.
    let r = call(
        &app,
        "POST",
        "/api/v1/auth/oauth/signup",
        Some(json!({"token": token, "email": email()})),
        &[],
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.oauth_state");

    // Same forum account again: straight in. A foreign return_to is ignored.
    let (u5, b5) = cfx_start(&app, "&return_to=https://evil.test/", &[]).await;
    mock.push(
        200,
        json!({"current_user": {"id": cfx_id, "username": "fivem_rp2"}}).to_string(),
    );
    mock.push(200, "{}");
    let r = call(
        &app,
        "GET",
        &forum_redirect(&u5, "uak-2", &q(&u5, "nonce")),
        None,
        &[("cookie", b5.as_str())],
    )
    .await;
    assert_eq!(
        r.location.as_deref(),
        Some("/dashboard"),
        "{:?}",
        r.location
    );
    assert!(session_of(&r).is_some());

    // Registration off: unknown forum account is not let in.
    settings(
        &app,
        &admin,
        "sso-cfx",
        json!({"show_button": true, "allow_register": false}),
    )
    .await;
    let (u6, b6) = cfx_start(&app, "", &[]).await;
    mock.push(
        200,
        json!({"current_user": {"id": cfx_id + 1_000_000_000, "username": "new"}}).to_string(),
    );
    mock.push(200, "{}");
    let r = call(
        &app,
        "GET",
        &forum_redirect(&u6, "uak-3", &q(&u6, "nonce")),
        None,
        &[("cookie", b6.as_str())],
    )
    .await;
    assert_eq!(err_of(&r), "oauth_not_linked");

    // ----- Steam -----
    settings(
        &app,
        &admin,
        "sso-steam",
        json!({"show_button": true, "api_key": "steam-web-key", "button_label": "Steam login"}),
    )
    .await;
    let prov = provider_ids(&app).await;
    let st = prov
        .iter()
        .find(|p| p["id"] == "sso-steam")
        .expect("steam shown");
    assert_eq!(
        (st["label"].as_str(), st["custom_label"].as_bool()),
        (Some("Steam login"), Some(true))
    );
    let cust = sess(&app, &state, "customer").await;
    let r = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-steam/start?format=json&mode=link",
        None,
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let surl = r.body["url"].as_str().unwrap().to_string();
    let sbind = bind_of(&r);
    assert!(surl.starts_with(STEAM_OP), "{surl}");
    assert_eq!(q(&surl, "openid.mode"), "checkid_setup");
    assert_eq!(
        q(&surl, "openid.realm"),
        format!("{}/", state.cfg.public_url.trim_end_matches('/'))
    );
    let sid = steam_id();
    // A SteamID the player already typed for FiveM is kept (never replaced
    // by a sign-in); RedM had none and gets the signed-in one.
    let typed = steam_id();
    let r = call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "fivem", "field": "steamid", "value": typed})),
        &cust.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let before = mock.requests().len();
    mock.push(200, "ns:http://specs.openid.net/auth/2.0\nis_valid:true\n");
    mock.push(
        200,
        json!({"response": {"players": [{"steamid": sid, "personaname": "Gordon"}]}}).to_string(),
    );
    let r = call(
        &app,
        "GET",
        &steam_assertion(&surl, &sid),
        None,
        &[("cookie", sbind.as_str())],
    )
    .await;
    assert_eq!(
        r.location.as_deref(),
        Some("/sessions?linked=sso-steam"),
        "{:?}",
        r.location
    );
    let sent = &mock.requests()[before..];
    assert_eq!((sent[0].method, sent[0].url.as_str()), ("POST", STEAM_OP));
    assert!(sent[0].body.contains("openid.mode=check_authentication"));
    assert!(sent[1].url.contains("GetPlayerSummaries") && sent[1].url.contains(&sid));
    let ids = call(&app, "GET", "/api/v1/account/identities", None, &cust.h()).await;
    let idn = ids.body["identities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["provider"] == "sso-steam")
        .cloned()
        .expect("steam linked");
    assert_eq!(
        (idn["subject"].as_str(), idn["username"].as_str()),
        (Some(sid.as_str()), Some("Gordon"))
    );
    let acc = accounts(own(&cust.h())).await;
    assert_eq!(acc.get("fivem.steamid"), Some(&typed), "{acc:?}");
    assert_eq!(acc.get("redm.steamid"), Some(&sid), "{acc:?}");

    // Steam login of that account.
    let steam_start = |extra: &'static str| {
        let app = app.clone();
        async move {
            let r = call(
                &app,
                "GET",
                &format!("/api/v1/auth/oauth/sso-steam/start?format=json{extra}"),
                None,
                &[],
            )
            .await;
            (r.body["url"].as_str().unwrap().to_string(), bind_of(&r))
        }
    };
    let (u, b) = steam_start("&return_to=/store").await;
    mock.push(200, "is_valid:true\n");
    mock.push(200, json!({"response": {"players": []}}).to_string());
    let r = call(
        &app,
        "GET",
        &steam_assertion(&u, &sid),
        None,
        &[("cookie", b.as_str())],
    )
    .await;
    assert_eq!(r.location.as_deref(), Some("/store"), "{:?}", r.location);
    let ck = session_of(&r).expect("steam session");
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("cookie", ck.as_str())],
    )
    .await;
    assert_eq!(
        me.body["user"]["email"]
            .as_str()
            .or(me.body["email"].as_str()),
        Some(cust.email.as_str())
    );

    // Steam says the signature is not valid.
    let (u, b) = steam_start("").await;
    mock.push(200, "ns:http://specs.openid.net/auth/2.0\nis_valid:false\n");
    let r = call(
        &app,
        "GET",
        &steam_assertion(&u, &sid),
        None,
        &[("cookie", b.as_str())],
    )
    .await;
    assert_eq!(err_of(&r), "oauth_failed");
    assert!(session_of(&r).is_none());
    // return_to pointing elsewhere (assertion minted for another attempt/site).
    let (u, b) = steam_start("").await;
    let (other, _) = steam_start("").await;
    let path = steam_assertion(&other, &sid).replacen(
        &path_of(&q(&other, "openid.return_to")),
        &path_of(&q(&u, "openid.return_to")),
        1,
    );
    let r = call(&app, "GET", &path, None, &[("cookie", b.as_str())]).await;
    assert_eq!(err_of(&r), "oauth_state");
    // A claimed id that is not Steam's: refused before asking Steam.
    let (u, b) = steam_start("").await;
    let before = mock.requests().len();
    let path = steam_assertion(&u, &sid).replace(
        "https%3A%2F%2Fsteamcommunity.com%2Fopenid%2Fid%2F",
        "https%3A%2F%2Fevil.test%2Fopenid%2Fid%2F",
    );
    let r = call(&app, "GET", &path, None, &[("cookie", b.as_str())]).await;
    assert_eq!(err_of(&r), "oauth_failed");
    assert_eq!(mock.requests().len(), before);
    // Cancelled at Steam.
    let (u, b) = steam_start("").await;
    let cancel = format!(
        "{}&openid.ns=http%3A%2F%2Fspecs.openid.net%2Fauth%2F2.0&openid.mode=cancel",
        path_of(&q(&u, "openid.return_to"))
    );
    let r = call(&app, "GET", &cancel, None, &[("cookie", b.as_str())]).await;
    assert_eq!(err_of(&r), "oauth_cancelled");

    // Unknown Steam account, registration on: asks for an e-mail.
    let (u, b) = steam_start("").await;
    mock.push(200, "is_valid:true\n");
    mock.push(200, json!({"response": {"players": []}}).to_string());
    let r = call(
        &app,
        "GET",
        &steam_assertion(&u, &steam_id()),
        None,
        &[("cookie", b.as_str())],
    )
    .await;
    assert!(
        r.location
            .as_deref()
            .unwrap_or("")
            .starts_with("/login?sso_signup="),
        "{:?}",
        r.location
    );

    // Disabling the addon hides it and blocks start.
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-steam/disable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());
    assert!(provider_ids(&app)
        .await
        .iter()
        .all(|p| p["id"] != "sso-steam"));
    let r = call(
        &app,
        "GET",
        "/api/v1/auth/oauth/sso-steam/start?format=json",
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-steam/enable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());

    // Leave both off for other suites.
    for id in ["sso-cfx", "sso-steam"] {
        settings(&app, &admin, id, json!({"show_button": false})).await;
    }
}
