//! Update checker ("automatic updates", WHMCS system/updates).
//!
//! A daily job reads the release feed, compares it with the running version
//! and shows staff a banner with the release notes link; `pelty-server
//! update` (and the console `update`) uses the same feed, token and parser
//! to download, verify and install the new binary.
//!
//! Channels: **stable** uses GitHub `releases/latest`; **nightly** lists
//! recent releases and picks the newest `*-nightly.*` tag so prereleases
//! surface without becoming GitHub "Latest". Docker installs get a pull
//! hint; binary installs point at `pelty-server update`.
//!
//! Feed override: `PELTY_UPDATE_URL` (mirror or test). Private repos need
//! `PELTY_UPDATE_TOKEN` (or `GH_TOKEN` / `GITHUB_TOKEN`).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
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
pub const RELEASES_LIST_URL: &str =
    "https://api.github.com/repos/MythicalLTD/PeltyCMS/releases?per_page=40";
pub const DOCKER_IMAGE: &str = "ghcr.io/mythicalltd/peltycms";
const CFG: &str = "update_check";
const STATUS: &str = "update_status";

/// Stable = GitHub Latest; Nightly = newest dated nightly prerelease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Stable,
    Nightly,
}

impl Channel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Nightly => "nightly",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("nightly") {
            Self::Nightly
        } else {
            Self::Stable
        }
    }
}

/// Resolved update-check settings.
#[derive(Debug, Clone)]
pub struct CheckConfig {
    pub enabled: bool,
    pub channel: Channel,
    /// Custom feed URL (empty = official feed for `channel`).
    pub custom_url: String,
}

