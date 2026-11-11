//! Image uploads: staff (logos, product images, store banner) and marketplace
//! creators (avatars, banners, listing galleries).
//! PNG/JPEG/WebP/GIF are sniffed, decoded with limits and re-encoded to PNG
//! (GIFs keep their first frame), with a 480 px thumbnail, under `data/media/`.
//! Public `GET /api/v1/media/{id}` (and `/thumb`) serves them without auth.
use crate::error::{ApiError, ApiResult};
use crate::extract::{perms, CurrentUser, RequirePermission};
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
const THUMB_EDGE: u32 = 480;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/admin/media",
            post(upload).layer(DefaultBodyLimit::max(
                pelty_core::config::upload_limits().avatar + 1024,
            )),
        )
        .route(
            "/client/creator/media",
            post(creator_upload).layer(DefaultBodyLimit::max(
                pelty_core::config::upload_limits().avatar + 1024,
            )),
        )
        .route("/media/{id}", get(serve))
        .route("/media/{id}/thumb", get(serve_thumb))
}

pub fn media_dir() -> PathBuf {
    // `PELTY_MEDIA_DIR`: uploaded brand/site images (default `data/media`).
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

/// A processed upload: full image and thumbnail (PNG) and the full size.
pub struct Processed {
    pub png: Vec<u8>,
    pub thumb: Vec<u8>,
    pub size: (u32, u32),
}

/// Validate + re-encode (keep aspect ratio, cap longest edge) plus a thumbnail.
pub fn process(content_type: Option<&str>, data: &[u8]) -> Result<Processed, &'static str> {
    if data.len() > max_bytes() {
        return Err("error.media_too_large");
    }
    let sniffed = match image::guess_format(data) {
        Ok(f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif)) => f,
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
        Some("image/gif") => ImageFormat::Gif,
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
    let thumb = if scaled.width().max(scaled.height()) > THUMB_EDGE {
        scaled.resize(THUMB_EDGE, THUMB_EDGE, FilterType::Triangle)
    } else {
        scaled.clone()
    };
    let png = |img: &image::DynamicImage| -> Result<Vec<u8>, &'static str> {
        let mut out = Vec::new();
        img.to_rgba8()
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .map_err(|_| "error.media_invalid")?;
        Ok(out)
    };
    Ok(Processed {
        png: png(&scaled)?,
        thumb: png(&thumb)?,
        size: (scaled.width(), scaled.height()),
    })
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

/// The thumbnail, or the full image for uploads made before thumbnails existed.
pub fn thumb_for(id: &str) -> Option<PathBuf> {
    if !id_ok(id) {
        return None;
    }
    let p = media_dir().join(format!("{id}_t.png"));
    if p.is_file() {
        Some(p)
    } else {
        file_for(id)
    }
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

fn content_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

/// Process and store one upload; answers the JSON every upload endpoint returns.
async fn store(
    s: &AppState,
    ct: Option<String>,
    data: Bytes,
    owner: Uuid,
    scope: &'static str,
) -> ApiResult<Value> {
    let data = data.to_vec();
    let p = tokio::task::spawn_blocking(move || process(ct.as_deref(), &data))
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
    let io = |e: std::io::Error| AppError::Internal(e.to_string());
    tokio::fs::write(dir.join(format!("{id}_t.png")), &p.thumb)
        .await
        .map_err(io)?;
    tokio::fs::write(dir.join(format!("{id}.png")), &p.png)
        .await
        .map_err(io)?;
    pelty_database::media::record(
        &s.pool,
        id,
        Some(owner),
        scope,
        p.png.len() + p.thumb.len(),
        p.size,
    )
    .await?;
    let url = public_url(s, id);
    Ok(json!({
        "id": id,
        "url": url,
        "path": format!("/api/v1/media/{id}"),
        "thumb": format!("/api/v1/media/{id}/thumb"),
        "content_type": "image/png",
        "bytes": p.png.len(),
        "width": p.size.0,
        "height": p.size.1,
    }))
}

/// Staff upload (logos, product images, store banner).
#[utoipa::path(post, path = "/api/v1/admin/media", request_body(content = Vec<u8>, content_type = "image/png"),
    responses((status = 200), (status = 413), (status = 422)))]
pub async fn upload(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let v = store(&s, content_type(&headers), body, u.user_id, "staff").await?;
    pelty_database::audit::record(
        &s.pool,
        Some(u.user_id),
        "media.uploaded",
        v["id"].as_str(),
        u.ip.as_deref(),
        json!({ "bytes": v["bytes"], "url": v["url"] }),
    )
    .await?;
    Ok(Json(v))
}

/// Marketplace upload for any signed-in customer (creator avatar, banner and
/// listing gallery, also while applying). Rate-limited and capped per user.
#[utoipa::path(post, path = "/api/v1/client/creator/media", request_body(content = Vec<u8>, content_type = "image/png"),
    responses((status = 200), (status = 413), (status = 422), (status = 429)))]
pub async fn creator_upload(
    State(s): State<AppState>,
    u: CurrentUser,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult<Json<Value>> {
    let used = pelty_database::media::usage(&s.pool, u.user_id, "creator").await?;
    if let Some(code) = pelty_database::media::creator_quota_error(used, body.len()) {
        return Err(if code == "error.media_rate_limited" {
            AppError::RateLimited.into()
        } else {
            bad(code)
        });
    }
    Ok(Json(
        store(&s, content_type(&headers), body, u.user_id, "creator").await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/media/{id}", responses((status = 200, content_type = "image/png"), (status = 404)))]
pub async fn serve(Path(id): Path<String>) -> ApiResult<Response> {
    send(file_for(&id)).await
}

/// 480 px thumbnail (cards, admin lists).
#[utoipa::path(get, path = "/api/v1/media/{id}/thumb", responses((status = 200, content_type = "image/png"), (status = 404)))]
pub async fn serve_thumb(Path(id): Path<String>) -> ApiResult<Response> {
    send(thumb_for(&id)).await
}

async fn send(path: Option<PathBuf>) -> ApiResult<Response> {
    let Some(path) = path else {
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
