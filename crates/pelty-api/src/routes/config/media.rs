//! Brand / site media uploads for logos and images used in admin settings.
//! Staff upload PNG/JPEG/WebP; files are re-encoded and stored under `data/media/`.
//! Public `GET /api/v1/media/{id}` serves them without auth (needed for site + emails).
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use image::{imageops::FilterType, ImageFormat, ImageReader, Limits};
use pelty_core::AppError;
use serde_json::{json, Value};
use std::io::Cursor;
use std::path::PathBuf;
use uuid::Uuid;

const MAX_DIM: u32 = 4096;
const MAX_EDGE: u32 = 2048;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/media",
            post(upload).layer(DefaultBodyLimit::max(
                pelty_core::config::upload_limits().avatar + 1024,
            )),
        )
        .route("/media/{id}", get(serve))
}

pub fn media_dir() -> PathBuf {
    std::env::var("PELTY_MEDIA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/media"))
}

fn max_bytes() -> usize {
    pelty_core::config::upload_limits().avatar
}

fn bad(code: &'static str) -> ApiError {
    AppError::Validation(code).into()
}

/// Validate + re-encode (keep aspect ratio, cap longest edge). Returns PNG bytes.
pub fn process(content_type: Option<&str>, data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.len() > max_bytes() {
        return Err("error.media_too_large");
    }
    let sniffed = match image::guess_format(data) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)) => f,
        _ => return Err("error.media_type"),
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
        _ => return Err("error.media_type"),
    };
    if declared != sniffed {
        return Err("error.media_type");
    }
    let mut reader = ImageReader::with_format(Cursor::new(data), sniffed);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIM);
    limits.max_image_height = Some(MAX_DIM);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|_| "error.media_invalid")?;
    if img.width() < 16 || img.height() < 16 {
        return Err("error.media_invalid");
    }
    let scaled = if img.width().max(img.height()) > MAX_EDGE {
        img.resize(MAX_EDGE, MAX_EDGE, FilterType::Lanczos3)
    } else {
        img
    };
    let mut out = Vec::new();
    scaled
        .to_rgba8()
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|_| "error.media_invalid")?;
    Ok(out)
}

fn id_ok(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

pub fn file_for(id: &str) -> Option<PathBuf> {
    if !id_ok(id) {
        return None;
    }
    let p = media_dir().join(format!("{id}.png"));
    p.is_file().then_some(p)
}

/// Absolute or same-origin path for a stored object.
fn public_url(s: &AppState, id: Uuid) -> String {
    let path = format!("/api/v1/media/{id}");
    let base = s.cfg.public_url.trim_end_matches('/');
    if base.starts_with("https://") || base.starts_with("http://") {
        format!("{base}{path}")
    } else {
        path
    }
}

#[utoipa::path(post, path = "/api/v1/admin/media", request_body(content = Vec<u8>, content_type = "image/png"),
    responses((status = 200), (status = 413), (status = 422)))]
pub async fn upload(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let ct = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let data = body.to_vec();
    let png = tokio::task::spawn_blocking(move || process(ct.as_deref(), &data))
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?
        .map_err(|c| {
            if c == "error.media_too_large" {
                ApiError::from(AppError::BadRequest(c))
            } else {
                bad(c)
            }
        })?;
    let id = Uuid::new_v4();
    let dir = media_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let path = dir.join(format!("{id}.png"));
    tokio::fs::write(&path, &png)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    let url = public_url(&s, id);
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "media.uploaded",
        Some(&id.to_string()),
        u.ip.as_deref(),
        json!({ "bytes": png.len(), "url": url }),
    )
    .await?;
    Ok(Json(json!({
        "id": id,
        "url": url,
        "path": format!("/api/v1/media/{id}"),
        "content_type": "image/png",
        "bytes": png.len(),
    })))
}

#[utoipa::path(get, path = "/api/v1/media/{id}", responses((status = 200, content_type = "image/png"), (status = 404)))]
pub async fn serve(Path(id): Path<String>) -> ApiResult<Response> {
    let Some(path) = file_for(&id) else {
        return Err(AppError::NotFound.into());
    };
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AppError::NotFound)?;
    let mut r = ([(header::CONTENT_TYPE, "image/png")], bytes).into_response();
    let h = r.headers_mut();
    h.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'"),
    );
    Ok(r)
}