impl CheckConfig {
    /// HTTP URL to GET for this check.
    pub fn fetch_url(&self) -> String {
        if let Some(u) = env_url() {
            return u;
        }
        if !self.custom_url.is_empty() {
            return self.custom_url.clone();
        }
        match self.channel {
            Channel::Stable => DEFAULT_URL.into(),
            Channel::Nightly => RELEASES_LIST_URL.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PrePart {
    Num(u64),
    Text(String),
}

impl PartialOrd for PrePart {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PrePart {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (self, other) {
            (Self::Num(a), Self::Num(b)) => a.cmp(b),
            (Self::Text(a), Self::Text(b)) => a.cmp(b),
            (Self::Num(_), Self::Text(_)) => Less,
            (Self::Text(_), Self::Num(_)) => Greater,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Ver {
    major: u64,
    minor: u64,
    patch: u64,
    /// `None` = release (sorts above any prerelease of the same core).
    pre: Option<Vec<PrePart>>,
}

impl PartialOrd for Ver {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Ver {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        match (
            self.major.cmp(&other.major),
            self.minor.cmp(&other.minor),
            self.patch.cmp(&other.patch),
        ) {
            (Equal, Equal, Equal) => match (&self.pre, &other.pre) {
                (None, None) => Equal,
                (None, Some(_)) => Greater,
                (Some(_), None) => Less,
                (Some(a), Some(b)) => {
                    for (x, y) in a.iter().zip(b.iter()) {
                        match x.cmp(y) {
                            Equal => {}
                            o => return o,
                        }
                    }
                    a.len().cmp(&b.len())
                }
            },
            (Equal, Equal, o) => o,
            (Equal, o, _) => o,
            (o, _, _) => o,
        }
    }
}

fn parse_ver(v: &str) -> Option<Ver> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre_raw) = match v.split_once('+') {
        Some((c, _)) => match c.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (c, None),
        },
        None => match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (v, None),
        },
    };
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().map_or(Some(0), |x| x.parse().ok())?;
    let patch = it.next().map_or(Some(0), |x| x.parse().ok())?;
    if it.next().is_some() {
        return None;
    }
    let pre = pre_raw.map(|p| {
        p.split('.')
            .filter(|s| !s.is_empty())
            .map(|s| {
                if s.chars().all(|c| c.is_ascii_digit()) {
                    PrePart::Num(s.parse().unwrap_or(0))
                } else {
                    PrePart::Text(s.to_ascii_lowercase())
                }
            })
            .collect::<Vec<_>>()
    });
    if pre.as_ref().is_some_and(|p| p.is_empty()) {
        return None;
    }
    Some(Ver {
        major,
        minor,
        patch,
        pre,
    })
}

/// `v1.2.3`, `1.2`, `1.2.3-rc.1` → (major, minor, patch, is_release).
pub fn parse_version(v: &str) -> Option<(u64, u64, u64, bool)> {
    let ver = parse_ver(v)?;
    Some((ver.major, ver.minor, ver.patch, ver.pre.is_none()))
}

/// Is `latest` strictly newer than `current`? Releases beat prereleases of the
/// same core; prerelease ids compare SemVer-style (`nightly.20261107` >
/// `nightly.20261106`).
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_ver(latest), parse_ver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

pub fn is_nightly_version(v: &str) -> bool {
    let v = v.trim().trim_start_matches(['v', 'V']);
    v.contains("-nightly.")
}

/// Binary install vs Docker/OCI (`/.dockerenv` or `PELTY_DOCKER=1|true`).
pub fn install_kind() -> &'static str {
    let marked = std::env::var("PELTY_DOCKER").is_ok_and(|v| v == "1" || v == "true");
    if std::path::Path::new("/.dockerenv").exists() || marked {
        "docker"
    } else {
        "binary"
    }
}

pub async fn config(s: &AppState) -> AppResult<CheckConfig> {
    config_in(&s.pool).await
}

pub async fn config_in(pool: &pelty_database::Pool) -> AppResult<CheckConfig> {
    let v = settings::get(pool, CFG).await?.unwrap_or(Value::Null);
    let custom = v["url"].as_str().unwrap_or("").trim().to_string();
    let channel = match v["channel"].as_str() {
        Some(c) => Channel::parse(c),
        None if is_nightly_version(CURRENT) => Channel::Nightly,
        None => Channel::Stable,
    };
    Ok(CheckConfig {
        enabled: v["enabled"].as_bool().unwrap_or(true),
        channel,
        custom_url: custom,
    })
}

/// `PELTY_UPDATE_URL`: release feed override (a mirror or a test server).
pub fn env_url() -> Option<String> {
    std::env::var("PELTY_UPDATE_URL")
        .ok()
        .map(|u| u.trim().to_string())
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

/// Token for a private release feed: `PELTY_UPDATE_TOKEN`, else `GH_TOKEN` / `GITHUB_TOKEN`.
pub fn token() -> Option<String> {
    ["PELTY_UPDATE_TOKEN", "GH_TOKEN", "GITHUB_TOKEN"]
        .iter()
        .find_map(|k| {
            std::env::var(k)
                .ok()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        })
}

/// Request headers for the feed (and, with `binary`, for asset downloads).
pub fn feed_headers(binary: bool) -> Vec<(String, String)> {
    let accept = if binary {
        "application/octet-stream"
    } else {
        "application/vnd.github+json, application/json"
    };
    let mut h = vec![
        ("accept".into(), accept.into()),
        ("user-agent".into(), format!("PeltyCMS/{CURRENT}")),
    ];
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
    if let Some(base) = url.split_once("/releases?").map(|(b, _)| b) {
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
            archive
                && (a.target.as_deref() == Some(target) || a.name.contains(&format!("-{target}.")))
        })
    }
}

/// Parse a GitHub release JSON or a `manifest.json` (asset URLs relative to it).
pub fn parse_feed(body: &str, feed_url: &str) -> Result<Release, &'static str> {
    let v: Value = serde_json::from_str(body).map_err(|_| "error.update_feed_invalid")?;
    parse_feed_value(&v, feed_url)
}

fn parse_feed_value(v: &Value, feed_url: &str) -> Result<Release, &'static str> {
    let tag = v["tag_name"]
        .as_str()
        .or(v["version"].as_str())
        .unwrap_or("");
    if parse_version(tag).is_none() {
        return Err("error.update_feed_invalid");
    }
    let github = v.get("tag_name").is_some();
    let base = feed_url
        .rsplit_once('/')
        .map(|(b, _)| b)
        .unwrap_or(feed_url);
    let use_api = token().is_some();
    let assets = v["assets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|a| {
            let name = a["name"].as_str()?.to_string();
            if name.contains('/') || name.contains("..") {
                return None;
            }
            let url = if github {
                let api = a["url"].as_str().filter(|_| use_api);
                api.or(a["browser_download_url"].as_str())?.to_string()
            } else {
                a["url"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{base}/{name}"))
            };
            Some(Asset {
                name,
                url,
                target: a["target"].as_str().map(str::to_string),
                sha256: a["sha256"].as_str().map(str::to_string),
            })
        })
        .collect();
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        notes: v["body"]
            .as_str()
            .or(v["notes"].as_str())
            .unwrap_or("")
            .chars()
            .take(4000)
            .collect(),
        html_url: v["html_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .map(str::to_string),
        published_at: v["published_at"].clone(),
        assets,
    })
}

