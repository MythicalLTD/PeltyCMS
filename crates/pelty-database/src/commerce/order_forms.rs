//! Order form templates (WHMCS "order form templates"): which products or
//! groups a form sells, its step layout, extra fields and the post-order
//! redirect. Groups can pin a form; the client order page renders it.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

pub const LAYOUTS: &[&str] = &["cards", "list", "compact", "wizard"];
pub const STEPS: &[&str] = &["product", "configure", "domain", "addons", "review", "checkout"];
pub const FIELD_KINDS: &[&str] = &["text", "textarea", "select", "checkbox", "email", "number"];

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct OrderForm {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub layout: String,
    pub product_ids: Vec<Uuid>,
    pub group_ids: Vec<Uuid>,
    pub steps: Vec<String>,
    #[schema(value_type = Object)]
    pub fields: Value,
    pub redirect_url: Option<String>,
    pub active: bool,
}

const COLS: &str = "id, slug, name, layout, product_ids, group_ids, steps, fields, redirect_url, active";

#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FormInput {
    pub slug: String,
    pub name: String,
    #[serde(default = "cards")]
    pub layout: String,
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
    #[serde(default)]
    pub group_ids: Vec<Uuid>,
    #[serde(default)]
    pub steps: Vec<String>,
    #[serde(default)]
    pub fields: Vec<Field>,
    #[serde(default)]
    pub redirect_url: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
}

fn cards() -> String {
    "cards".into()
}
fn yes() -> bool {
    true
}

pub fn validate(i: &FormInput) -> AppResult<()> {
    let slug_ok = !i.slug.is_empty()
        && i.slug.len() <= 64
        && i.slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !i.slug.starts_with('-');
    if !slug_ok || i.name.trim().is_empty() || i.name.len() > 200 || !LAYOUTS.contains(&i.layout.as_str()) {
        return Err(AppError::Validation("error.validation"));
    }
    if i.steps.iter().any(|s| !STEPS.contains(&s.as_str())) || (!i.steps.is_empty() && !i.steps.iter().any(|s| s == "checkout")) {
        return Err(AppError::Validation("error.order_form_steps"));
    }
    if i.fields.len() > 30 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut keys = std::collections::HashSet::new();
    for f in &i.fields {
        let key_ok = !f.key.is_empty() && f.key.len() <= 40 && f.key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !key_ok || !keys.insert(f.key.clone()) || f.label.trim().is_empty() || f.label.len() > 200 || !FIELD_KINDS.contains(&f.kind.as_str()) || (f.kind == "select" && f.options.is_empty()) {
            return Err(AppError::Validation("error.order_form_field"));
        }
    }
    if let Some(u) = i.redirect_url.as_deref().filter(|u| !u.is_empty()) {
        let ok = u.len() <= 500 && ((u.starts_with('/') && !u.starts_with("//")) || u.starts_with("https://"));
        if !ok {
            return Err(AppError::Validation("error.order_form_redirect"));
        }
    }
    Ok(())
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<OrderForm>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM order_forms ORDER BY name, id")).fetch_all(pool).await.map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<OrderForm>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM order_forms WHERE id = $1")).bind(id).fetch_optional(pool).await.map_err(db_err)
}

pub async fn by_slug(pool: &PgPool, slug: &str) -> AppResult<Option<OrderForm>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM order_forms WHERE slug = $1 AND active")).bind(slug).fetch_optional(pool).await.map_err(db_err)
}

pub async fn for_group(pool: &PgPool, group: Uuid) -> AppResult<Option<OrderForm>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM order_forms WHERE active AND id = (SELECT order_form_id FROM product_groups WHERE id = $1)"))
        .bind(group)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn save(pool: &PgPool, id: Option<Uuid>, i: &FormInput) -> AppResult<OrderForm> {
    validate(i)?;
    let steps: Vec<String> = if i.steps.is_empty() { ["product", "configure", "domain", "review", "checkout"].map(String::from).to_vec() } else { i.steps.clone() };
    let fields = serde_json::to_value(&i.fields).map_err(|_| AppError::Validation("error.validation"))?;
    let redirect = i.redirect_url.as_deref().filter(|u| !u.is_empty());
    let q = match id {
        None => format!("INSERT INTO order_forms (id, slug, name, layout, product_ids, group_ids, steps, fields, redirect_url, active) VALUES (gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {COLS}"),
        Some(_) => format!("UPDATE order_forms SET slug=$1, name=$2, layout=$3, product_ids=$4, group_ids=$5, steps=$6, fields=$7, redirect_url=$8, active=$9, updated_at=now() WHERE id = $10 RETURNING {COLS}"),
    };
    sqlx::query_as(&q)
        .bind(&i.slug)
        .bind(i.name.trim())
        .bind(&i.layout)
        .bind(&i.product_ids)
        .bind(&i.group_ids)
        .bind(&steps)
        .bind(fields)
        .bind(redirect)
        .bind(i.active)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| match &e {
            sqlx::Error::Database(d) if d.is_unique_violation() => AppError::Conflict("error.slug_taken"),
            _ => db_err(e),
        })?
        .ok_or(AppError::NotFound)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM order_forms WHERE id = $1").bind(id).execute(pool).await.map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn set_group_form(pool: &PgPool, group: Uuid, form: Option<Uuid>) -> AppResult<()> {
    let n = sqlx::query("UPDATE product_groups SET order_form_id = $2 WHERE id = $1").bind(group).bind(form).execute(pool).await.map_err(db_err)?;
    if n.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Visible products of a form: explicit products plus every product of its groups.
