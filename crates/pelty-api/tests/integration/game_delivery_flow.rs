use crate::common;
use crate::modules_flow::{enable, reset_modules};
use axum::http::StatusCode;
use common::*;
use pelty_extensions::events::DomainEvent;
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Write-only server field that carries the RCON credential.
const RCON_FIELD: &str = "rcon_password";

/// Fake Source RCON server accepting `secret`; records every executed command.
async fn fake_rcon(secret: String) -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    let log = Arc::new(Mutex::new(vec![]));
    let log2 = log.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut c, _)) = l.accept().await else {
                return;
            };
            let log = log2.clone();
            let secret = secret.clone();
            tokio::spawn(async move {
                loop {
                    let Ok(len) = c.read_i32_le().await else {
                        return;
                    };
                    let mut b = vec![0u8; len as usize];
                    if c.read_exact(&mut b).await.is_err() {
                        return;
                    }
                    let id = i32::from_le_bytes(b[0..4].try_into().unwrap());
                    let kind = i32::from_le_bytes(b[4..8].try_into().unwrap());
                    let body = String::from_utf8_lossy(&b[8..b.len() - 2]).to_string();
                    let (rid, rkind, out): (i32, i32, String) = match kind {
                        3 => (if body == secret { id } else { -1 }, 2, String::new()),
                        _ => {
                            log.lock().unwrap().push(body.clone());
                            (id, 0, format!("ok:{body}"))
                        }
                    };
                    let mut p = vec![];
                    p.extend_from_slice(&((out.len() + 10) as i32).to_le_bytes());
                    p.extend_from_slice(&rid.to_le_bytes());
                    p.extend_from_slice(&rkind.to_le_bytes());
                    p.extend_from_slice(out.as_bytes());
                    p.extend_from_slice(&[0, 0]);
                    if c.write_all(&p).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (port, log)
}

async fn wait_for(log: &Arc<Mutex<Vec<String>>>, needle: &str) -> bool {
    for _ in 0..100 {
        if log.lock().unwrap().iter().any(|c| c == needle) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    false
}

/// Purchase → RCON command with the linked Minecraft name; FiveM agent
/// commands wait for the license and for the player to be online; expiry
/// runs the remove command.
#[tokio::test]
async fn rcon_and_agent_delivery() {
    let (app, st) = app().await;
    reset_modules(&st).await;
    let admin = sess(&app, &st, "admin").await;
    let alice = sess(&app, &st, "customer").await;
    let tag = &uuid::Uuid::new_v4().simple().to_string()[..8];
    let rcon_secret = uuid::Uuid::new_v4().simple().to_string();
    let (port, log) = fake_rcon(rcon_secret.clone()).await;

    // Off: queue routes and the agent API do not exist.
    assert_eq!(
        call(
            &app,
            "GET",
            "/api/v1/admin/game/delivery/queue",
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["game_profiles"]).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/game-agent/poll",
            Some(json!({})),
            &[]
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    enable(&app, &admin, &["game_delivery"]).await;

    let mut mc_body = json!({
        "game": "minecraft_java", "name": "MC", "slug": format!("mc-{tag}"), "transport": "rcon",
        "rcon_host": "127.0.0.1", "rcon_port": port
    });
    mc_body[RCON_FIELD] = json!(rcon_secret);
    let mc = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(mc_body),
        &admin.h(),
    )
    .await;
    assert_eq!(mc.status, StatusCode::OK, "{:?}", mc.body);
    let mc = mc.body["id"].as_str().unwrap().to_string();
    let fivem = call(
        &app,
        "POST",
        "/api/v1/admin/game/servers",
        Some(json!({
            "game": "fivem", "name": "RP", "slug": format!("rp-{tag}"), "transport": "agent"
        })),
        &admin.h(),
    )
    .await
    .body["id"]
        .as_str()
        .unwrap()
        .to_string();
    let token = call(
        &app,
        "POST",
        &format!("/api/v1/admin/game/servers/{fivem}/agent-token"),
        None,
        &admin.h(),
    )
    .await
    .body["token"]
        .as_str()
        .unwrap()
        .to_string();
    let agent = [("authorization", format!("Bearer {token}"))];
    let agent_h: Vec<(&str, &str)> = agent.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/game/delivery/servers/{mc}/test"),
        Some(json!({"command": "list"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["ok"], true, "{:?}", r.body);
    assert_eq!(r.body["response"], "ok:list");

    let (pid, oid, ch) = seed_product(&app, &admin).await;
    for s in [&mc, &fivem] {
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/game/servers/{s}/products"),
            Some(json!({"product_ids": [pid]})),
            &admin.h(),
        )
        .await;
    }
    let bad = json!({"commands": [{"trigger": "purchase", "command": "op {nope}"}]});
    assert_eq!(
        call(
            &app,
            "PUT",
            &format!("/api/v1/admin/game/delivery/products/{pid}/commands"),
            Some(bad),
            &admin.h()
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(&app, "PUT", &format!("/api/v1/admin/game/delivery/products/{pid}/commands"), Some(json!({"commands": [
        {"server_id": mc, "trigger": "purchase", "command": "lp user {username} parent add vip"},
        {"server_id": fivem, "trigger": "purchase", "command": "add_principal identifier.license:{license} group.vip", "require_online": true},
        {"server_id": mc, "trigger": "expiry", "command": "lp user {username} parent remove vip"}
    ]})), &admin.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["items"].as_array().unwrap().len(), 3);

    call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "minecraft_java", "field": "username", "value": "Steve"})),
        &alice.h(),
    )
    .await;
    let o = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [{"product_id": pid, "cycle": "monthly", "config": {oid: ch[0]}}]})), &alice.h()).await;
    let order_id = o.body["order"]["id"].as_str().unwrap().to_string();
    let ap = call(
        &app,
        "POST",
        &format!("/api/v1/admin/orders/{order_id}/approve"),
        None,
        &admin.h(),
    )
    .await;
    let sid = ap.body["services"][0].as_str().unwrap().to_string();

    assert!(
        wait_for(&log, "lp user Steve parent add vip").await,
        "{:?}",
        log.lock().unwrap()
    );
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/game/delivery/queue?service_id={sid}"),
        None,
        &admin.h(),
    )
    .await;
    let items = r.body["items"].as_array().unwrap().clone();
    assert_eq!(items.len(), 2, "{items:?}");
    let fivem_row = items
        .iter()
        .find(|d| d["server_id"] == fivem.as_str())
        .unwrap();
    assert_eq!(fivem_row["status"], "waiting_identity");
    for _ in 0..50 {
        let r = call(
            &app,
            "GET",
            "/api/v1/client/game/deliveries",
            None,
            &alice.h(),
        )
        .await;
        if r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["status"] == "delivered")
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let r = call(
        &app,
        "GET",
        "/api/v1/client/game/deliveries",
        None,
        &alice.h(),
    )
    .await;
    assert!(
        r.body["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["status"] == "delivered" && d["player"] == "Steve"),
        "{:?}",
        r.body
    );
    assert!(r.body["items"][0].get("command").is_none());

    // Agent: bad token, then nothing until the license is linked and the player is online.
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/game-agent/poll",
            Some(json!({})),
            &[("authorization", "Bearer pga_nope")]
        )
        .await
        .status,
        StatusCode::UNAUTHORIZED
    );
    let lic = "abcdef0123456789abcdef0123456789abcdef01";
    call(
        &app,
        "POST",
        "/api/v1/client/game/accounts",
        Some(json!({"game": "fivem", "field": "license", "value": format!("license:{lic}")})),
        &alice.h(),
    )
    .await;
    let r = call(
        &app,
        "POST",
        "/api/v1/game-agent/poll",
        Some(json!({})),
        &agent_h,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["commands"], json!([]));
    let r = call(
        &app,
        "POST",
        "/api/v1/game-agent/poll",
        Some(json!({"online": [lic]})),
        &agent_h,
    )
    .await;
    let cmds = r.body["commands"].as_array().unwrap();
    assert_eq!(cmds.len(), 1, "{:?}", r.body);
    assert_eq!(
        cmds[0]["command"],
        format!("add_principal identifier.license:{lic} group.vip")
    );
    let did = cmds[0]["id"].as_str().unwrap();
    // Leased: a second poll does not hand it out again.
    let r = call(
        &app,
        "POST",
        "/api/v1/game-agent/poll",
        Some(json!({"online": [lic]})),
        &agent_h,
    )
    .await;
    assert_eq!(r.body["commands"], json!([]));
    let r = call(
        &app,
        "POST",
        "/api/v1/game-agent/ack",
        Some(json!({"delivered": [did]})),
        &agent_h,
    )
    .await;
    assert_eq!(r.body["delivered"], 1);

    // Expiry → remove command.
    st.emit(DomainEvent::new(
        "service.suspended",
        json!({"service_id": sid, "reason": "overdue"}),
    ));
    assert!(wait_for(&log, "lp user Steve parent remove vip").await);

    // Resend and retry.
    let r = call(
        &app,
        "POST",
        &format!("/api/v1/admin/game/delivery/services/{sid}/resend"),
        Some(json!({"trigger": "purchase"})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.body["queued"], 2, "{:?}", r.body);
    reset_modules(&st).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/game-agent/poll",
            Some(json!({})),
            &agent_h
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
