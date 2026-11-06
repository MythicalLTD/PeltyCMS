//! Serves the generated docs (`make docs` → `target/docs`) at `/docs`: the
//! user docs at `/docs/`, the developer docs at `/docs/developers/`. Staff only unless `PELTY_DOCS_PUBLIC` is true.
use crate::extract::CurrentUser;
use crate::AppState;
use axum::extract::{FromRequestParts, Request, State};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::get;
use axum::Router;
use std::path::{Component, Path, PathBuf};

pub fn root_router() -> Router<AppState> {
    Router::new()
        .route("/docs", get(|| async { Redirect::permanent("/docs/") }))
        .route("/docs/", get(serve))
        .route("/docs/{*path}", get(serve))
}

fn dir() -> PathBuf {
    // Release archives ship `docs/` next to the binary; source checkouts use `target/docs`.
    if let Ok(p) = std::env::var("PELTY_DOCS_DIR") {
        return PathBuf::from(p);
    }
    let release = PathBuf::from("docs");
    if release.join("index.html").is_file() || release.is_dir() {
        release
    } else {
        PathBuf::from("target/docs")
    }
}

fn public() -> bool {
    // `true`/`1` makes `/docs` readable without signing in (default: staff only).
    std::env::var("PELTY_DOCS_PUBLIC").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn page(status: StatusCode, title: &str, text: &str) -> Response {
    (status, Html(format!("<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>{title}</title><body style=\"font:16px system-ui;max-width:36rem;margin:15vh auto;padding:0 1rem\"><h1>{title}</h1><p>{text}</p><p><a href=\"/\">PeltyCMS</a></p>"))).into_response()
}

async fn serve(State(s): State<AppState>, req: Request) -> Response {
    let (mut parts, _) = req.into_parts();
    if !public() {
        match CurrentUser::from_request_parts(&mut parts, &s).await {
            Err(_) => {
                return page(
                    StatusCode::UNAUTHORIZED,
                    "Sign in required",
                    "The developer docs are for staff. Sign in to the admin area first.",
                )
            }
            Ok(u) if !u.roles.iter().any(|r| r != "customer") => {
                return page(
                    StatusCode::FORBIDDEN,
                    "Staff only",
                    "The developer docs are only available to staff accounts.",
                )
            }
            Ok(_) => {}
        }
    }
    let rel = parts
        .uri
        .path()
        .trim_start_matches("/docs")
        .trim_start_matches('/');
    // A folder (`/docs/`, `/docs/developers/`) serves its index.html.
    let rel = if rel.is_empty() || rel.ends_with('/') {
        format!("{rel}index.html")
    } else {
        rel.to_string()
    };
    // The admin page probes the page index; answer "no pages" instead of a 404
    // so an unbuilt docs folder doesn't log errors in the browser console.
    let index_probe = rel == "developers/pages.json";
    let empty_index = || ([(header::CONTENT_TYPE, "application/json")], "[]").into_response();
    let Ok(root) = dir().canonicalize() else {
        if index_probe {
            return empty_index();
        }
        return page(
            StatusCode::NOT_FOUND,
            "Docs not built",
            "Set PELTY_DOCS_DIR, unpack the release <code>docs/</code> folder, or run <code>cargo xtask docs</code>.",
        );
    };
    let p = Path::new(&rel);
    if !p.components().all(|c| matches!(c, Component::Normal(_))) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(file) = root
        .join(p)
        .canonicalize()
        .ok()
        .filter(|f| f.starts_with(&root) && f.is_file())
    else {
        if index_probe {
            return empty_index();
        }
        return page(StatusCode::NOT_FOUND, "Not found", "No such docs page.");
    };
    match crate::static_files::serve_file(&file, &parts.headers, crate::static_files::NO_CACHE)
        .await
    {
        Some(mut r) => {
            // Docs never need framing or third-party content.
            r.headers_mut().insert(
                header::X_FRAME_OPTIONS,
                header::HeaderValue::from_static("DENY"),
            );
            r
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}
