//! Pelty as an OIDC provider: discovery, admin client CRUD, consent,
//! authorization code + PKCE, ES256 id_token verified against JWKS, userinfo,
//! refresh rotation, code replay revocation, revocation endpoint, consents.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use common::*;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tower::ServiceExt;

async fn form(
    app: &axum::Router,
    uri: &str,
    body: &[(&str, &str)],
    basic: Option<(&str, &str)>,
) -> (StatusCode, Value) {
    let mut s = form_urlencoded::Serializer::new(String::new());
    for (k, v) in body {
        s.append_pair(k, v);
    }
    let mut b = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some((u, p)) = basic {
        b = b.header(
            "authorization",
            format!("Basic {}", STANDARD.encode(format!("{u}:{p}"))),
        );
    }
    let res = app
        .clone()
        .oneshot(b.body(Body::from(s.finish())).unwrap())
        .await
        .unwrap();
    let st = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (st, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn q(url: &str, key: &str) -> Option<String> {
    form_urlencoded::parse(url.split_once('?')?.1.as_bytes())
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
}

fn enc(s: &str) -> String {
    form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[tokio::test]
async fn oidc_provider_end_to_end() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let user = sess(&app, &state, "client").await;

    let d = call(&app, "GET", "/.well-known/openid-configuration", None, &[]).await;
    assert_eq!(d.status, StatusCode::OK);
    assert_eq!(d.body["code_challenge_methods_supported"], json!(["S256"]));
    let issuer = d.body["issuer"].as_str().unwrap().to_string();

    // ---- admin: validation + create (secret shown once)
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/oauth-clients",
        Some(json!({"name": "X", "redirect_uris": ["http://evil.example/cb"]})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        bad.body["error"]["code"], "error.oauth_redirect_invalid",
        "{:?}",
        bad.body
    );
    let forbidden = call(
        &app,
        "POST",
        "/api/v1/admin/oauth-clients",
        Some(json!({"name": "X", "redirect_uris": ["https://a.example/cb"]})),
        &user.h(),
    )
    .await;
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    let redirect = "https://app.example.com/callback";
    let c = call(&app, "POST", "/api/v1/admin/oauth-clients", Some(json!({"name": "Panel", "redirect_uris": [redirect], "scopes": ["openid", "email", "profile", "offline_access"]})), &admin.h()).await;
    assert_eq!(c.status, StatusCode::CREATED, "{:?}", c.body);
    let cid = c.body["client_id"].as_str().unwrap().to_string();
    let secret = c.body["client_secret"].as_str().unwrap().to_string();
    let id = c.body["id"].as_str().unwrap().to_string();
    let list = call(&app, "GET", "/api/v1/admin/oauth-clients", None, &admin.h()).await;
    assert!(
        list.body.to_string().contains(&cid) && !list.body.to_string().contains(&secret),
        "secret never listed"
    );

    // ---- authorize: unknown redirect is shown, not redirected
    let verifier = "v".repeat(50);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let base = format!("client_id={cid}&response_type=code&scope=openid%20email%20profile%20offline_access&state=st1&nonce=n-1&code_challenge={challenge}&code_challenge_method=S256");
    let r = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?{base}&redirect_uri={}",
            enc("https://evil.example/cb")
        ),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.oauth_redirect_mismatch");
    let r = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?{base}&redirect_uri={}",
            enc(redirect)
        ),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED, "needs a session");
    // no PKCE -> error back to the client
    let r = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?client_id={cid}&scope=openid&state=s&redirect_uri={}",
            enc(redirect)
        ),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(
        q(r.body["redirect_to"].as_str().unwrap(), "error").as_deref(),
        Some("invalid_request")
    );

    let info = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?{base}&redirect_uri={}",
            enc(redirect)
        ),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(info.body["consent_required"], true, "{:?}", info.body);
    assert_eq!(info.body["client"]["name"], "Panel");
    let mut params: Value = serde_json::from_str(&format!(
        r#"{{"client_id":"{cid}","redirect_uri":"{redirect}","response_type":"code","scope":"openid email profile offline_access","state":"st1","nonce":"n-1","code_challenge":"{challenge}","code_challenge_method":"S256"}}"#
    )).unwrap();
    params["approve"] = json!(false);
    let deny = call(
        &app,
        "POST",
        "/api/v1/oauth/authorize",
        Some(params.clone()),
        &user.h(),
    )
    .await;
    assert_eq!(
        q(deny.body["redirect_to"].as_str().unwrap(), "error").as_deref(),
        Some("access_denied")
    );
    params["approve"] = json!(true);
    let no_csrf = call(
        &app,
        "POST",
        "/api/v1/oauth/authorize",
        Some(params.clone()),
        &[("cookie", user.cookie.as_str())],
    )
    .await;
    assert_eq!(
        no_csrf.status,
        StatusCode::FORBIDDEN,
        "CSRF enforced on consent"
    );
    let ok = call(
        &app,
        "POST",
        "/api/v1/oauth/authorize",
        Some(params.clone()),
        &user.h(),
    )
    .await;
    let to = ok.body["redirect_to"].as_str().unwrap().to_string();
    assert!(to.starts_with(redirect), "{to}");
    assert_eq!(q(&to, "state").as_deref(), Some("st1"));
    assert_eq!(q(&to, "iss").as_deref(), Some(issuer.as_str()));
    let code = q(&to, "code").unwrap();

    // ---- token: wrong secret / wrong verifier
    let (st, b) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("code_verifier", &verifier),
        ],
        Some((&cid, "nope")),
    )
    .await;
    assert_eq!(
        (st, b["error"].as_str()),
        (StatusCode::UNAUTHORIZED, Some("invalid_client"))
    );

    let (st, tok) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("code_verifier", &verifier),
        ],
        Some((&cid, &secret)),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{tok:?}");
    let access = tok["access_token"].as_str().unwrap().to_string();
    let refresh = tok["refresh_token"].as_str().unwrap().to_string();
    let idt = tok["id_token"].as_str().unwrap().to_string();

    // ---- id_token: verify ES256 with the published JWKS
    let jwks = call(&app, "GET", "/oauth/jwks", None, &[]).await;
    let (input, sig) = idt.rsplit_once('.').unwrap();
    let (h64, p64) = input.split_once('.').unwrap();
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(h64).unwrap()).unwrap();
    let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(p64).unwrap()).unwrap();
    assert_eq!(header["alg"], "ES256");
    let k = jwks.body["keys"]
        .as_array()
        .unwrap()
        .iter()
        .find(|k| k["kid"] == header["kid"])
        .expect("kid in jwks");
    let mut point = vec![4u8];
    point.extend(URL_SAFE_NO_PAD.decode(k["x"].as_str().unwrap()).unwrap());
    point.extend(URL_SAFE_NO_PAD.decode(k["y"].as_str().unwrap()).unwrap());
    ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &point)
        .verify(input.as_bytes(), &URL_SAFE_NO_PAD.decode(sig).unwrap())
        .expect("id_token signature verifies");
    assert_eq!(claims["iss"], json!(issuer));
    assert_eq!(claims["aud"], json!(cid));
    assert_eq!(claims["nonce"], "n-1");
    assert_eq!(claims["email"], json!(user.email));
    let sub = claims["sub"].as_str().unwrap().to_string();

    // ---- userinfo
    let ui = call(
        &app,
        "GET",
        "/oauth/userinfo",
        None,
        &[("authorization", &format!("Bearer {access}"))],
    )
    .await;
    assert_eq!(ui.status, StatusCode::OK);
    assert_eq!(
        (ui.body["sub"].as_str(), ui.body["email"].as_str()),
        (Some(sub.as_str()), Some(user.email.as_str()))
    );
    let bad = call(
        &app,
        "GET",
        "/oauth/userinfo",
        None,
        &[("authorization", "Bearer pat_nope")],
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNAUTHORIZED);
    // OAuth tokens are not API keys
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("authorization", &format!("Bearer {access}"))],
    )
    .await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED);

    // ---- refresh rotates; old refresh token is dead
    let (st, r2) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
            ("client_id", &cid),
            ("client_secret", &secret),
        ],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r2:?}");
    assert_ne!(r2["refresh_token"], json!(refresh));
    let (st, b) = form(
        &app,
        "/oauth/token",
        &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
        Some((&cid, &secret)),
    )
    .await;
    assert_eq!(
        (st, b["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    let access2 = r2["access_token"].as_str().unwrap().to_string();

    // ---- code replay: invalid_grant and every token from that code revoked
    let (st, b) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", redirect),
            ("code_verifier", &verifier),
        ],
        Some((&cid, &secret)),
    )
    .await;
    assert_eq!(
        (st, b["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );
    for t in [&access, &access2] {
        let ui = call(
            &app,
            "GET",
            "/oauth/userinfo",
            None,
            &[("authorization", &format!("Bearer {t}"))],
        )
        .await;
        assert_eq!(ui.status, StatusCode::UNAUTHORIZED, "revoked after replay");
    }

    // ---- second login: consent remembered; bad PKCE verifier rejected
    let info = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?{base}&redirect_uri={}",
            enc(redirect)
        ),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(info.body["consent_required"], false);
    let ok = call(
        &app,
        "POST",
        "/api/v1/oauth/authorize",
        Some(params.clone()),
        &user.h(),
    )
    .await;
    let code2 = q(ok.body["redirect_to"].as_str().unwrap(), "code").unwrap();
    let (st, b) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &code2),
            ("redirect_uri", redirect),
            ("code_verifier", &"w".repeat(50)),
        ],
        Some((&cid, &secret)),
    )
    .await;
    assert_eq!(
        (st, b["error"].as_str()),
        (StatusCode::BAD_REQUEST, Some("invalid_grant"))
    );

    // ---- public client: no secret, PKCE only; revoke endpoint
    let pc = call(&app, "POST", "/api/v1/admin/oauth-clients", Some(json!({"name": "CLI", "redirect_uris": ["http://127.0.0.1:9999/cb"], "confidential": false})), &admin.h()).await;
    assert!(pc.body["client_secret"].is_null());
    let pcid = pc.body["client_id"].as_str().unwrap().to_string();
    let mut pp = params.clone();
    pp["client_id"] = json!(pcid);
    pp["redirect_uri"] = json!("http://127.0.0.1:9999/cb");
    pp["scope"] = json!("openid email");
    let ok = call(&app, "POST", "/api/v1/oauth/authorize", Some(pp), &user.h()).await;
    let pcode = q(ok.body["redirect_to"].as_str().unwrap(), "code").unwrap();
    let (st, ptok) = form(
        &app,
        "/oauth/token",
        &[
            ("grant_type", "authorization_code"),
            ("code", &pcode),
            ("redirect_uri", "http://127.0.0.1:9999/cb"),
            ("code_verifier", &verifier),
            ("client_id", &pcid),
        ],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{ptok:?}");
    assert!(
        ptok["refresh_token"].is_null(),
        "no offline_access, no refresh token"
    );
    let pat = ptok["access_token"].as_str().unwrap().to_string();
    let (st, _) = form(
        &app,
        "/oauth/revoke",
        &[("token", &pat), ("client_id", &pcid)],
        None,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    let ui = call(
        &app,
        "GET",
        "/oauth/userinfo",
        None,
        &[("authorization", &format!("Bearer {pat}"))],
    )
    .await;
    assert_eq!(ui.status, StatusCode::UNAUTHORIZED);

    // ---- user revokes the app; disabled client is rejected
    let apps = call(&app, "GET", "/api/v1/account/oauth-apps", None, &user.h()).await;
    assert_eq!(
        apps.body["apps"].as_array().unwrap().len(),
        2,
        "{:?}",
        apps.body
    );
    let r = call(
        &app,
        "DELETE",
        &format!("/api/v1/account/oauth-apps/{id}"),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let r = call(
        &app,
        "PATCH",
        &format!("/api/v1/admin/oauth-clients/{id}"),
        Some(json!({"enabled": false})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = call(
        &app,
        "GET",
        &format!(
            "/api/v1/oauth/authorize?{base}&redirect_uri={}",
            enc(redirect)
        ),
        None,
        &user.h(),
    )
    .await;
    assert_eq!(r.body["error"]["code"], "error.oauth_client_unknown");
    let rot = call(
        &app,
        "POST",
        &format!("/api/v1/admin/oauth-clients/{id}/secret"),
        None,
        &admin.h(),
    )
    .await;
    assert!(rot.body["client_secret"]
        .as_str()
        .unwrap()
        .starts_with("pcs_"));
    let del = call(
        &app,
        "DELETE",
        &format!("/api/v1/admin/oauth-clients/{id}"),
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(del.status, StatusCode::NO_CONTENT);
}
