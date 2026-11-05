use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;

#[tokio::test]
async fn quantity_and_yesno_options_price_orders_and_invoices() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let (pid, loc, ch) = seed_product(&app, &admin).await;
    let path = format!("/api/v1/admin/catalog/products/{pid}/options");
    let ah = admin.h();
    let add = |b: serde_json::Value| call(&app, "POST", &path, Some(b), &ah);
    let r = add(json!({"name": "RAM", "option_type": "quantity", "qty_min": 1, "qty_max": 16,
        "choices": [{"label": "GB", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "1.2500"}]}]})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let ram = r.body["id"].as_str().unwrap().to_string();
    let r = add(json!({"name": "Backups", "option_type": "yesno", "required": false,
        "choices": [{"label": "Yes", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "3.00"}]}]})).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let bk = r.body["id"].as_str().unwrap().to_string();
    // Invalid definitions: quantity with 2 choices, unknown type, bad range.
    for b in [
        json!({"name": "X", "option_type": "quantity", "choices": [{"label": "a"}, {"label": "b"}]}),
        json!({"name": "X", "option_type": "slider", "choices": [{"label": "a"}]}),
        json!({"name": "X", "option_type": "quantity", "qty_min": 5, "qty_max": 2, "choices": [{"label": "a"}]}),
    ] {
        assert_eq!(add(b).await.status, StatusCode::UNPROCESSABLE_ENTITY);
    }
    // Admin listing exposes the type.
    let p = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
    let prod = p.body["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == pid.as_str())
        .unwrap()
        .clone();
    let ram_opt = prod["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == ram.as_str())
        .unwrap()
        .clone();
    assert_eq!(ram_opt["option_type"], "quantity");
    assert_eq!(ram_opt["qty_max"], 16);
    let yes = prod["options"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == bk.as_str())
        .unwrap()["choices"][0]["id"]
        .clone();

    let c = sess(&app, &st, "customer").await;
    // 10 + 2.50 (US) + 4 × 1.25 + 3 (backups) = 20.50 recurring, + 5 setup.
    let r = call(&app, "POST", "/api/v1/client/orders", Some(json!({"currency": "EUR", "items": [
        {"product_id": pid, "cycle": "monthly", "config": {loc.clone(): ch[1], ram.clone(): 4, bk.clone(): yes}}
    ]})), &c.h()).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "25.5000");
    let iid = r.body["invoice"]["id"].as_str().unwrap().to_string();
    let inv = call(
        &app,
        "GET",
        &format!("/api/v1/client/invoices/{iid}"),
        None,
        &c.h(),
    )
    .await;
    let text = inv.body.to_string();
    assert!(text.contains("4 × GB"), "{text}");
    assert!(text.contains("Backups"), "{text}");

    // Without backups and default quantity (min 1): 10 + 0 (EU) + 1.25 + 5 = 16.25.
    let r = call(
        &app,
        "POST",
        "/api/v1/client/orders",
        Some(json!({"currency": "EUR", "items": [
            {"product_id": pid, "cycle": "monthly", "config": {loc.clone(): ch[0]}}
        ]})),
        &c.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["invoice"]["total"], "16.2500");

    // Out-of-range quantity / choice id on quantity option are rejected.
    for cfg in [
        json!({loc.clone(): ch[0], ram.clone(): 17}),
        json!({loc.clone(): ch[0], ram.clone(): ch[0]}),
    ] {
        let r = call(
            &app,
            "POST",
            "/api/v1/client/orders",
            Some(json!({"currency": "EUR", "items": [
                {"product_id": pid, "cycle": "monthly", "config": cfg}
            ]})),
            &c.h(),
        )
        .await;
        assert_eq!(r.status, StatusCode::UNPROCESSABLE_ENTITY, "{:?}", r.body);
    }

    // Delete option: 404 for wrong product, then removed from catalog.
    let other = uuid::Uuid::new_v4();
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/catalog/products/{other}/options/{bk}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/catalog/products/{pid}/options/{bk}"),
            None,
            &c.h()
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/catalog/products/{pid}/options/{bk}"),
            None,
            &admin.h()
        )
        .await
        .status,
        StatusCode::OK
    );
    let p = call(&app, "GET", "/api/v1/catalog", None, &[]).await;
    let prod = p.body["products"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["id"] == pid.as_str())
        .unwrap()
        .clone();
    assert_eq!(prod["options"].as_array().unwrap().len(), 2);
}
