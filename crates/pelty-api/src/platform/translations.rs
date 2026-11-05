//! Translation editor (WHMCS "Translation editor"): staff override any
//! shipped string per language; overrides win over `locales/*.json` in the
//! UI (`/i18n/{lang}`) and in server-rendered text (mails, errors).
//! Also manages the `languages` catalog (add a locale without a JSON file).
use crate::error::ApiResult;
use crate::extract::{perms, RequirePermission};
use crate::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use pelty_core::{AppError, AppResult};
use pelty_database::{audit, languages as langs_db, translations as db};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const MAX_LEN: usize = 4000;

/// Load DB languages + translation overrides into memory.
pub async fn reload(s: &AppState) -> AppResult<usize> {
    sync_languages(s).await?;
    let rows = db::all(&s.pool).await?;
    let n = rows.len();
    s.i18n.set_overrides(rows);
    Ok(n)
}

/// Register enabled DB languages that have no `locales/<code>.json`.
pub async fn sync_languages(s: &AppState) -> AppResult<()> {
    let rows = langs_db::list_enabled(&s.pool).await?;
    let extra: Vec<(String, String)> = rows
        .into_iter()
        .filter(|l| !s.i18n.has_file(&l.code))
        .map(|l| (l.code, l.name))
        .collect();
    s.i18n.set_extra_langs(extra);
    Ok(())
}

/// Placeholders as written: `{{name}}` (e-mail templates, server texts) and
/// `{name}` (UI texts). An override keeps the original's syntax, so a mail
/// variable never turns into one the template engine does not fill.
pub fn placeholders(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        if let Some(inner) = after.strip_prefix('{') {
            let Some(j) = inner.find("}}") else { break };
            let name = inner[..j].trim();
            if !name.is_empty() && name.len() <= 64 {
                out.insert(format!("{{{{{name}}}}}"));
            }
            rest = &inner[j + 2..];
            continue;
        }
        let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
        if end > 0 && end <= 64 && after[end..].starts_with('}') {
            out.insert(format!("{{{}}}", &after[..end]));
            rest = &after[end + 1..];
        } else {
            rest = after;
        }
    }
    out
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/admin/translations", get(list).put(put).delete(remove))
        .route("/admin/translations/export", get(export_json))
        .route("/admin/translations/import", post(import_json))
        .route("/admin/languages", get(list_languages).post(add_language))
        .route("/admin/languages/{code}", delete(remove_language))
}

fn normalize_code(raw: &str) -> Result<String, AppError> {
    let code = raw.trim().to_ascii_lowercase();
    if code.len() < 2
        || code.len() > 10
        || !code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
        || code.starts_with('-')
        || code.ends_with('-')
    {
        return Err(AppError::Validation("error.invalid_language"));
    }
    Ok(code)
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NewLanguage {
    pub code: String,
    pub name: String,
}

#[utoipa::path(get, path = "/api/v1/admin/languages", responses((status = 200), (status = 403)))]
pub async fn list_languages(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
) -> ApiResult<Json<Value>> {
    let items = langs_db::list(&s.pool).await?;
    Ok(Json(json!({ "items": items })))
}

#[utoipa::path(post, path = "/api/v1/admin/languages", request_body = NewLanguage, responses((status = 200), (status = 409), (status = 422)))]
pub async fn add_language(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<NewLanguage>,
) -> ApiResult<Json<Value>> {
    let code = normalize_code(&b.code)?;
    let name = b.name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Validation("error.invalid_language_name").into());
    }
    if langs_db::get(&s.pool, &code).await?.is_some() {
        return Err(AppError::Conflict("error.language_exists").into());
    }
    let row = langs_db::insert(&s.pool, &code, name).await?;
    // So the picker shows the right label before any other overrides exist.
    db::set(&s.pool, &code, "language.name", name, u.user_id).await?;
    reload(&s).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "language.created",
        Some(&code),
        u.ip.as_deref(),
        json!({ "name": name }),
    )
    .await?;
    Ok(Json(json!({ "language": row })))
}

