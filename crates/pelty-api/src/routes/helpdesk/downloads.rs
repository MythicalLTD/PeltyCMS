//! Downloads area: staff upload (`downloads.manage`, raw body, ≤ 25 MiB,
//! allow-listed types by magic bytes); customers download what their active
//! services entitle them to. Files are always served as attachments.
use crate::error::ApiResult;
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::routes::customers::customer_of;
use crate::routes::ticket_attachments::{clean_name, serve};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::audit;
use pelty_database::downloads::{self, NewDownload};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/downloads", get(admin_list).post(admin_upload))
        .layer(DefaultBodyLimit::max(
            pelty_core::config::upload_limits().download + 1024,
        ))
        .route(
            "/admin/downloads/{id}",
            get(admin_file).delete(admin_delete),
        )
        .route("/client/downloads", get(client_list))
        .route("/client/downloads/{id}", get(client_file))
}

/// Allowed archive/document/image types, decided by content, not by name.
pub fn sniff(data: &[u8]) -> Result<&'static str, &'static str> {
    if data.is_empty() {
        return Err("error.attachment_invalid");
    }
    if data.len() > pelty_core::config::upload_limits().download {
        return Err("error.download.too_large");
    }
    Ok(if data.starts_with(b"PK\x03\x04") {
        "application/zip"
    } else if data.starts_with(b"\x1f\x8b") {
        "application/gzip"
    } else if data.starts_with(b"%PDF-") {
        "application/pdf"
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if !data.contains(&0) && std::str::from_utf8(data).is_ok() {
        "text/plain"
    } else {
        return Err("error.download.type");
    })
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct UploadQuery {
    pub title: String,
    pub filename: String,
    #[serde(default)]
    pub description: String,
    /// Comma-separated product ids; empty = all customers.
    #[serde(default)]
    pub products: String,
}

#[utoipa::path(get, path = "/api/v1/admin/downloads", responses((status = 200), (status = 403)))]
pub async fn admin_list(
    State(s): State<AppState>,
    _p: RequirePermission<perms::DownloadsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "items": downloads::list_all(&s.pool).await? }),
    ))
}

#[utoipa::path(post, path = "/api/v1/admin/downloads", params(UploadQuery), request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 200), (status = 400), (status = 422)))]
pub async fn admin_upload(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::DownloadsManage>,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let ct = sniff(&body).map_err(|c| {
        if c == "error.download.too_large" {
            AppError::BadRequest(c)
        } else {
            AppError::Validation(c)
        }
    })?;
    let products = q
        .products
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.parse::<Uuid>()
                .map_err(|_| AppError::Validation("error.invalid_reference"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let name = clean_name(&q.filename);
    let hash = Sha256::digest(&body);
    let id = downloads::create(
        &s.pool,
        NewDownload {
            title: &q.title,
            description: &q.description,
            filename: &name,
            content_type: ct,
            sha256: hash.as_slice(),
            data: &body,
            product_ids: &products,
        },
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "download.created",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "filename": name, "size": body.len() }),
    )
    .await?;
    Ok(Json(json!({ "id": id })))
}

#[utoipa::path(get, path = "/api/v1/admin/downloads/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_file(
    State(s): State<AppState>,
    _p: RequirePermission<perms::DownloadsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let (n, ct, d) = downloads::fetch(&s.pool, id, None)
        .await?
        .ok_or(AppError::NotFound)?;
    serve(n, ct, d)
}

#[utoipa::path(delete, path = "/api/v1/admin/downloads/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_delete(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::DownloadsManage>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Value>> {
    downloads::delete(&s.pool, id).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "download.deleted",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "ok": true })))
}

#[utoipa::path(get, path = "/api/v1/client/downloads", responses((status = 200), (status = 401)))]
pub async fn client_list(State(s): State<AppState>, u: CurrentUser) -> ApiResult<Json<Value>> {
    let cid = customer_of(&s, &u).await?;
    Ok(Json(
        json!({ "items": downloads::list_for(&s.pool, cid).await? }),
    ))
}

#[utoipa::path(get, path = "/api/v1/client/downloads/{id}", responses((status = 200), (status = 404)))]
pub async fn client_file(
    State(s): State<AppState>,
    u: CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Response> {
    let cid = customer_of(&s, &u).await?;
    let (n, ct, d) = downloads::fetch(&s.pool, id, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    serve(n, ct, d)
}

#[cfg(test)]
mod tests {
    #[test]
    fn sniffs_by_content() {
        assert_eq!(super::sniff(b"PK\x03\x04rest"), Ok("application/zip"));
        assert_eq!(super::sniff(b"hello"), Ok("text/plain"));
        assert_eq!(super::sniff(b"MZ\x90\x00\x03"), Err("error.download.type"));
        assert!(super::sniff(b"").is_err());
    }
}
