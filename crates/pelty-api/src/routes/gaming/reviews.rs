//! Product reviews and Q&A (module `product_reviews`): public ratings and
//! answered questions, customer reviews and questions, moderation for staff
//! (`catalog.manage`), and replies from the creator who owns the product.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::reviews::{self as rv, NewReview};
use pelty_database::{audit, marketplace as mk};
use pelty_extensions::events::DomainEvent;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/public/reviews/products/{id}", get(public_reviews))
        .route(
            "/public/reviews/products/{id}/questions",
            get(public_questions),
        )
        .route("/client/reviews", get(my_reviews))
        .route("/client/reviews/{id}", delete(delete_review))
        .route(
            "/client/reviews/products/{id}",
            get(eligibility).post(submit_review),
        )
        .route("/client/reviews/products/{id}/questions", post(ask))
        .route("/client/reviews/creator", get(creator_inbox))
        .route(
            "/client/reviews/creator/reviews/{id}/reply",
            post(creator_reply),
        )
        .route(
            "/client/reviews/creator/questions/{id}/answer",
            post(creator_answer),
        )
        .route("/admin/reviews", get(admin_reviews))
        .route("/admin/reviews/{id}/status", post(admin_review_status))
        .route("/admin/reviews/{id}/reply", post(admin_reply))
        .route("/admin/reviews/questions", get(admin_questions))
        .route(
            "/admin/reviews/questions/{id}/status",
            post(admin_question_status),
        )
        .route("/admin/reviews/questions/{id}/answer", post(admin_answer))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

async fn visible(s: &AppState, product: Uuid) -> ApiResult<()> {
    if !rv::product_visible(&s.pool, product).await? {
        return Err(AppError::NotFound.into());
    }
    Ok(())
}

/// Approved reviews with the rating summary.
#[utoipa::path(get, path = "/api/v1/public/reviews/products/{id}", params(("id" = Uuid, Path), Page), responses((status = 200), (status = 404)))]
pub async fn public_reviews(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
    Query(p): Query<Page>,
) -> ApiResult<Json<Value>> {
    visible(&s, id).await?;
    let cfg = rv::config(&s.pool).await?;
    Ok(Json(json!({
        "summary": rv::summary(&s.pool, id).await?,
        "items": rv::public(&s.pool, id, p.limit.unwrap_or(20), p.offset.unwrap_or(0)).await?,
        "questions_open": cfg.questions,
        "verified_only": cfg.verified_only,
    })))
}

