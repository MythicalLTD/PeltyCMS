//! SAML SP end to end against a simulated IdP (pelty_saml::testing::TestIdp):
//! admin setup from metadata, SP metadata, SP-initiated login with
//! InResponseTo, auto-provisioning, replay, wrong audience, forged signature,
//! unknown request id, link by verified email, group mapping, SLO and the
//! addon kill switch.
use crate::common;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use base64::Engine;
use common::*;
use http_body_util::BodyExt;
use pelty_saml::testing::{ResponseSpec, TestIdp};
use serde_json::json;
use tower::ServiceExt;

struct Out {
    status: StatusCode,
    location: String,
    cookie: Option<String>,
    body: String,
}

async fn raw(
    app: &axum::Router,
    method: &str,
    uri: &str,
    form: Option<String>,
    cookie: Option<&str>,
) -> Out {
    let mut b = Request::builder().method(method).uri(uri);
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    let req = match form {
        Some(f) => b
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(f)),
        None => b.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let location = res
        .headers()
        .get("location")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let cookie = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|c| c.starts_with("pelty_session="))
        .map(|c| c.split(';').next().unwrap().to_string());
    let body =
        String::from_utf8_lossy(&res.into_body().collect().await.unwrap().to_bytes()).into_owned();
    Out {
        status,
        location,
        cookie,
        body,
    }
}

async fn post_acs(app: &axum::Router, id: &str, xml: &str) -> Out {
    let b64 = base64::engine::general_purpose::STANDARD.encode(xml);
    let f: String = form_urlencoded::Serializer::new(String::new())
        .append_pair("SAMLResponse", &b64)
        .finish();
    raw(
        app,
        "POST",
        &format!("/api/v1/sso/saml/{id}/acs"),
        Some(f),
        None,
    )
    .await
}

fn q(url: &str, k: &str) -> String {
    form_urlencoded::parse(url.split_once('?').unwrap().1.as_bytes())
        .find(|(n, _)| n == k)
        .unwrap()
        .1
        .into_owned()
}

async fn start(app: &axum::Router, id: &str) -> String {
    let r = call(
        app,
        "GET",
        &format!("/api/v1/sso/saml/{id}/login?format=json"),
        None,
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let url = r.body["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://idp.test/sso?SAMLRequest="),
        "{url}"
    );
    let xml = pelty_saml::sp::inflate(&q(url, "SAMLRequest")).unwrap();
    let rid = r.body["request_id"].as_str().unwrap().to_string();
    assert!(
        xml.contains(&format!("ID=\"{rid}\"")) && xml.contains("AuthnRequest"),
        "{xml}"
    );
    rid
}

