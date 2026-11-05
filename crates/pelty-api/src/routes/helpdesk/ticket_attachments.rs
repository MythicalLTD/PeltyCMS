//! Ticket attachment upload/download. Type is decided by magic bytes (never
//! by the client's name or Content-Type alone); the declared type must agree
//! with the sniffed one. Files are only ever served as downloads with
//! `nosniff` and a sandbox CSP.
use crate::routes::customers::customer_of;
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, CurrentUser, RequirePermission};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::AppError;
use pelty_database::ticket_attachments as att;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

/// Request body cap; the effective per-file limit is the configurable
/// `general_settings.support.attachment_max_mb` (default 5 MiB).
/// Request cap: `PELTY_UPLOAD_ATTACHMENT_MB` (default and maximum 50 MB);
/// the per-file limit in General settings applies below it.
pub fn max_bytes() -> usize {
    pelty_core::config::upload_limits().attachment.min(pelty_database::general_settings::ATTACHMENT_HARD_MAX_MB as usize * 1024 * 1024)
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tickets/attachments", post(upload))
        .layer(DefaultBodyLimit::max(max_bytes() + 1024))
        .route(
            "/client/tickets/{tid}/attachments/{id}",
            get(client_download),
        )
        .route("/admin/tickets/{tid}/attachments/{id}", get(admin_download))
}

/// Returns the canonical content type for allowed files, or an error code.
pub fn sniff(declared: Option<&str>, data: &[u8]) -> Result<&'static str, &'static str> {
    if data.is_empty() {
        return Err("error.attachment_invalid");
    }
    if data.len() > max_bytes() {
        return Err("error.attachment_too_large");
    }
    let sniffed = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a") {
        "image/gif"
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        "image/webp"
    } else if data.starts_with(b"%PDF-") {
        "application/pdf"
    } else if !data.contains(&0) && std::str::from_utf8(data).is_ok() {
        "text/plain"
    } else {
        return Err("error.attachment_type");
    };
    let decl = declared
        .map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let ok = match sniffed {
        "image/jpeg" => decl == "image/jpeg" || decl == "image/jpg",
        // Logs/configs often arrive with odd types; accept generic ones for text.
        "text/plain" => matches!(
            decl.as_str(),
            "text/plain" | "text/x-log" | "application/octet-stream" | ""
        ),
        s => decl == s,
    };
    if !ok {
        return Err("error.attachment_type");
    }
    Ok(sniffed)
}

/// Keeps a display-safe filename: no paths, control chars or quotes.
pub fn clean_name(raw: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("");
    let s: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\'' | '<' | '>' | ';'))
        .take(120)
        .collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() {
        "file".into()
    } else {
        s
    }
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct UploadQuery {
    pub filename: Option<String>,
}

/// Upload a file (raw body). Returns an id to pass as `attachment_ids` when
/// opening a ticket or replying. Max size is configurable (default 5 MiB); PNG/JPEG/GIF/WebP/PDF/plain text.
#[utoipa::path(post, path = "/api/v1/tickets/attachments", params(UploadQuery), request_body(content = Vec<u8>, content_type = "application/octet-stream"),
    responses((status = 200), (status = 400), (status = 422)))]
pub async fn upload(
    State(s): State<AppState>,
    u: CurrentUser,
    headers: HeaderMap,
    Query(q): Query<UploadQuery>,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    let limit = pelty_database::general_settings::load(&s.pool).await?.support.max_bytes();
    if body.len() > limit {
        return Err(AppError::BadRequest("error.attachment_too_large").into());
    }
    let kind = sniff(ct, &body).map_err(|c| {
        if c == "error.attachment_too_large" {
            ApiError::from(AppError::BadRequest(c))
        } else {
            AppError::Validation(c).into()
        }
    })?;
    let name = clean_name(q.filename.as_deref().unwrap_or("file"));
    let a = att::create_pending(&s.pool, u.user_id, &name, kind, &body).await?;
    Ok(Json(json!(a)))
}

pub(crate) fn serve(name: String, ct: String, data: Vec<u8>) -> ApiResult<Response> {
    let mut r = ([(header::CONTENT_TYPE, ct)], data).into_response();
    let h = r.headers_mut();
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let disp = format!("attachment; filename=\"{ascii}\"");
    h.insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&disp).map_err(|e| AppError::Internal(e.to_string()))?,
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; sandbox"),
    );
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(r)
}

#[utoipa::path(get, path = "/api/v1/client/tickets/{tid}/attachments/{id}", responses((status = 200), (status = 404)))]
pub async fn client_download(
    State(s): State<AppState>,
    u: CurrentUser,
    Path((tid, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let cid = customer_of(&s, &u).await?;
    s.tickets().get(tid, Some(cid))
        .await?
        .ok_or(AppError::NotFound)?;
    let (n, ct, d) = att::download(&s.pool, tid, id, false)
        .await?
        .ok_or(AppError::NotFound)?;
    serve(n, ct, d)
}

#[utoipa::path(get, path = "/api/v1/admin/tickets/{tid}/attachments/{id}", responses((status = 200), (status = 404)))]
pub async fn admin_download(
    State(s): State<AppState>,
    _p: RequirePermission<perms::TicketsManage>,
    Path((tid, id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Response> {
    let (n, ct, d) = att::download(&s.pool, tid, id, true)
        .await?
        .ok_or(AppError::NotFound)?;
    serve(n, ct, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sniffing() {
        assert_eq!(
            sniff(Some("image/png"), b"\x89PNG\r\n\x1a\nxxxx"),
            Ok("image/png")
        );
        assert_eq!(
            sniff(Some("image/jpeg"), b"\x89PNG\r\n\x1a\nxxxx"),
            Err("error.attachment_type")
        );
        assert_eq!(
            sniff(Some("text/html"), b"<script>alert(1)</script>"),
            Err("error.attachment_type")
        );
        assert_eq!(sniff(Some("text/plain"), b"log line"), Ok("text/plain"));
        assert_eq!(
            sniff(Some("application/pdf"), b"%PDF-1.7"),
            Ok("application/pdf")
        );
        assert_eq!(
            sniff(Some("application/x-msdownload"), b"MZ\x90\x00\x03"),
            Err("error.attachment_type")
        );
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("..\\a\"b<c>.txt"), "abc.txt");
        assert_eq!(clean_name(".."), "file");
    }
}