/// Parse a single release object or a GitHub releases **array**, picking the
/// newest release that matches `channel`.
pub fn pick_release(body: &str, feed_url: &str, channel: Channel) -> Result<Release, &'static str> {
    let v: Value = serde_json::from_str(body).map_err(|_| "error.update_feed_invalid")?;
    if let Some(arr) = v.as_array() {
        let mut best: Option<Release> = None;
        for item in arr {
            let Ok(r) = parse_feed_value(item, feed_url) else {
                continue;
            };
            let nightly = is_nightly_version(&r.version);
            match channel {
                Channel::Nightly if !nightly => continue,
                Channel::Stable if nightly => continue,
                _ => {}
            }
            if best
                .as_ref()
                .is_none_or(|b| is_newer(&r.version, &b.version))
            {
                best = Some(r);
            }
        }
        return best.ok_or("error.update_feed_invalid");
    }
    let r = parse_feed_value(&v, feed_url)?;
    if channel == Channel::Nightly && !is_nightly_version(&r.version) {
        return Err("error.update_feed_invalid");
    }
    Ok(r)
}

fn docker_tag_for(channel: Channel, latest: &str) -> String {
    if latest.is_empty() {
        return match channel {
            Channel::Nightly => "nightly".into(),
            Channel::Stable => "latest".into(),
        };
    }
    if channel == Channel::Nightly {
        "nightly".into()
    } else {
        format!("v{}", latest.trim_start_matches(['v', 'V']))
    }
}

fn update_command(install: &str, channel: Channel, latest: &str) -> String {
    if install == "docker" {
        let tag = docker_tag_for(channel, latest);
        format!("docker pull {DOCKER_IMAGE}:{tag} && docker compose up -d")
    } else if latest.is_empty() {
        "pelty-server update --check".into()
    } else {
        format!(
            "pelty-server update --yes --version {}",
            latest.trim_start_matches(['v', 'V'])
        )
    }
}

pub async fn status(s: &AppState) -> AppResult<Value> {
    let cfg = config(s).await?;
    let st = settings::get(&s.pool, STATUS).await?.unwrap_or(json!({}));
    let latest = st["latest"].as_str().unwrap_or("").to_string();
    let install = install_kind();
    let available = !latest.is_empty() && is_newer(&latest, CURRENT);
    Ok(json!({
        "enabled": cfg.enabled,
        "channel": cfg.channel.as_str(),
        "url": cfg.fetch_url(),
        "custom_url": cfg.custom_url,
        "current": CURRENT,
        "latest": st["latest"],
        "release_url": st["release_url"],
        "notes": st["notes"],
        "published_at": st["published_at"],
        "checked_at": st["checked_at"],
        "error": st["error"],
        "update_available": available,
        "install": install,
        "docker_image": DOCKER_IMAGE,
        "docker_tag": docker_tag_for(cfg.channel, &latest),
        "update_command": update_command(install, cfg.channel, &latest),
    }))
}