#[tokio::test]
async fn saml_sp_flow() {
    let (app, state) = app().await;
    let admin = sess(&app, &state, "admin").await;
    let ent = format!("https://idp.test/{}", uuid::Uuid::new_v4().simple());
    let idp = TestIdp::new(&ent, false).unwrap();
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-saml/enable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());

    // Customers cannot manage IdPs.
    let cust = sess(&app, &state, "customer").await;
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/saml/idps", None, &cust.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Metadata parse, then create from pasted XML.
    let md = idp.metadata("https://idp.test/sso", "https://idp.test/slo");
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/saml/metadata",
        Some(json!({"xml": md})),
        &admin.h(),
    )
    .await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    assert_eq!(p.body["entity_id"], ent.as_str());
    assert_eq!(p.body["sso_url"], "https://idp.test/sso");
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/saml/metadata",
            Some(json!({"xml": "<x/>"})),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );

    let gname = format!("saml-{}", uuid::Uuid::new_v4().simple());
    let g = call(
        &app,
        "POST",
        "/api/v1/admin/client-groups",
        Some(json!({"name": gname, "color": "violet", "discount_percent": "0"})),
        &admin.h(),
    )
    .await;
    let gid = g.body["group"]["id"].as_str().expect("group").to_string();
    let c = call(&app, "POST", "/api/v1/admin/saml/idps", Some(json!({
        "name": "Corp IdP", "metadata_xml": md, "allow_register": true, "link_by_verified_email": true,
        "group_map": {"vip": gid}, "sign_requests": true,
    })), &admin.h()).await;
    assert_eq!(c.status, StatusCode::CREATED, "{:?}", c.body);
    let id = c.body["id"].as_str().unwrap().to_string();
    assert_eq!(c.body["has_sp_key"], true);
    let acs = c.body["sp"]["acs_url"].as_str().unwrap().to_string();
    let sp_ent = c.body["sp"]["entity_id"].as_str().unwrap().to_string();
    assert!(acs.ends_with(&format!("/sso/saml/{id}/acs")));

    // SP metadata is public and carries the ACS + signing cert.
    let m = raw(
        &app,
        "GET",
        &format!("/api/v1/sso/saml/{id}/metadata"),
        None,
        None,
    )
    .await;
    assert_eq!(m.status, StatusCode::OK);
    assert!(
        m.body.contains(&acs)
            && m.body.contains("X509Certificate")
            && m.body.contains("AuthnRequestsSigned=\"true\""),
        "{}",
        m.body
    );
    let prov = call(&app, "GET", "/api/v1/auth/saml/providers", None, &[]).await;
    assert!(prov.body["providers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == id.as_str() && p["label"] == "Corp IdP"));

    // Signed AuthnRequest (redirect binding carries Signature + SigAlg).
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/sso/saml/{id}/login?format=json"),
        None,
        &[],
    )
    .await;
    let url = r.body["url"].as_str().unwrap();
    assert!(
        url.contains("&SigAlg=") && url.contains("&Signature="),
        "{url}"
    );

    // ---- happy path: auto-provision with group mapping ----
    let mail = email();
    let admin_cust = call(
        &app,
        "POST",
        "/api/v1/admin/customers",
        Some(json!({"email": mail, "first_name": "Pre"})),
        &admin.h(),
    )
    .await;
    assert!(admin_cust.status.is_success(), "{:?}", admin_cust.body);
    let rid = start(&app, &id).await;
    let mut spec = ResponseSpec::new(
        &ent,
        &sp_ent,
        &acs,
        &format!("nid-{}", uuid::Uuid::new_v4().simple()),
        Some(&rid),
    );
    spec.attributes = vec![
        ("email".into(), vec![mail.clone()]),
        ("displayName".into(), vec!["Ana Pop".into()]),
        ("groups".into(), vec!["vip".into()]),
    ];
    let ok_xml = idp.response(&spec).unwrap();
    let o = post_acs(&app, &id, &ok_xml).await;
    assert_eq!(
        (o.status, o.location.as_str()),
        (StatusCode::SEE_OTHER, "/dashboard"),
        "{}",
        o.body
    );
    let cookie = o.cookie.expect("session");
    let me = call(
        &app,
        "GET",
        "/api/v1/auth/me",
        None,
        &[("cookie", cookie.as_str())],
    )
    .await;
    assert_eq!(
        me.body["user"]["email"]
            .as_str()
            .or(me.body["email"].as_str()),
        Some(mail.as_str()),
        "{:?}",
        me.body
    );
    let cg: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT client_group_id FROM customers WHERE email = $1::citext")
            .bind(&mail)
            .fetch_one(&state.pool)
            .await
            .unwrap();
    assert_eq!(
        cg.map(|u| u.to_string()),
        Some(gid.clone()),
        "IdP group mapped to client group"
    );

    // ---- replay of the same response is rejected ----
    let o = post_acs(&app, &id, &ok_xml).await;
    assert_eq!(o.location, "/login?saml_error=saml_failed");
    assert!(o.cookie.is_none());

    // ---- negative cases ----
    let neg = |f: &dyn Fn(&mut ResponseSpec)| {
        let mut s = ResponseSpec::new(&ent, &sp_ent, &acs, "neg", None);
        f(&mut s);
        s
    };
    for (what, mk) in [
        (
            "wrong audience",
            neg(&|s| s.audience = "https://evil.test".into()),
        ),
        (
            "wrong destination",
            neg(&|s| s.destination = "https://evil.test/acs".into()),
        ),
        (
            "wrong issuer",
            neg(&|s| s.issuer = "https://evil.test".into()),
        ),
        (
            "expired",
            neg(&|s| {
                s.issued -= time::Duration::hours(2);
            }),
        ),
        ("unsigned", neg(&|s| s.sign_assertion = false)),
    ] {
        let rid = start(&app, &id).await;
        let mut s = mk;
        s.in_response_to = Some(rid);
        let o = post_acs(&app, &id, &idp.response(&s).unwrap()).await;
        assert_eq!(o.location, "/login?saml_error=saml_failed", "{what}");
        assert!(o.cookie.is_none(), "{what}");
    }
    // Signed by another key.
    let evil = TestIdp::new(&ent, true).unwrap();
    let rid = start(&app, &id).await;
    let o = post_acs(
        &app,
        &id,
        &evil
            .response(&ResponseSpec::new(&ent, &sp_ent, &acs, "x", Some(&rid)))
            .unwrap(),
    )
    .await;
    assert!(
        o.cookie.is_none() && o.location.contains("saml_error"),
        "foreign key"
    );
    // Tampered after signing.
    let rid = start(&app, &id).await;
    let mut s = ResponseSpec::new(&ent, &sp_ent, &acs, "victim", Some(&rid));
    s.attributes = vec![("email".into(), vec!["a@x.test".into()])];
    let t = idp.response(&s).unwrap().replace("a@x.test", "b@x.test");
    assert!(post_acs(&app, &id, &t).await.cookie.is_none(), "tampered");
    // Unknown request id / unsolicited (IdP-initiated is off).
    let o = post_acs(
        &app,
        &id,
        &idp.response(&ResponseSpec::new(&ent, &sp_ent, &acs, "x", Some("_nope")))
            .unwrap(),
    )
    .await;
    assert!(o.cookie.is_none());
    let o = post_acs(
        &app,
        &id,
        &idp.response(&ResponseSpec::new(&ent, &sp_ent, &acs, "x", None))
            .unwrap(),
    )
    .await;
    assert!(o.cookie.is_none(), "unsolicited rejected");
    // Request ids are single-use.
    let rid = start(&app, &id).await;
    let mut s = ResponseSpec::new(
        &ent,
        &sp_ent,
        &acs,
        &format!("n-{}", uuid::Uuid::new_v4().simple()),
        Some(&rid),
    );
    s.attributes = vec![("email".into(), vec![email()])];
    assert!(post_acs(&app, &id, &idp.response(&s).unwrap())
        .await
        .cookie
        .is_some());
    s.assertion_id = pelty_saml::sp::new_id();
    s.response_id = pelty_saml::sp::new_id();
    assert!(
        post_acs(&app, &id, &idp.response(&s).unwrap())
            .await
            .cookie
            .is_none(),
        "InResponseTo reused"
    );

    // ---- link existing account by verified email ----
    let local = sess(&app, &state, "customer").await;
    sqlx::query("UPDATE users SET email_verified_at = now() WHERE email = $1::citext")
        .bind(&local.email)
        .execute(&state.pool)
        .await
        .unwrap();
    let rid = start(&app, &id).await;
    let mut s = ResponseSpec::new(&ent, &sp_ent, &acs, "link-me", Some(&rid));
    s.name_id = format!("link-{}", uuid::Uuid::new_v4().simple());
    s.attributes = vec![("email".into(), vec![local.email.clone()])];
    let o = post_acs(&app, &id, &idp.response(&s).unwrap()).await;
    assert!(
        o.cookie.is_some(),
        "linked by verified email: {}",
        o.location
    );
    // Unverified local account is NOT taken over.
    let unv = sess(&app, &state, "customer").await;
    sqlx::query("UPDATE users SET email_verified_at = NULL WHERE email = $1::citext")
        .bind(&unv.email)
        .execute(&state.pool)
        .await
        .unwrap();
    let rid = start(&app, &id).await;
    let mut s = ResponseSpec::new(
        &ent,
        &sp_ent,
        &acs,
        &format!("u-{}", uuid::Uuid::new_v4().simple()),
        Some(&rid),
    );
    s.attributes = vec![("email".into(), vec![unv.email.clone()])];
    let o = post_acs(&app, &id, &idp.response(&s).unwrap()).await;
    assert!(
        o.cookie.is_none() && o.location.contains("oauth_email_in_use"),
        "{}",
        o.location
    );
    // Staff are refused unless allow_staff.
    let rid = start(&app, &id).await;
    let mut s = ResponseSpec::new(
        &ent,
        &sp_ent,
        &acs,
        &format!("a-{}", uuid::Uuid::new_v4().simple()),
        Some(&rid),
    );
    s.attributes = vec![("email".into(), vec![admin.email.clone()])];
    assert!(
        post_acs(&app, &id, &idp.response(&s).unwrap())
            .await
            .cookie
            .is_none(),
        "staff takeover refused"
    );

    // ---- admin dry-run validate ----
    let mut s = ResponseSpec::new(&ent, &sp_ent, &acs, "dry", None);
    s.attributes = vec![("email".into(), vec!["dry@x.test".into()])];
    let v = call(
        &app,
        "POST",
        &format!("/api/v1/admin/saml/idps/{id}/validate"),
        Some(json!({"saml_response": idp.response(&s).unwrap()})),
        &admin.h(),
    )
    .await;
    assert_eq!(
        (v.body["ok"].as_bool(), v.body["mapped"]["email"].as_str()),
        (Some(true), Some("dry@x.test")),
        "{:?}",
        v.body
    );
    s.audience = "nope".into();
    let v = call(
        &app,
        "POST",
        &format!("/api/v1/admin/saml/idps/{id}/validate"),
        Some(json!({"saml_response": idp.response(&s).unwrap()})),
        &admin.h(),
    )
    .await;
    assert_eq!(v.body["ok"], false, "{:?}", v.body);

    // ---- SLO: IdP-initiated LogoutRequest (signed redirect) revokes the session ----
    let lr = pelty_saml::sp::logout_request(
        &pelty_saml::sp::new_id(),
        &pelty_saml::sp::Sp {
            entity_id: &ent,
            acs_url: "",
            slo_url: "",
        },
        &format!("https://x/sso/saml/{id}/slo"),
        &spec.name_id,
        None,
        time::OffsetDateTime::now_utc(),
    );
    let unsigned =
        pelty_saml::sp::redirect_url("https://x/slo", "SAMLRequest", &lr, None, None).unwrap();
    let o = raw(
        &app,
        "GET",
        &format!(
            "/api/v1/sso/saml/{id}/slo?{}",
            unsigned.split_once('?').unwrap().1
        ),
        None,
        None,
    )
    .await;
    assert_eq!(
        o.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "unsigned logout refused: {}",
        o.body
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/auth/me",
            None,
            &[("cookie", cookie.as_str())]
        )
        .await
        .status,
        StatusCode::OK
    );
    let signed = pelty_saml::sp::redirect_url(
        "https://x/slo",
        "SAMLRequest",
        &lr,
        Some("rs"),
        Some(&idp.key),
    )
    .unwrap();
    let o = raw(
        &app,
        "GET",
        &format!(
            "/api/v1/sso/saml/{id}/slo?{}",
            signed.split_once('?').unwrap().1
        ),
        None,
        None,
    )
    .await;
    assert_eq!(o.status, StatusCode::SEE_OTHER, "{}", o.body);
    assert!(
        o.location.starts_with("https://idp.test/slo?SAMLResponse="),
        "{}",
        o.location
    );
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/auth/me",
            None,
            &[("cookie", cookie.as_str())]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );

    // ---- addon disabled: everything public 404s ----
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-saml/disable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());
    assert_eq!(
        raw(
            &app,
            "GET",
            &format!("/api/v1/sso/saml/{id}/metadata"),
            None,
            None
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "GET",
            &format!("/api/v1/sso/saml/{id}/login"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert!(call(&app, "GET", "/api/v1/auth/saml/providers", None, &[])
        .await
        .body["providers"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(call(
        &app,
        "POST",
        "/api/v1/admin/addons/sso-saml/enable",
        None,
        &admin.h()
    )
    .await
    .status
    .is_success());
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/saml/idps/{id}"),
            None,
            &admin.h()
        )
        .await
        .status
        .as_u16()
            / 100,
        2
    );
}
