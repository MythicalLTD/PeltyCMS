//! Update checker ("automatic updates", WHMCS system/updates).
//!
//! A daily job reads the release feed, compares it with the running version
//! and shows staff a banner with the release notes link; `pelty-server
//! update` (and the console `update`) uses the same feed, token and parser
//! to download, verify and install the new binary.
//!
//! Feed: `PELTY_UPDATE_URL` when set (any http(s) URL: a GitHub
//! `releases/latest` API URL or a mirror's `manifest.json` as written by
//! `cargo xtask release`), else the URL saved in Admin > System > Updates,
//! else the official GitHub feed. The repository is private, so a token from
//! `PELTY_UPDATE_TOKEN` (or `GH_TOKEN` / `GITHUB_TOKEN`) is sent as a bearer
//! token when set.
use crate::extract::{perms, RequirePermission};
use crate::error::ApiResult;
use crate::AppState;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, jobs, settings};
use pelty_payments::HttpRequestSpec;
use serde::Deserialize;
use serde_json::{json, Value};
use time::OffsetDateTime;

pub const JOB_KIND: &str = "system.update_check";
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
pub const DEFAULT_URL: &str = "https://api.github.com/repos/MythicalLTD/PeltyCMS/releases/latest";
const CFG: &str = "update_check";
const STATUS: &str = "update_status";

/// `v1.2.3`, `1.2`, `1.2.3-rc.1` → (major, minor, patch, is_release).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, bool)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((c, rest)) => (c, !rest.is_empty() && v.contains('-')),
        None => (v, false),
    };
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next().map_or(Some(0), |x| x.parse().ok())?;
    let c = it.next().map_or(Some(0), |x| x.parse().ok())?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c, !pre))
}

/// Is `latest` strictly newer than `current`? A pre-release of the same
/// version is older than the release.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub async fn config(s: &AppState) -> AppResult<(bool, String)> {
    config_in(&s.pool).await
}

/// `(enabled, feed URL)`; `PELTY_UPDATE_URL` wins over the saved URL.
pub async fn config_in(pool: &pelty_database::Pool) -> AppResult<(bool, String)> {
    let v = settings::get(pool, CFG).await?.unwrap_or(Value::Null);
    let saved = v["url"].as_str().filter(|u| !u.is_empty()).unwrap_or(DEFAULT_URL).to_string();
    Ok((v["enabled"].as_bool().unwrap_or(true), env_url().unwrap_or(saved)))
}

/// `PELTY_UPDATE_URL`: release feed override (a mirror or a test server).
pub fn env_url() -> Option<String> {
    // Release feed URL override: a GitHub releases API URL or a mirror's manifest.json.
    std::env::var("PELTY_UPDATE_URL").ok().map(|u| u.trim().to_string()).filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

/// Token for a private release feed: `PELTY_UPDATE_TOKEN`, else `GH_TOKEN` / `GITHUB_TOKEN`.
pub fn token() -> Option<String> {
    // Token for the private release feed (`PELTY_UPDATE_TOKEN`, else GH_TOKEN / GITHUB_TOKEN).
    ["PELTY_UPDATE_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"].iter().find_map(|k| std::env::var(k).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()))
}

/// Request headers for the feed (and, with `binary`, for asset downloads).
pub fn feed_headers(binary: bool) -> Vec<(String, String)> {
    let accept = if binary { "application/octet-stream" } else { "application/vnd.github+json, application/json" };
    let mut h = vec![("accept".into(), accept.into()), ("user-agent".into(), format!("PeltyCMS/{CURRENT}"))];
    if let Some(t) = token() {
        h.push(("authorization".into(), format!("Bearer {t}")));
    }
    h
}

/// The feed URL for one specific version: GitHub `.../releases/latest` →
/// `.../releases/tags/v<version>`; a mirror `.../latest/manifest.json` →
/// `.../<version>/manifest.json` (the `dist/<version>/` layout).
pub fn feed_for_version(url: &str, version: &str) -> Option<String> {
    let v = version.trim_start_matches(['v', 'V']);
    if let Some(base) = url.strip_suffix("/releases/latest") {
        return Some(format!("{base}/releases/tags/v{v}"));
    }
    let (dir, file) = url.rsplit_once('/')?;
    let (base, last) = dir.rsplit_once('/')?;
    (last == "latest").then(|| format!("{base}/{v}/{file}"))
}