/// Fetch the feed now and store the outcome (errors too, so the page shows them).
pub async fn check(s: &AppState) -> AppResult<Value> {
    let cfg = config(s).await?;
    let url = cfg.fetch_url();
    let now = OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let mut st = settings::get(&s.pool, STATUS).await?.unwrap_or(json!({}));
    st["checked_at"] = json!(now);
    let res = s
        .http()
        .send(HttpRequestSpec {
            method: "GET",
            url: url.clone(),
            headers: feed_headers(false),
            body: String::new(),
        })
        .await;
    let outcome = match res {
        Ok((200..=299, body)) => pick_release(&body, &url, cfg.channel),
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
                audit::record(
                    &s.pool,
                    None,
                    "system.update_available",
                    None,
                    None,
                    json!({ "current": CURRENT, "latest": tag, "channel": cfg.channel.as_str() }),
                )
                .await?;
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
    if !config(s).await?.enabled {
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
pub async fn get_status(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    Ok(Json(status(&s).await?))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct UpdateConfig {
    pub enabled: bool,
    /// Empty = the official feed for `channel`.
    #[serde(default)]
    pub url: String,
    /// `stable` (default) or `nightly`.
    #[serde(default = "default_channel_field")]
    pub channel: String,
}

fn default_channel_field() -> String {
    "stable".into()
}

#[utoipa::path(put, path = "/api/v1/admin/system/updates", request_body = UpdateConfig, responses((status = 200), (status = 422)))]
pub async fn put_config(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<UpdateConfig>,
) -> ApiResult<Json<Value>> {
    let url = b.url.trim();
    if !url.is_empty()
        && (!url.starts_with("https://") || url.len() > 500 || url.contains(char::is_whitespace))
    {
        return Err(AppError::Validation("error.update_url_invalid").into());
    }
    let channel = Channel::parse(&b.channel);
    settings::set(
        &s.pool,
        CFG,
        json!({ "enabled": b.enabled, "url": url, "channel": channel.as_str() }),
    )
    .await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "settings.update_check",
        None,
        u.ip.as_deref(),
        json!({ "enabled": b.enabled, "url": url, "channel": channel.as_str() }),
    )
    .await?;
    Ok(Json(status(&s).await?))
}

#[utoipa::path(post, path = "/api/v1/admin/system/updates/check", responses((status = 200)))]
pub async fn check_now(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
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
        assert!(is_newer("0.1.0-nightly.20261107", "0.1.0-nightly.20261106"));
        assert!(!is_newer(
            "0.1.0-nightly.20261106",
            "0.1.0-nightly.20261107"
        ));
        assert!(is_newer("0.1.0", "0.1.0-nightly.20261107"));
        assert!(is_nightly_version("v0.1.0-nightly.20261106"));
        assert!(!is_nightly_version("0.1.0-rc.1"));
    }

    #[test]
    fn picks_newest_nightly_from_list() {
        let body = r#"[
          {"tag_name":"v1.0.0","body":"stable","html_url":"https://github.com/x/y/releases/tag/v1.0.0","assets":[]},
          {"tag_name":"v0.1.0-nightly.20261105","body":"old","html_url":"https://github.com/x/y/releases/tag/v0.1.0-nightly.20261105","assets":[{"name":"SHA256SUMS","browser_download_url":"https://x/a"}]},
          {"tag_name":"v0.1.0-nightly.20261106","body":"new","html_url":"https://github.com/x/y/releases/tag/v0.1.0-nightly.20261106","assets":[{"name":"SHA256SUMS","browser_download_url":"https://x/b"}]}
        ]"#;
        let r = pick_release(body, RELEASES_LIST_URL, Channel::Nightly).unwrap();
        assert_eq!(r.version, "0.1.0-nightly.20261106");
        let s = pick_release(body, RELEASES_LIST_URL, Channel::Stable).unwrap();
        assert_eq!(s.version, "1.0.0");
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
        assert_eq!(
            a.url,
            "http://127.0.0.1:9/rel/latest/pelty-server-0.3.0-x86_64-pc-windows-msvc.zip"
        );
        assert_eq!(a.sha256.as_deref(), Some("ab"));
        assert_eq!(
            parse_feed("{\"version\":\"x\"}", "u").err(),
            Some("error.update_feed_invalid")
        );
        assert_eq!(
            feed_for_version(DEFAULT_URL, "v1.2.0").unwrap(),
            "https://api.github.com/repos/MythicalLTD/PeltyCMS/releases/tags/v1.2.0"
        );
        assert_eq!(
            feed_for_version("https://m.example/p/latest/manifest.json", "1.2.0").unwrap(),
            "https://m.example/p/1.2.0/manifest.json"
        );
        assert_eq!(
            feed_for_version("https://m.example/p/manifest.json", "1.2.0"),
            None
        );
        assert_eq!(
            feed_for_version(RELEASES_LIST_URL, "0.1.0-nightly.1").unwrap(),
            "https://api.github.com/repos/MythicalLTD/PeltyCMS/releases/tags/v0.1.0-nightly.1"
        );
    }
}
