//! Static file serving for frontend bundles and theme assets.
//!
//! - Weak ETag from size + mtime, `If-None-Match` → 304.
//! - Vite's content-hashed `assets/*` get a 1-year immutable cache; everything
//!   else (notably `index.html`) is `no-cache` so theme switches and deploys
//!   show up immediately.
//! - Compression is applied by the router's `CompressionLayer`.
//! - Paths are canonicalized and must stay inside the bundle root.
use crate::AppState;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::UNIX_EPOCH;

/// Lookup into a web UI build compiled into the binary (`pelty-server`'s
/// `embed-frontend` feature): request path without the leading `/` → bytes.
pub type EmbeddedLookup = fn(&str) -> Option<&'static [u8]>;
static EMBEDDED: OnceLock<EmbeddedLookup> = OnceLock::new();

/// Register the compiled-in web UI. It is served when neither the active
/// theme nor `PELTY_FRONTEND_DIR` has a build on disk (a disk build always
/// wins, so a custom frontend needs no rebuild of the server).
pub fn set_embedded(f: EmbeddedLookup) {
    let _ = EMBEDDED.set(f);
}

/// A file of the compiled-in web UI, if there is one.
pub fn embedded_file(rel: &str) -> Option<&'static [u8]> {
    EMBEDDED.get().and_then(|f| f(rel))
}

/// Serve compiled-in bytes; the ETag is a content hash (computed per
/// request, but `assets/*` are immutable so browsers rarely ask twice).
fn serve_bytes(
    name: &str,
    bytes: &'static [u8],
    req_headers: &HeaderMap,
    cache: &'static str,
) -> Option<Response> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    let etag = format!("W/\"e{:x}-{:x}\"", bytes.len(), h.finish());
    let etag_hv = HeaderValue::from_str(&etag).ok()?;
    let not_modified = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    let base = [
        (header::ETAG, etag_hv),
        (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
    ];
    if not_modified {
        return Some((StatusCode::NOT_MODIFIED, base).into_response());
    }
    Some(
        (
            base,
            [(header::CONTENT_TYPE, content_type(Path::new(name)))],
            Body::from(bytes),
        )
            .into_response(),
    )
}

/// Embedded fallback for [`frontend`]: same rules as the disk bundle.
fn serve_embedded(path: &str, req_headers: &HeaderMap) -> Option<Response> {
    let rel = path.trim_start_matches('/');
    let safe = Path::new(rel)
        .components()
        .all(|c| matches!(c, Component::Normal(_)));
    if !safe {
        // `..`, absolute or prefixed paths are never a route.
        return Some(StatusCode::NOT_FOUND.into_response());
    }
    if !rel.is_empty() {
        if let Some(b) = embedded_file(rel) {
            let cache = if path.starts_with("/assets/") {
                IMMUTABLE
            } else {
                NO_CACHE
            };
            return serve_bytes(rel, b, req_headers, cache);
        }
    }
    let last = path.rsplit('/').next().unwrap_or("");
    if last.contains('.') {
        return Some(StatusCode::NOT_FOUND.into_response());
    }
    serve_bytes(
        "index.html",
        embedded_file("index.html")?,
        req_headers,
        NO_CACHE,
    )
}

pub const IMMUTABLE: &str = "public, max-age=31536000, immutable";
pub const NO_CACHE: &str = "no-cache";

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "map" => "application/json",
        _ => "application/octet-stream",
    }
}

/// Serve one file with ETag / conditional GET support.
pub async fn serve_file(
    path: &Path,
    req_headers: &HeaderMap,
    cache: &'static str,
) -> Option<Response> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let etag = format!("W/\"{:x}-{:x}\"", meta.len(), mtime);
    let etag_hv = HeaderValue::from_str(&etag).ok()?;
    let not_modified = req_headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == etag || t.trim() == "*"));
    let base = [
        (header::ETAG, etag_hv),
        (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
    ];
    if not_modified {
        return Some((StatusCode::NOT_MODIFIED, base).into_response());
    }
    let bytes = tokio::fs::read(path).await.ok()?;
    Some(
        (
            base,
            [(header::CONTENT_TYPE, content_type(path))],
            Body::from(bytes),
        )
            .into_response(),
    )
}

/// Resolve a request path to a file inside `root`, rejecting traversal.
fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let rel = uri_path.trim_start_matches('/');
    let rel_path = Path::new(rel);
    if !rel_path
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let p = root.join(rel_path).canonicalize().ok()?;
    (p.starts_with(root) && p.is_file()).then_some(p)
}

/// Bundle root for the request: the active theme's full frontend when it has
/// one, otherwise the configured default frontend dir.
pub fn bundle_root(s: &AppState) -> Option<PathBuf> {
    s.themes
        .frontend_root(&s.active_theme())
        .or_else(|| s.cfg.frontend_dir.canonicalize().ok())
}

/// Router fallback: static file or SPA `index.html`.
pub async fn frontend(State(s): State<AppState>, req: Request) -> Response {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let path = req.uri().path();
    // Unknown API routes are JSON 404s, never the SPA.
    if path.starts_with("/api/") {
        return crate::error::ApiError(pelty_core::AppError::NotFound).into_response();
    }
    let Some(root) = bundle_root(&s) else {
        if let Some(r) = serve_embedded(path, req.headers()) {
            return r;
        }
        return (StatusCode::SERVICE_UNAVAILABLE, "frontend not built").into_response();
    };
    if let Some(file) = resolve(&root, path) {
        let cache = if path.starts_with("/assets/") {
            IMMUTABLE
        } else {
            NO_CACHE
        };
        if let Some(r) = serve_file(&file, req.headers(), cache).await {
            return r;
        }
    }
    // Missing files with an extension are real 404s; routes get index.html.
    let last = path.rsplit('/').next().unwrap_or("");
    if last.contains('.') {
        return StatusCode::NOT_FOUND.into_response();
    }
    match serve_file(&root.join("index.html"), req.headers(), NO_CACHE).await {
        Some(r) => r,
        None => (StatusCode::SERVICE_UNAVAILABLE, "frontend not built").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake(rel: &str) -> Option<&'static [u8]> {
        match rel {
            "index.html" => Some(b"<html>spa</html>"),
            "assets/app-1.js" => Some(b"console.log(1)"),
            _ => None,
        }
    }

    #[tokio::test]
    async fn embedded_ui_serves_files_spa_routes_and_404s() {
        set_embedded(fake);
        let h = HeaderMap::new();
        let r = serve_embedded("/assets/app-1.js", &h).unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        assert_eq!(r.headers()[header::CACHE_CONTROL], IMMUTABLE);
        assert_eq!(
            r.headers()[header::CONTENT_TYPE],
            "text/javascript; charset=utf-8"
        );
        let etag = r.headers()[header::ETAG].clone();
        let mut h2 = HeaderMap::new();
        h2.insert(header::IF_NONE_MATCH, etag);
        assert_eq!(
            serve_embedded("/assets/app-1.js", &h2).unwrap().status(),
            StatusCode::NOT_MODIFIED
        );
        let spa = serve_embedded("/admin/jobs", &h).unwrap();
        assert_eq!(spa.status(), StatusCode::OK);
        assert_eq!(spa.headers()[header::CACHE_CONTROL], NO_CACHE);
        assert_eq!(
            serve_embedded("/missing.js", &h).unwrap().status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            serve_embedded("/../etc/passwd", &h).unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
}
