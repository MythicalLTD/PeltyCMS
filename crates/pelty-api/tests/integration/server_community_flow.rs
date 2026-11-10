//! Server community: status refresh against a fake Minecraft server (Server
//! List Ping), vote sites with token-verified callbacks, cooldown, reward
//! queued through game delivery, leaderboards, goal, and 404 while off.
use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn varint(mut v: u32, out: &mut Vec<u8>) {
    while v & !0x7f != 0 {
        out.push(((v & 0x7f) | 0x80) as u8);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Answers one Server List Ping per connection.
async fn fake_minecraft() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = l.accept().await {
            tokio::spawn(async move {
                let mut buf = [0u8; 512];
                let _ = s.read(&mut buf).await;
                let body = r#"{"version":{"name":"Paper 1.21.1","protocol":767},"players":{"max":50,"online":3,"sample":[{"name":"Steve","id":"0"}]},"description":{"text":"Test MOTD"}}"#;
                let mut inner = vec![0u8];
                varint(body.len() as u32, &mut inner);
                inner.extend_from_slice(body.as_bytes());
                let mut out = vec![];
                varint(inner.len() as u32, &mut out);
                out.extend(inner);
                let _ = s.write_all(&out).await;
            });
        }
    });
    port
}

#[tokio::test]
async fn status_votes_leaderboards_and_goal() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let bob = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let player = format!("Steve{tag}");
    assert_eq!(
        call(&app, "GET", "/api/v1/public/community", None, &[])
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["game_profiles", "game_delivery"]).await;
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/modules/server_community",
        Some(json!({"enabled": true, "settings": {"goal_amount": "100", "goal_currency": "EUR", "goal_label": "Server costs", "show_donors": true, "show_donor_amounts": true}})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);

    let port = fake_minecraft().await;
    let srv = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(json!({"game": "minecraft_java", "name": "Survival", "slug": format!("surv-{tag}"), "host": "127.0.0.1", "port": port, "transport": "agent"})),
        &admin.h(),
    )
    .await;
    assert_eq!(srv.status, StatusCode::OK, "{:?}", srv.body);
    let sid = srv.body["id"].as_str().unwrap().to_string();

    let rf = call(
        &app,
        "POST",
        "/api/v1/admin/community/refresh",
        None,
        &admin.h(),
    )
    .await;
    assert_eq!(rf.status, StatusCode::OK, "{:?}", rf.body);
    let pubv = call(&app, "GET", "/api/v1/public/community", None, &[]).await;
    let me = pubv.body["servers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["server_id"] == sid.as_str())
        .cloned()
        .expect("server listed");
    assert_eq!(me["online"], true, "{me:?}");
    assert_eq!(me["players"], 3);
    assert_eq!(me["max_players"], 50);
    assert_eq!(me["motd"], "Test MOTD");
    assert_eq!(me["version"], "Paper 1.21.1");
    assert_eq!(me["sample"], json!(["Steve"]));
    assert_eq!(me["peak_players"], 3);
    assert_eq!(pubv.body["goal"]["amount"], "100");
    assert_eq!(pubv.body["goal"]["label"], "Server costs");

    // Vote site + token.
    let bad = call(
        &app,
        "POST",
        "/api/v1/admin/community/sites",
        Some(json!({"name": "List", "url": "javascript:alert(1)"})),
        &admin.h(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::UNPROCESSABLE_ENTITY);
    let site = call(
        &app,
        "POST",
        "/api/v1/admin/community/sites",
        Some(json!({"name": "MinecraftServers", "url": "https://vote.example/server/1?u={username}", "server_id": sid, "reward_command": "give {username} diamond 1"})),
        &admin.h(),
    )
    .await;
    assert_eq!(site.status, StatusCode::OK, "{:?}", site.body);
    let site_id = site.body["id"].as_str().unwrap().to_string();
    for bad in [
        "give {usernme} diamond 1",
        "give {username} 1\nop {username}",
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/admin/community/sites",
            Some(json!({"name": "Bad", "url": "https://vote.example/x", "reward_command": bad})),
            &admin.h(),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{bad}: {:?}",
            r.body
        );
    }
    let vurl = format!("/api/v1/public/community/vote/{site_id}");
    assert_eq!(
        call_from(
            &app,
            "198.51.100.9",
            "POST",
            &format!("{vurl}?username={player}&token=nope"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::FORBIDDEN,
        "no token issued yet"
    );
    let tok = call(
        &app,
        "POST",
        &format!("/api/v1/admin/community/sites/{site_id}/token"),
        None,
        &admin.h(),
    )
    .await;
    let token = tok.body["token"].as_str().unwrap().to_string();
    assert_eq!(tok.body["callback"], vurl.as_str());
    assert_eq!(
        call_from(
            &app,
            "198.51.100.9",
            "POST",
            &format!("{vurl}?username={player}&token=wrong"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call_from(
            &app,
            "198.51.100.9",
            "GET",
            &format!("{vurl}?username=%3Bop%20me&token={token}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "names with command characters are refused"
    );
    let v = call_from(
        &app,
        "198.51.100.9",
        "GET",
        &format!("{vurl}?user={player}&key={token}"),
        None,
        &[],
    )
    .await;
    assert_eq!(v.status, StatusCode::OK, "{:?}", v.body);
    assert_eq!(v.body["rewards"], 1);
    let again = call_from(
        &app,
        "198.51.100.9",
        "POST",
        &format!("{vurl}?username={}&token={token}", player.to_lowercase()),
        None,
        &[],
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT);
    assert_eq!(again.body["error"]["code"], "error.vote_cooldown");
    let cmd: String = sqlx::query_scalar(
        "SELECT command FROM game_deliveries WHERE trigger = 'vote' AND server_id = $1::uuid",
    )
    .bind(&sid)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(cmd, format!("give {player} diamond 1"));
    // A name with spaces must never add arguments on a Java server: the vote
    // counts, no command is queued.
    let multi = call_from(
        &app,
        "198.51.100.10",
        "GET",
        &format!("{vurl}?username=Bob{tag}%20parent%20add%20admin&token={token}"),
        None,
        &[],
    )
    .await;
    assert_eq!(multi.status, StatusCode::OK, "{:?}", multi.body);
    assert_eq!(multi.body["rewards"], 0);
    let n: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_deliveries WHERE trigger = 'vote' AND server_id = $1::uuid",
    )
    .bind(&sid)
    .fetch_one(&st.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);

    // Leaderboards and goal after a paid order.
    let g = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(json!({"name": "Ranks", "slug": format!("rk-{tag}")})),
        &admin.h(),
    )
    .await;
    let p = call(
        &app,
        "POST",
        "/api/v1/admin/catalog/products",
        Some(json!({"group_id": g.body["id"], "name": "VIP", "slug": format!("vip-{tag}")})),
        &admin.h(),
    )
    .await;
    let pid = p.body["id"].as_str().unwrap().to_string();
    call(
        &app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(json!({"prices": [{"currency": "EUR", "cycle": "one_time", "amount": "25.00"}]})),
        &admin.h(),
    )
    .await;
    let before = call(&app, "GET", "/api/v1/public/community", None, &[])
        .await
        .body["goal"]["raised"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    let o = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "one_time"}]})),
        &bob.h(),
    )
    .await;
    assert_eq!(o.status, StatusCode::OK, "{:?}", o.body);
    let iid = o.body["invoice"]["id"].as_str().unwrap().to_string();
    let total = o.body["invoice"]["total"].as_str().unwrap().to_string();
    let pay = call(
        &app,
        "POST",
        &format!("/api/v1/admin/invoices/{iid}/payments"),
        Some(json!({"amount": total, "idempotency_key": format!("cm-{iid}")})),
        &admin.h(),
    )
    .await;
    assert_eq!(pay.status, StatusCode::OK, "{:?}", pay.body);
    let pubv = call(&app, "GET", "/api/v1/public/community", None, &[]).await;
    let after = pubv.body["goal"]["raised"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(after >= before + 25.0, "{before} -> {after}");
    assert!(pubv.body["top_voters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["username"] == player.as_str() && v["votes"] == 1));
    let donors = pubv.body["top_donors"].as_array().unwrap();
    assert!(!donors.is_empty() && donors[0]["total"].is_string());
    assert!(
        pubv.body.to_string().find("@").is_none(),
        "no emails on the public page"
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/admin/modules/server_community",
        Some(json!({"enabled": true, "settings": {"goal_amount": "100"}})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let hidden = call(&app, "GET", "/api/v1/public/community", None, &[]).await;
    assert!(
        hidden.body["top_donors"].is_null(),
        "donors are opt-in: {:?}",
        hidden.body["top_donors"]
    );

    let adm = call(&app, "GET", "/api/v1/admin/community", None, &admin.h()).await;
    assert!(adm.body["votes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["username"] == player.as_str() && v["rewarded"] == 1));
    let mine = adm.body["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == site_id.as_str())
        .unwrap();
    assert_eq!(mine["has_token"], true);
    assert!(mine.get("token_hash").is_none());
    assert_eq!(
        call(&app, "GET", "/api/v1/admin/community", None, &bob.h())
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    reset_modules(&st).await;
    assert_eq!(
        call_from(
            &app,
            "198.51.100.9",
            "POST",
            &format!("{vurl}?username=Alex&token={token}"),
            None,
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