#[utoipa::path(delete, path = "/api/v1/admin/languages/{code}", responses((status = 200), (status = 404), (status = 422)))]
pub async fn remove_language(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Path(code): Path<String>,
) -> ApiResult<Json<Value>> {
    let code = normalize_code(&code)?;
    let Some(row) = langs_db::get(&s.pool, &code).await? else {
        return Err(AppError::NotFound.into());
    };
    if row.built_in {
        return Err(AppError::Validation("error.language_builtin").into());
    }
    if !langs_db::delete(&s.pool, &code).await? {
        return Err(AppError::NotFound.into());
    }
    let wiped = langs_db::wipe_overrides(&s.pool, &code).await?;
    s.i18n.drop_lang(&code);
    reload(&s).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "language.deleted",
        Some(&code),
        u.ip.as_deref(),
        json!({ "overrides_removed": wiped }),
    )
    .await?;
    Ok(Json(json!({ "code": code, "overrides_removed": wiped })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQ {
    pub lang: String,
    #[serde(default)]
    pub q: String,
    /// `all` (default), `overridden`, or `missing` (no shipped text in this language).
    #[serde(default)]
    pub filter: String,
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

fn check_lang(s: &AppState, lang: &str) -> AppResult<()> {
    if s.i18n.has(lang) { Ok(()) } else { Err(AppError::Validation("error.unknown_language")) }
}

#[utoipa::path(get, path = "/api/v1/admin/translations", params(ListQ), responses((status = 200)))]
pub async fn list(State(s): State<AppState>, _p: RequirePermission<perms::SettingsManage>, Query(q): Query<ListQ>) -> ApiResult<Json<Value>> {
    check_lang(&s, &q.lang)?;
    let base = s.i18n.messages(pelty_core::i18n::BASE).cloned().unwrap_or_default();
    let file = s.i18n.messages(&q.lang).cloned().unwrap_or_default();
    let ov: HashMap<String, String> = db::for_lang(&s.pool, &q.lang).await?.into_iter().collect();
    let needle = q.q.trim().to_lowercase();
    let per = q.per_page.unwrap_or(50).clamp(1, 200) as usize;
    let page = q.page.unwrap_or(1).max(1) as usize;
    let rows: Vec<Value> = base
        .iter()
        .filter(|(k, _)| match q.filter.as_str() {
            "overridden" => ov.contains_key(*k),
            "missing" => !file.contains_key(*k),
            _ => true,
        })
        .filter(|(k, v)| {
            needle.is_empty() || k.to_lowercase().contains(&needle) || v.to_lowercase().contains(&needle)
                || file.get(*k).is_some_and(|f| f.to_lowercase().contains(&needle))
                || ov.get(*k).is_some_and(|o| o.to_lowercase().contains(&needle))
        })
        .map(|(k, v)| json!({ "key": k, "base": v, "file": file.get(k), "override": ov.get(k) }))
        .collect();
    let total = rows.len();
    let items: Vec<Value> = rows.into_iter().skip((page - 1) * per).take(per).collect();
    let languages: Vec<Value> = langs_db::list(&s.pool)
        .await?
        .into_iter()
        .map(|l| {
            json!({
                "code": l.code,
                "name": l.name,
                "built_in": l.built_in,
                "enabled": l.enabled,
            })
        })
        .collect();
    Ok(Json(json!({
        "items": items,
        "total": total,
        "page": page,
        "per_page": per,
        "overrides": ov.len(),
        "languages": s.i18n.languages(),
        "language_meta": languages,
    })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ExportQ {
    pub lang: String,
    /// `effective` (default): file + overrides, missing keys use English.
    /// `overrides`: only DB edits. `missing`: English text for keys with no file/override.
    #[serde(default = "default_export_mode")]
    pub mode: String,
}

fn default_export_mode() -> String {
    "effective".into()
}

/// Download a flat `locales`-style JSON for offline editing.
#[utoipa::path(get, path = "/api/v1/admin/translations/export", params(ExportQ), responses((status = 200, content_type = "application/json"), (status = 422)))]
pub async fn export_json(
    State(s): State<AppState>,
    _p: RequirePermission<perms::SettingsManage>,
    Query(q): Query<ExportQ>,
) -> ApiResult<Response> {
    check_lang(&s, &q.lang)?;
    let base = s.i18n.messages(pelty_core::i18n::BASE).cloned().unwrap_or_default();
    let file = s.i18n.messages(&q.lang).cloned().unwrap_or_default();
    let ov: HashMap<String, String> = db::for_lang(&s.pool, &q.lang).await?.into_iter().collect();
    let mut out = BTreeMap::new();
    match q.mode.as_str() {
        "overrides" => {
            for (k, v) in &ov {
                out.insert(k.clone(), v.clone());
            }
        }
        "missing" => {
            for (k, v) in &base {
                if !file.contains_key(k) && !ov.contains_key(k) {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        _ => {
            for (k, en) in &base {
                let v = ov
                    .get(k)
                    .cloned()
                    .or_else(|| file.get(k).cloned())
                    .unwrap_or_else(|| en.clone());
                out.insert(k.clone(), v);
            }
        }
    }
    let body = serde_json::to_vec_pretty(&out).map_err(|_| AppError::Internal("json".into()))?;
    let name = format!("pelty-{}-{}.json", q.lang, q.mode);
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
        ],
        body,
    )
        .into_response())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct ImportBody {
    pub lang: String,
    /// Flat key → text map (same shape as `locales/*.json`).
    pub entries: BTreeMap<String, String>,
}

/// Upsert entries as translation overrides. Unknown keys / bad placeholders are skipped (counted).
#[utoipa::path(post, path = "/api/v1/admin/translations/import", request_body = ImportBody, responses((status = 200), (status = 422)))]
pub async fn import_json(
    State(s): State<AppState>,
    RequirePermission(u, _): RequirePermission<perms::SettingsManage>,
    Json(b): Json<ImportBody>,
) -> ApiResult<Json<Value>> {
    check_lang(&s, &b.lang)?;
    if b.entries.is_empty() {
        return Err(AppError::Validation("error.translation_import_empty").into());
    }
    if b.entries.len() > 20_000 {
        return Err(AppError::Validation("error.translation_import_too_large").into());
    }
    let base = s.i18n.messages(pelty_core::i18n::BASE).cloned().unwrap_or_default();
    let mut saved = 0u32;
    let mut skipped = 0u32;
    for (key, value) in &b.entries {
        let Some(en) = base.get(key) else {
            skipped += 1;
            continue;
        };
        if value.trim().is_empty() || value.chars().count() > MAX_LEN {
            skipped += 1;
            continue;
        }
        if !placeholders(value).is_subset(&placeholders(en)) {
            skipped += 1;
            continue;
        }
        db::set(&s.pool, &b.lang, key, value, u.user_id).await?;
        saved += 1;
    }
    reload(&s).await?;
    audit::record(
        &s.pool,
        Some(u.user_id),
        "translation.imported",
        Some(&b.lang),
        u.ip.as_deref(),
        json!({ "saved": saved, "skipped": skipped }),
    )
    .await?;
    Ok(Json(json!({ "lang": b.lang, "saved": saved, "skipped": skipped })))
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct PutBody {
    pub lang: String,
    pub key: String,
    pub value: String,
}

#[utoipa::path(put, path = "/api/v1/admin/translations", request_body = PutBody, responses((status = 200), (status = 422)))]
pub async fn put(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Json(b): Json<PutBody>) -> ApiResult<Json<Value>> {
    check_lang(&s, &b.lang)?;
    let base = s.i18n.messages(pelty_core::i18n::BASE).and_then(|m| m.get(&b.key)).cloned().ok_or(AppError::Validation("error.unknown_translation_key"))?;
    if b.value.trim().is_empty() || b.value.chars().count() > MAX_LEN {
        return Err(AppError::Validation("error.translation_value_invalid").into());
    }
    // Templates only know the variables the shipped text uses.
    if !placeholders(&b.value).is_subset(&placeholders(&base)) {
        return Err(AppError::Validation("error.translation_placeholder").into());
    }
    db::set(&s.pool, &b.lang, &b.key, &b.value, u.user_id).await?;
    reload(&s).await?;
    audit::record(&s.pool, Some(u.user_id), "translation.override_set", Some(&format!("{}:{}", b.lang, b.key)), u.ip.as_deref(), json!({ "lang": b.lang, "key": b.key })).await?;
    Ok(Json(json!({ "key": b.key, "lang": b.lang, "override": b.value, "effective": s.i18n.t(&b.lang, &b.key) })))
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct DelQ {
    pub lang: String,
    pub key: String,
}

#[utoipa::path(delete, path = "/api/v1/admin/translations", params(DelQ), responses((status = 200), (status = 404)))]
pub async fn remove(State(s): State<AppState>, RequirePermission(u, _): RequirePermission<perms::SettingsManage>, Query(q): Query<DelQ>) -> ApiResult<Json<Value>> {
    if !db::delete(&s.pool, &q.lang, &q.key).await? {
        return Err(AppError::NotFound.into());
    }
    reload(&s).await?;
    audit::record(&s.pool, Some(u.user_id), "translation.override_removed", Some(&format!("{}:{}", q.lang, q.key)), u.ip.as_deref(), json!({})).await?;
    Ok(Json(json!({ "key": q.key, "lang": q.lang, "effective": s.i18n.t(&q.lang, &q.key) })))
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholders() {
        let p = super::placeholders("Hi {{name}}, pay {{ amount }} by {{due}} {{");
        assert_eq!(p.into_iter().collect::<Vec<_>>(), vec!["{{amount}}", "{{due}}", "{{name}}"]);
        let p = super::placeholders("PeltyCMS {version} for {{company}}; { css } {a-b} {x");
        assert_eq!(p.into_iter().collect::<Vec<_>>(), vec!["{version}", "{{company}}"]);
        assert!(super::placeholders("none").is_empty());
    }
}