/// One downloadable file of a release.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Rust target triple (manifest feeds; else guessed from the name).
    pub target: Option<String>,
    pub sha256: Option<String>,
}

/// A release as read from either feed format.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub html_url: Option<String>,
    pub published_at: Value,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }

    /// The archive for `target` (`pelty-server-<v>-<target>.tar.gz|.zip`).
    pub fn archive_for(&self, target: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| {
            let archive = a.name.ends_with(".tar.gz") || a.name.ends_with(".zip");
            archive && (a.target.as_deref() == Some(target) || a.name.contains(&format!("-{target}.")))
        })
    }
}

/// Parse a GitHub release JSON or a `manifest.json` (asset URLs relative to it).
pub fn parse_feed(body: &str, feed_url: &str) -> Result<Release, &'static str> {
    let v: Value = serde_json::from_str(body).map_err(|_| "error.update_feed_invalid")?;
    let tag = v["tag_name"].as_str().or(v["version"].as_str()).unwrap_or("");
    if parse_version(tag).is_none() {
        return Err("error.update_feed_invalid");
    }
    let github = v.get("tag_name").is_some();
    let base = feed_url.rsplit_once('/').map(|(b, _)| b).unwrap_or(feed_url);
    let use_api = token().is_some();
    let assets = v["assets"].as_array().map(Vec::as_slice).unwrap_or_default().iter().filter_map(|a| {
        let name = a["name"].as_str()?.to_string();
        if name.contains('/') || name.contains("..") {
            return None;
        }
        let url = if github {
            // Private repos: the API URL with a token (browser URLs need a session).
            let api = a["url"].as_str().filter(|_| use_api);
            api.or(a["browser_download_url"].as_str())?.to_string()
        } else {
            a["url"].as_str().map(str::to_string).unwrap_or_else(|| format!("{base}/{name}"))
        };
        Some(Asset { name, url, target: a["target"].as_str().map(str::to_string), sha256: a["sha256"].as_str().map(str::to_string) })
    }).collect();
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"].as_str().or(v["notes"].as_str()).unwrap_or("").chars().take(4000).collect(),
        html_url: v["html_url"].as_str().filter(|u| u.starts_with("https://")).map(str::to_string),
        published_at: v["published_at"].clone(),
        assets,
    })
}

pub async fn status(s: &AppState) -> AppResult<Value> {
    let (enabled, url) = config(s).await?;
    let st = settings::get(&s.pool, STATUS).await?.unwrap_or(json!({}));
    let latest = st["latest"].as_str().unwrap_or("");
    Ok(json!({
        "enabled": enabled, "url": url, "current": CURRENT,
        "latest": st["latest"], "release_url": st["release_url"], "notes": st["notes"], "published_at": st["published_at"],
        "checked_at": st["checked_at"], "error": st["error"],
        "update_available": !latest.is_empty() && is_newer(latest, CURRENT),
    }))
}

/// Fetch the feed now and store the outcome (errors too, so the page shows them).
pub async fn check(s: &AppState) -> AppResult<Value> {
    let (_, url) = config(s).await?;
    let now = OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
    let mut st = settings::get(&s.pool, STATUS).await?.unwrap_or(json!({}));
    st["checked_at"] = json!(now);
    let res = s.http().send(HttpRequestSpec { method: "GET", url: url.clone(), headers: feed_headers(false), body: String::new() }).await;
    let outcome = match res {
        Ok((200..=299, body)) => parse_feed(&body, &url),
        _ => Err("error.update_feed_unavailable"),
    };
    match outcome {
        Ok(r) => {
            let tag = r.version.clone();
            let was = st["latest"].as_str().map(str::to_string);
            st["latest"] = json!(tag);
            st["notes"] = json!(r.notes);
            st["release_url"] = json!(r.html_url);
            st["published_at"] = r.published_at.clone();
            st["error"] = Value::Null;
            settings::set(&s.pool, STATUS, st).await?;
            if is_newer(&tag, CURRENT) && was.as_deref() != Some(tag.as_str()) {
                audit::record(&s.pool, None, "system.update_available", None, None, json!({ "current": CURRENT, "latest": tag })).await?;
            }
        }
        Err(code) => {
            st["error"] = json!(code);
            settings::set(&s.pool, STATUS, st).await?;
        }
    }
    status(s).await
}

