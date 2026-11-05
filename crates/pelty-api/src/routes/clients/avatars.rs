//! Profile pictures. Uploads are sniffed by magic bytes (PNG/JPEG/WebP only,
//! must match the declared Content-Type), decoded with dimension and memory
//! limits, centre-cropped and re-encoded to PNG, so no original bytes
//! (metadata, appended payloads, polyglots) are ever stored or served.
use crate::error::{ApiError, ApiResult};
use crate::extract::CurrentUser;
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put};
use axum::{Json, Router};
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use pelty_core::AppError;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::Cursor;
use uuid::Uuid;

/// Avatar / logo cap: `PELTY_UPLOAD_AVATAR_MB` (default 2 MB).
pub fn max_bytes() -> usize {
    pelty_core::config::upload_limits().avatar
}
const MAX_DIM: u32 = 4096;
const LARGE: u32 = 256;
const SMALL: u32 = 64;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/avatar", put(upload).delete(remove))
        // Body cap slightly above MAX_BYTES so we can answer 413 with our own code.
        .layer(DefaultBodyLimit::max(max_bytes() + 1024))
        .route("/avatars/{id}", get(serve))
}

fn bad(code: &'static str) -> ApiError {
    AppError::Validation(code).into()
}

/// Validate and re-encode. Returns (large PNG, small PNG).
pub fn process(
    content_type: Option<&str>,
    data: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), &'static str> {
    if data.len() > max_bytes() {
        return Err("error.avatar_too_large");
    }
    let sniffed = match image::guess_format(data) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) => f,
        _ => return Err("error.avatar_type"),
    };
    let declared = match content_type
        .map(|c| {
            c.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase()
        })
        .as_deref()
    {
        Some("image/png") => ImageFormat::Png,
        Some("image/jpeg") | Some("image/jpg") => ImageFormat::Jpeg,
        Some("image/webp") => ImageFormat::WebP,
        _ => return Err("error.avatar_type"),
    };
    if declared != sniffed {
        return Err("error.avatar_type");
    }
    let mut reader = ImageReader::with_format(Cursor::new(data), sniffed);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| "error.avatar_invalid")?;
    let side = img.width().min(img.height());
    if side < 16 {
        return Err("error.avatar_invalid");
    }
    let sq = img.crop_imm(
        (img.width() - side) / 2,
        (img.height() - side) / 2,
        side,
        side,
    );
    let enc = |px: u32| -> Result<Vec<u8>, &'static str> {
        let mut out = Vec::new();
        sq.resize_exact(px, px, FilterType::Lanczos3)
            .to_rgba8()
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .map_err(|_| "error.avatar_invalid")?;
        Ok(out)
    };
    Ok((enc(LARGE)?, enc(SMALL)?))
}

#[utoipa::path(put, path = "/api/v1/auth/avatar", request_body(content = Vec<u8>, content_type = "image/png"),
    responses((status = 200), (status = 413), (status = 422)))]
pub async fn upload(
    State(s): State<AppState>,
    u: CurrentUser,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let data = body.to_vec();
    let (large, small) = tokio::task::spawn_blocking(move || process(ct.as_deref(), &data))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|c| {
            if c == "error.avatar_too_large" {
                ApiError::from(AppError::BadRequest(c))
            } else {
                bad(c)
            }
        })?;
    let v = pelty_database::avatars::set(&s.pool, u.user_id, &large, &small).await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "user.avatar_updated",
        Some(&u.user_id.to_string()),
        u.ip.as_deref(),
        json!({}),
    )
    .await?;
    Ok(Json(json!({ "avatar_version": v })))
}

#[utoipa::path(delete, path = "/api/v1/auth/avatar", responses((status = 204)))]
pub async fn remove(State(s): State<AppState>, u: CurrentUser) -> ApiResult<StatusCode> {
    if pelty_database::avatars::remove(&s.pool, u.user_id).await? {
        pelty_database::audit::record(
            &s.pool,
            Some(u.user_id),
            "user.avatar_removed",
            Some(&u.user_id.to_string()),
            u.ip.as_deref(),
            json!({}),
        )
        .await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, utoipa::IntoParams)]
pub struct AvatarQuery {
    /// `64` for the small variant; anything else serves 256px.
    pub s: Option<u32>,
    /// Cache-busting version; when it matches, the response is cacheable for a day.
    pub v: Option<i64>,
}

/// Any signed-in user may view avatars (staff lists, ticket threads).
#[utoipa::path(get, path = "/api/v1/avatars/{id}", params(AvatarQuery), responses((status = 200, content_type = "image/png"), (status = 404)))]
pub async fn serve(
    State(s): State<AppState>,
    _u: CurrentUser,
    Path(id): Path<Uuid>,
    Query(q): Query<AvatarQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let Some((bytes, version)) =
        pelty_database::avatars::get(&s.pool, id, q.s == Some(SMALL)).await?
    else {
        return Err(AppError::NotFound.into());
    };
    let etag = format!("\"{version}-{}\"", q.s.unwrap_or(LARGE));
    let cache = if q.v == Some(version) {
        "private, max-age=86400, immutable"
    } else {
        "private, no-cache"
    };
    let mut r = if headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        == Some(etag.as_str())
    {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([(header::CONTENT_TYPE, "image/png")], bytes).into_response()
    };
    let h = r.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    h.insert(
        header::ETAG,
        HeaderValue::from_str(&etag).map_err(|e| AppError::Internal(e.to_string()))?,
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
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("inline; filename=\"avatar.png\""),
    );
    Ok(r)
}
