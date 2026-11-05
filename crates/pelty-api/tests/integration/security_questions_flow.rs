use crate::common;
use axum::http::StatusCode;
use common::*;
use serde_json::json;
use uuid::Uuid;

const PASSWORD: &str = "Very-Long-Pass-123";

#[tokio::test]
async fn security_questions_set_and_verify() {
    let (app, st) = app().await;
    let admin = sess(&app, &st, "admin").await;
    let cust = sess(&app, &st, "customer").await;
    let (ah, ch) = (admin.h(), cust.h());

    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/security-questions",
            Some(json!({"question": "Pet?"})),
            &ch
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/v1/admin/security-questions",
            Some(json!({"question": " "})),
            &ah
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "POST",
        "/api/v1/admin/security-questions",
        Some(json!({"question": "First pet's name?"})),
        &ah,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let qid = r.body["id"].as_str().unwrap().to_string();

    let r = call(&app, "GET", "/api/v1/account/security-question", None, &ch).await;
    assert!(r.body["questions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|q| q["id"] == qid.as_str()));
    assert!(r.body["current"].is_null());

    let uid: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE email = $1")
        .bind(&cust.email)
        .fetch_one(&st.pool)
        .await
        .unwrap();
    let cid = pelty_database::customers::id_for_user(&st.pool, uid)
        .await
        .unwrap();
    let verify = format!("/api/v1/admin/customers/{cid}/security-question/verify");
    // No answer yet.
    assert_eq!(
        call(&app, "POST", &verify, Some(json!({"answer": "x"})), &ah)
            .await
            .status,
        StatusCode::CONFLICT
    );

    let set = |pw: &str, answer: &str, q: &str| json!({"question_id": q, "answer": answer, "password": pw});
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/security-question",
            Some(set("wrong", "Fluffy", &qid)),
            &ch
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/security-question",
            Some(set(PASSWORD, "x", &qid)),
            &ch
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        call(
            &app,
            "PUT",
            "/api/v1/account/security-question",
            Some(set(PASSWORD, "Fluffy", &Uuid::new_v4().to_string())),
            &ch
        )
        .await
        .status,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let r = call(
        &app,
        "PUT",
        "/api/v1/account/security-question",
        Some(set(PASSWORD, "  Mr  Fluffy ", &qid)),
        &ch,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    assert_eq!(r.body["current"]["question_id"], qid.as_str());
    assert!(
        r.body.to_string().find("fluffy").is_none(),
        "answer never echoed"
    );

    // Staff see the question, not the answer, and can check a caller's reply.
    let r = call(
        &app,
        "GET",
        &format!("/api/v1/admin/customers/{cid}/security-question"),
        None,
        &ah,
    )
    .await;
    assert_eq!(r.body["current"]["question"], "First pet's name?");
    assert_eq!(
        call(
            &app,
            "POST",
            &verify,
            Some(json!({"answer": "mr fluffy"})),
            &ah
        )
        .await
        .body["match"],
        true
    );
    assert_eq!(
        call(&app, "POST", &verify, Some(json!({"answer": "rex"})), &ah)
            .await
            .body["match"],
        false
    );
    assert_eq!(
        call(&app, "POST", &verify, Some(json!({"answer": "x"})), &ch)
            .await
            .status,
        StatusCode::FORBIDDEN
    );

    // Clearing, then deleting the question.
    assert_eq!(
        call(
            &app,
            "DELETE",
            "/api/v1/account/security-question",
            None,
            &ch
        )
        .await
        .status,
        StatusCode::OK
    );
    assert!(
        call(&app, "GET", "/api/v1/account/security-question", None, &ch)
            .await
            .body["current"]
            .is_null()
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/security-questions/{qid}"),
            None,
            &ah
        )
        .await
        .status,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &app,
            "DELETE",
            &format!("/api/v1/admin/security-questions/{qid}"),
            None,
            &ah
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
}