pub async fn schedule(s: &AppState) -> AppResult<Option<i64>> {
    if !config(s).await?.0 {
        return Ok(None);
    }
    let key = format!("{JOB_KIND}:{}", OffsetDateTime::now_utc().date());
    jobs::enqueue(&s.pool, JOB_KIND, json!({}), None, Some(&key), 2).await
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/system/updates", get(get_status).put(put_config))
        .route("/admin/system/updates/check", post(check_now))
}

#[utoipa::path(get, path = "/api/v1/admin/system/updates", responses((status = 200)))]
pub async fn get_status(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(status(&s).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateConfig {
    pub enabled: bool,
    /// Empty = the official release feed.
    #[serde(default)]
    pub url: String,
}

#[utoipa::path(put, path = "/api/v1/admin/system/updates", request_body = UpdateConfig, responses((status = 200), (status = 422)))]
pub async fn put_config(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<UpdateConfig>) -> ApiResult<Json<Value>> {
    let url = b.url.trim();
    if !url.is_empty() && (!url.starts_with("https://") || url.len() > 500 || url.contains(char::is_whitespace)) {
        return Err(AppError::Validation("error.update_url_invalid").into());
    }
    settings::set(&s.pool, CFG, json!({ "enabled": b.enabled, "url": url })).await?;
    audit::record(&s.pool, Some(u.user_id), "settings.update_check", None, u.ip.as_deref(), json!({ "enabled": b.enabled, "url": url })).await?;
    Ok(Json(status(&s).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/system/updates/check", responses((status = 200)))]
pub async fn check_now(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>) -> ApiResult<Json<Value>> {
    Ok(Json(check(&s).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn versions() {
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3, true)));
        assert_eq!(parse_version("2.0"), Some((2, 0, 0, true)));
        assert_eq!(parse_version("1.0.0-rc.1"), Some((1, 0, 0, false)));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-rc.1", "0.1.0"));
        assert!(is_newer("0.1.0", "0.1.0-rc.1"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
    }

    #[test]
    fn feeds_and_version_urls() {
        let gh = r#"{"tag_name":"v0.3.0","body":"notes","html_url":"https://github.com/x/y/releases/tag/v0.3.0","assets":[
            {"name":"pelty-server-0.3.0-x86_64-unknown-linux-gnu.tar.gz","url":"https://api.github.com/repos/x/y/releases/assets/1","browser_download_url":"https://github.com/x/y/releases/download/v0.3.0/pelty-server-0.3.0-x86_64-unknown-linux-gnu.tar.gz"},
            {"name":"SHA256SUMS","url":"https://api.github.com/repos/x/y/releases/assets/2","browser_download_url":"https://github.com/b"},
            {"name":"../evil","browser_download_url":"https://e"}]}"#;
        let r = parse_feed(gh, DEFAULT_URL).unwrap();
        assert_eq!(r.version, "0.3.0");
        assert_eq!(r.assets.len(), 2);
        assert!(r.archive_for("x86_64-unknown-linux-gnu").is_some());
        assert!(r.archive_for("aarch64-apple-darwin").is_none());
        let m = r#"{"version":"0.3.0","notes":"n","assets":[{"target":"x86_64-pc-windows-msvc","name":"pelty-server-0.3.0-x86_64-pc-windows-msvc.zip","sha256":"ab"}]}"#;
        let r = parse_feed(m, "http://127.0.0.1:9/rel/latest/manifest.json").unwrap();
        let a = r.archive_for("x86_64-pc-windows-msvc").unwrap();
        assert_eq!(a.url, "http://127.0.0.1:9/rel/latest/pelty-server-0.3.0-x86_64-pc-windows-msvc.zip");
        assert_eq!(a.sha256.as_deref(), Some("ab"));
        assert_eq!(parse_feed("{\"version\":\"x\"}", "u").err(), Some("error.update_feed_invalid"));
        assert_eq!(feed_for_version(DEFAULT_URL, "v1.2.0").unwrap(), "https://api.github.com/repos/MythicalLTD/PeltyCMS/releases/tags/v1.2.0");
        assert_eq!(feed_for_version("https://m.example/p/latest/manifest.json", "1.2.0").unwrap(), "https://m.example/p/1.2.0/manifest.json");
        assert_eq!(feed_for_version("https://m.example/p/manifest.json", "1.2.0"), None);
    }
}