/// Approved questions with answers.
#[utoipa::path(get, path = "/api/v1/public/reviews/products/{id}/questions", params(("id" = Uuid, Path)), responses((status = 200, body = Vec<rv::PublicQuestion>), (status = 404)))]
pub async fn public_questions(
    State(s): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    visible(&s, id).await?;
    if !rv::config(&s.pool).await?.questions {
        return Ok(Json(json!({ "items": [] })));
    }
    Ok(Json(
        json!({ "items": rv::public_questions(&s.pool, id).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/reviews", responses((status = 200, body = Vec<rv::Review>)))]
pub async fn my_reviews(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = customer_of(&s, &u).await?;
    Ok(Json(json!({ "items": rv::mine(&s.pool, c).await? })))
}

/// Can the customer review this product, and their current review.
#[utoipa::path(get, path = "/api/v1/client/reviews/products/{id}", params(("id" = Uuid, Path)), responses((status = 200), (status = 404)))]
pub async fn eligibility(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    visible(&s, id).await?;
    let c = customer_of(&s, &u).await?;
    let cfg = rv::config(&s.pool).await?;
    let purchased = rv::purchased(&s.pool, c, id).await?;
    let mine = rv::mine(&s.pool, c)
        .await?
        .into_iter()
        .find(|r| r.product_id == id);
    Ok(Json(json!({
        "purchased": purchased,
        "can_review": purchased || !cfg.verified_only,
        "can_ask": cfg.questions,
        "review": mine,
    })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ReviewBody {
    pub rating: i16,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub body: String,
}

/// Create or edit the customer's review of a product.
#[utoipa::path(post, path = "/api/v1/client/reviews/products/{id}", params(("id" = Uuid, Path)), request_body = ReviewBody, responses((status = 200, body = rv::Review), (status = 403), (status = 404), (status = 422)))]
pub async fn submit_review(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<ReviewBody>,
) -> ApiResult<Json<rv::Review>> {
    visible(&s, id).await?;
    let c = customer_of(&s, &u).await?;
    let cfg = rv::config(&s.pool).await?;
    let purchased = rv::purchased(&s.pool, c, id).await?;
    if cfg.verified_only && !purchased {
        return Err(AppError::Denied("error.review_purchase_required").into());
    }
    let r = rv::upsert(
        &s.pool,
        c,
        id,
        &NewReview {
            rating: b.rating,
            title: &b.title,
            body: &b.body,
        },
        purchased,
        !cfg.require_approval,
    )
    .await?;
    s.emit(DomainEvent::new(
        "review.submitted",
        json!({ "review_id": r.id, "product_id": id, "customer_id": c, "rating": r.rating, "status": r.status, "verified": r.verified }),
    ));
    Ok(Json(r))
}

#[utoipa::path(delete, path = "/api/v1/client/reviews/{id}", params(("id" = Uuid, Path)), responses((status = 204), (status = 404)))]
pub async fn delete_review(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<axum::http::StatusCode> {
    let c = customer_of(&s, &u).await?;
    rv::delete_own(&s.pool, c, id).await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TextBody {
    pub text: String,
}

/// Ask a question about a product.
#[utoipa::path(post, path = "/api/v1/client/reviews/products/{id}/questions", params(("id" = Uuid, Path)), request_body = TextBody, responses((status = 200, body = rv::Question), (status = 404), (status = 409), (status = 422)))]
pub async fn ask(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<TextBody>,
) -> ApiResult<Json<rv::Question>> {
    visible(&s, id).await?;
    let cfg = rv::config(&s.pool).await?;
    if !cfg.questions {
        return Err(AppError::Conflict("error.questions_closed").into());
    }
    let c = customer_of(&s, &u).await?;
    let q = rv::ask(&s.pool, c, id, &b.text, !cfg.require_approval).await?;
    s.emit(DomainEvent::new(
        "question.asked",
        json!({ "question_id": q.id, "product_id": id, "customer_id": c }),
    ));
    Ok(Json(q))
}

/// The approved creator behind the current user, if any.
async fn my_creator(s: &AppState, u: &CurrentUser) -> ApiResult<Uuid> {
    match mk::by_user(&s.pool, u.user_id).await? {
        Some(c) if c.status == "approved" => Ok(c.id),
        _ => Err(AppError::Forbidden.into()),
    }
}

async fn owns(s: &AppState, creator: Uuid, product: Uuid) -> ApiResult<()> {
    if mk::owner_of(&s.pool, product).await? != Some(creator) {
        return Err(AppError::NotFound.into());
    }
    Ok(())
}

/// Reviews and questions on the current creator's products.
#[utoipa::path(get, path = "/api/v1/client/reviews/creator", responses((status = 200), (status = 403)))]
pub async fn creator_inbox(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let c = my_creator(&s, &u).await?;
    Ok(Json(json!({
        "reviews": rv::list(&s.pool, Some("approved"), Some(c)).await?,
        "questions": rv::questions(&s.pool, Some("approved"), Some(c)).await?
            .into_iter()
            .chain(rv::questions(&s.pool, Some("pending"), Some(c)).await?)
            .collect::<Vec<_>>(),
    })))
}

#[utoipa::path(post, path = "/api/v1/client/reviews/creator/reviews/{id}/reply", params(("id" = Uuid, Path)), request_body = TextBody, responses((status = 200, body = rv::Review), (status = 403), (status = 404)))]
pub async fn creator_reply(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<TextBody>,
) -> ApiResult<Json<rv::Review>> {
    let c = my_creator(&s, &u).await?;
    let r = rv::get(&s.pool, id).await?;
    owns(&s, c, r.product_id).await?;
    Ok(Json(rv::reply(&s.pool, id, &b.text).await?))
}

/// Answering a question publishes it, so creators may only answer
/// questions staff have not rejected.
#[utoipa::path(post, path = "/api/v1/client/reviews/creator/questions/{id}/answer", params(("id" = Uuid, Path)), request_body = TextBody, responses((status = 200, body = rv::Question), (status = 403), (status = 404)))]
pub async fn creator_answer(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
    Json(b): Json<TextBody>,
) -> ApiResult<Json<rv::Question>> {
    let c = my_creator(&s, &u).await?;
    let q = rv::question(&s.pool, id).await?;
    owns(&s, c, q.product_id).await?;
    if q.status == "rejected" {
        return Err(AppError::NotFound.into());
    }
    let q = rv::answer(&s.pool, id, u.user_id, &b.text).await?;
    s.emit(DomainEvent::new(
        "question.answered",
        json!({ "question_id": q.id, "product_id": q.product_id, "customer_id": q.customer_id }),
    ));
    Ok(Json(q))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct StatusFilter {
    pub status: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/admin/reviews", params(StatusFilter), responses((status = 200, body = Vec<rv::ReviewRow>), (status = 403)))]
pub async fn admin_reviews(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(f): Query<StatusFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": rv::list(&s.pool, f.status.as_deref(), None).await? }),
    ))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct StatusBody {
    pub status: String,
}

#[utoipa::path(post, path = "/api/v1/admin/reviews/{id}/status", params(("id" = Uuid, Path)), request_body = StatusBody, responses((status = 200, body = rv::Review), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_review_status(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<rv::Review>> {
    let r = rv::set_status(&s.pool, id, &b.status).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "review.moderated",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "status": r.status }),
    )
    .await?;
    if r.status == "approved" {
        s.emit(DomainEvent::new(
            "review.approved",
            json!({ "review_id": r.id, "product_id": r.product_id, "customer_id": r.customer_id, "rating": r.rating }),
        ));
    }
    Ok(Json(r))
}

#[utoipa::path(post, path = "/api/v1/admin/reviews/{id}/reply", params(("id" = Uuid, Path)), request_body = TextBody, responses((status = 200, body = rv::Review), (status = 403), (status = 404)))]
pub async fn admin_reply(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TextBody>,
) -> ApiResult<Json<rv::Review>> {
    Ok(Json(rv::reply(&s.pool, id, &b.text).await?))
}

#[utoipa::path(get, path = "/api/v1/admin/reviews/questions", params(StatusFilter), responses((status = 200, body = Vec<rv::QuestionRow>), (status = 403)))]
pub async fn admin_questions(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Query(f): Query<StatusFilter>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": rv::questions(&s.pool, f.status.as_deref(), None).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/reviews/questions/{id}/status", params(("id" = Uuid, Path)), request_body = StatusBody, responses((status = 200, body = rv::Question), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_question_status(
    State(s): State<AppState>,
    _p: RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<StatusBody>,
) -> ApiResult<Json<rv::Question>> {
    Ok(Json(rv::set_question_status(&s.pool, id, &b.status).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/reviews/questions/{id}/answer", params(("id" = Uuid, Path)), request_body = TextBody, responses((status = 200, body = rv::Question), (status = 403), (status = 404), (status = 422)))]
pub async fn admin_answer(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::CatalogManage>,
    Path(id): Path<Uuid>,
    Json(b): Json<TextBody>,
) -> ApiResult<Json<rv::Question>> {
    let q = rv::answer(&s.pool, id, u.user_id, &b.text).await?;
    s.emit(DomainEvent::new(
        "question.answered",
        json!({ "question_id": q.id, "product_id": q.product_id, "customer_id": q.customer_id }),
    ));
    Ok(Json(q))
}