pub async fn products(pool: &PgPool, f: &OrderForm) -> AppResult<Vec<Value>> {
    let rows: Vec<(Value,)> = sqlx::query_as(
        "SELECT json_build_object('id', p.id, 'name', p.name, 'slug', p.slug, 'description', p.description, 'group_id', p.group_id)
         FROM products p WHERE (p.id = ANY($1) OR p.group_id = ANY($2)) AND p.status = 'active' ORDER BY p.group_id, p.name",
    )
    .bind(&f.product_ids)
    .bind(&f.group_ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// Check client answers against the form's fields; returns the cleaned object.
pub fn check_answers(f: &OrderForm, answers: &Value) -> AppResult<Value> {
    let fields: Vec<Field> = serde_json::from_value(f.fields.clone()).unwrap_or_default();
    let mut out = serde_json::Map::new();
    for fd in fields {
        let v = answers.get(&fd.key);
        let text = match v {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Bool(b)) => if *b { "yes".into() } else { String::new() },
            Some(Value::Number(n)) => n.to_string(),
            _ => String::new(),
        };
        if text.is_empty() {
            if fd.required {
                return Err(AppError::Validation("error.order_form_required"));
            }
            continue;
        }
        let ok = text.chars().count() <= 2000
            && match fd.kind.as_str() {
                "select" => fd.options.contains(&text),
                "email" => text.contains('@') && !text.contains(' '),
                "number" => text.parse::<rust_decimal::Decimal>().is_ok(),
                _ => true,
            };
        if !ok {
            return Err(AppError::Validation("error.order_form_field"));
        }
        out.insert(fd.key, Value::String(text));
    }
    Ok(Value::Object(out))
}

/// Product ids a form may sell.
pub async fn allows(pool: &PgPool, f: &OrderForm, product_ids: &[Uuid]) -> AppResult<bool> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM products WHERE id = ANY($1) AND (id = ANY($2) OR group_id = ANY($3))")
        .bind(product_ids)
        .bind(&f.product_ids)
        .bind(&f.group_ids)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let distinct: std::collections::HashSet<_> = product_ids.iter().collect();
    Ok(n as usize == distinct.len())
}

pub async fn attach(pool: &PgPool, order: Uuid, form: Uuid, answers: &Value) -> AppResult<()> {
    sqlx::query("UPDATE orders SET order_form_id = $2, form_answers = $3 WHERE id = $1").bind(order).bind(form).bind(answers).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// Product free-domain rule (admin).
#[derive(Debug, Clone, Deserialize, Serialize, FromRow, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FreeDomain {
    pub free_domain: String,
    #[serde(default)]
    pub free_domain_tlds: Vec<String>,
    #[serde(default)]
    pub free_domain_cycles: Vec<String>,
}

pub async fn free_domain(pool: &PgPool, product: Uuid) -> AppResult<Option<FreeDomain>> {
    sqlx::query_as("SELECT free_domain, free_domain_tlds, free_domain_cycles FROM products WHERE id = $1").bind(product).fetch_optional(pool).await.map_err(db_err)
}

pub async fn set_free_domain(pool: &PgPool, product: Uuid, r: &FreeDomain) -> AppResult<FreeDomain> {
    if !["none", "once", "on_renewal"].contains(&r.free_domain.as_str())
        || r.free_domain_cycles.iter().any(|c| !pelty_billing::cycle::is_valid(c))
        || r.free_domain_tlds.len() > 100
    {
        return Err(AppError::Validation("error.validation"));
    }
    let tlds: Vec<String> = r.free_domain_tlds.iter().map(|t| t.trim().trim_start_matches('.').to_ascii_lowercase()).filter(|t| !t.is_empty()).collect();
    sqlx::query_as("UPDATE products SET free_domain = $2, free_domain_tlds = $3, free_domain_cycles = $4 WHERE id = $1 RETURNING free_domain, free_domain_tlds, free_domain_cycles")
        .bind(product)
        .bind(&r.free_domain)
        .bind(&tlds)
        .bind(&r.free_domain_cycles)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

/// Every product group with the order form it uses (None = default page).
pub async fn group_forms(pool: &PgPool) -> AppResult<Vec<(Uuid, String, Option<Uuid>)>> {
    sqlx::query_as("SELECT id, name, order_form_id FROM product_groups ORDER BY sort_order, name")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}
