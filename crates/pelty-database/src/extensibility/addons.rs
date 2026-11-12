//! Product addons and custom order fields.
//!
//! Addons are priced per currency + cycle (a `one_time` price applies to any
//! cycle). Custom fields are validated server-side when an order is placed;
//! `admin_only` fields are never shown to (or accepted from) customers.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool, Postgres, Transaction};
use std::collections::HashMap;
use uuid::Uuid;

pub const MAX_ADDONS_PER_ITEM: usize = 20;
pub const MAX_FIELD_LEN: usize = 2000;

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct Addon {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub status: String,
    pub sort_order: i32,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, utoipa::ToSchema)]
pub struct AddonPrice {
    pub currency: String,
    pub cycle: String,
    #[schema(value_type = String)]
    pub amount: Decimal,
    #[serde(default)]
    #[schema(value_type = String)]
    pub setup_fee: Decimal,
}

#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct AddonFull {
    #[serde(flatten)]
    pub addon: Addon,
    pub prices: Vec<AddonPrice>,
    pub product_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AddonInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "active")]
    pub status: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub prices: Vec<AddonPrice>,
    #[serde(default)]
    pub product_ids: Vec<Uuid>,
}

fn active() -> String {
    "active".into()
}

fn validate(i: &AddonInput) -> AppResult<()> {
    let n = i.name.trim();
    if n.is_empty() || n.chars().count() > 200 || i.description.chars().count() > 2000 {
        return Err(AppError::Validation("error.invalid_name"));
    }
    if !["active", "hidden", "retired"].contains(&i.status.as_str()) {
        return Err(AppError::Validation("error.invalid_status"));
    }
    if i.prices.len() > 100 || i.product_ids.len() > 500 {
        return Err(AppError::Validation("error.validation"));
    }
    for p in &i.prices {
        if !pelty_billing::cycle::is_valid(&p.cycle)
            || p.amount < Decimal::ZERO
            || p.setup_fee < Decimal::ZERO
        {
            return Err(AppError::Validation("error.invalid_price"));
        }
    }
    Ok(())
}

/// Create (id = None) or replace an addon with its prices and product links.
pub async fn upsert(pool: &PgPool, id: Option<Uuid>, i: &AddonInput) -> AppResult<AddonFull> {
    validate(i)?;
    let cur = crate::catalog::currencies(pool).await?;
    for p in &i.prices {
        crate::catalog::check_scale(
            &cur,
            &p.currency.to_ascii_uppercase(),
            &[p.amount, p.setup_fee],
        )?;
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = match id {
        Some(id) => {
            let n = sqlx::query(
                "UPDATE addons SET name=$2, description=$3, status=$4, sort_order=$5 WHERE id=$1",
            )
            .bind(id)
            .bind(i.name.trim())
            .bind(&i.description)
            .bind(&i.status)
            .bind(i.sort_order)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
            if n.rows_affected() == 0 {
                return Err(AppError::NotFound);
            }
            id
        }
        None => {
            let id = Uuid::new_v4();
            sqlx::query("INSERT INTO addons (id, name, description, status, sort_order) VALUES ($1,$2,$3,$4,$5)")
                .bind(id).bind(i.name.trim()).bind(&i.description).bind(&i.status).bind(i.sort_order)
                .execute(&mut *tx).await.map_err(db_err)?;
            id
        }
    };
    sqlx::query("DELETE FROM addon_prices WHERE addon_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for p in &i.prices {
        sqlx::query("INSERT INTO addon_prices (addon_id, currency, cycle, amount, setup_fee) VALUES ($1,$2,$3,$4,$5)")
            .bind(id).bind(p.currency.to_ascii_uppercase()).bind(&p.cycle).bind(p.amount).bind(p.setup_fee)
            .execute(&mut *tx).await.map_err(db_err)?;
    }
    sqlx::query("DELETE FROM addon_products WHERE addon_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO addon_products (addon_id, product_id) SELECT $1, unnest($2::uuid[]) ON CONFLICT DO NOTHING")
        .bind(id).bind(&i.product_ids)
        .execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    list(pool, None, false)
        .await?
        .into_iter()
        .find(|a| a.addon.id == id)
        .ok_or(AppError::NotFound)
}

/// All addons (admin) or the active addons offered for one product (public).
pub async fn list(pool: &PgPool, product: Option<Uuid>, public: bool) -> AppResult<Vec<AddonFull>> {
    let addons: Vec<Addon> = sqlx::query_as(
        "SELECT a.id, a.name, a.description, a.status, a.sort_order FROM addons a
         WHERE ($1::uuid IS NULL OR EXISTS (SELECT 1 FROM addon_products ap WHERE ap.addon_id = a.id AND ap.product_id = $1))
           AND (NOT $2 OR a.status = 'active')
         ORDER BY a.sort_order, a.name, a.id",
    )
    .bind(product).bind(public)
    .fetch_all(pool).await.map_err(db_err)?;
    let ids: Vec<Uuid> = addons.iter().map(|a| a.id).collect();
    let prices: Vec<(Uuid, String, String, Decimal, Decimal)> = sqlx::query_as(
        "SELECT addon_id, currency::text, cycle, amount, setup_fee FROM addon_prices WHERE addon_id = ANY($1) ORDER BY currency, cycle",
    )
    .bind(&ids).fetch_all(pool).await.map_err(db_err)?;
    let links: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT addon_id, product_id FROM addon_products WHERE addon_id = ANY($1) ORDER BY product_id")
        .bind(&ids).fetch_all(pool).await.map_err(db_err)?;
    Ok(addons
        .into_iter()
        .map(|a| AddonFull {
            prices: prices
                .iter()
                .filter(|p| p.0 == a.id)
                .map(|p| AddonPrice {
                    currency: p.1.clone(),
                    cycle: p.2.clone(),
                    amount: p.3,
                    setup_fee: p.4,
                })
                .collect(),
            product_ids: if public {
                vec![]
            } else {
                links.iter().filter(|l| l.0 == a.id).map(|l| l.1).collect()
            },
            addon: a,
        })
        .collect())
}

// custom fields

/// Field kinds of the custom-field kit. `password` values are sealed at
/// rest and never returned by the API (see [`crate::field_kit`]).
pub const FIELD_KINDS: &[&str] = &[
    "text", "textarea", "number", "select", "checkbox", "email", "password",
];

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct CustomField {
    pub id: Uuid,
    pub product_id: Uuid,
    pub name: String,
    pub description: String,
    pub kind: String,
    #[schema(value_type = Vec<String>)]
    pub choices: serde_json::Value,
    pub pattern: String,
    pub required: bool,
    pub admin_only: bool,
    pub sort_order: i32,
    /// Player identity field this stands for (`license`, `steamid`, ...):
    /// auto-filled from the buyer's game account.
    pub identity_key: Option<String>,
    /// Key of a field a provisioning module declared (managed by the module).
    pub module_key: Option<String>,
    /// Key staff gave the field: provisioning modules read the value as
    /// `custom_fields[key]`, delivery commands as `{key}`.
    pub field_key: Option<String>,
}

impl CustomField {
    /// The key modules and commands see (module key wins).
    pub fn key(&self) -> Option<&str> {
        self.module_key.as_deref().or(self.field_key.as_deref())
    }
}

/// Identity fields a checkout field may map to.
pub const IDENTITY_KEYS: &[&str] = &[
    "username",
    "uuid",
    "xuid",
    "steamid",
    "license",
    "discord_id",
    "fivem",
];

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: String,
    #[serde(default)]
    pub choices: Vec<String>,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub admin_only: bool,
    #[serde(default)]
    pub sort_order: i32,
    /// Auto-fill from this player identity field (see [`IDENTITY_KEYS`]).
    #[serde(default)]
    pub identity_key: Option<String>,
    /// Key for modules and delivery commands (`[a-z][a-z0-9_]{0,39}`).
    /// Derived from the identity field or the name when empty.
    #[serde(default)]
    pub key: Option<String>,
}

pub(crate) const FIELD_COLS: &str = "id, product_id, name, description, kind, choices, pattern, required, admin_only, sort_order, identity_key, module_key, field_key";

pub async fn fields(pool: &PgPool, product_id: Uuid, public: bool) -> AppResult<Vec<CustomField>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {FIELD_COLS} FROM product_custom_fields WHERE product_id = $1 AND (NOT $2 OR NOT admin_only) ORDER BY sort_order, name, id"
    )))
    .bind(product_id).bind(public)
    .fetch_all(pool).await.map_err(db_err)
}

/// Create (id = None) or update a field. The pattern is a PostgreSQL regex,
/// compiled once here so a broken pattern is rejected up front.
pub async fn upsert_field(
    pool: &PgPool,
    product_id: Uuid,
    id: Option<Uuid>,
    f: &FieldInput,
) -> AppResult<CustomField> {
    let n = f.name.trim();
    if n.is_empty() || n.chars().count() > 120 || f.description.chars().count() > 500 {
        return Err(AppError::Validation("error.invalid_name"));
    }
    if !FIELD_KINDS.contains(&f.kind.as_str()) {
        return Err(AppError::Validation("error.invalid_field_kind"));
    }
    let choices: Vec<String> = f
        .choices
        .iter()
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .collect();
    if (f.kind == "select") == choices.is_empty()
        || choices.len() > 100
        || choices.iter().any(|c| c.chars().count() > 120)
    {
        return Err(AppError::Validation("error.invalid_field_choices"));
    }
    if f.pattern.chars().count() > 200 {
        return Err(AppError::Validation("error.invalid_field_pattern"));
    }
    if !f.pattern.is_empty()
        && sqlx::query_scalar::<_, bool>("SELECT '' ~ $1")
            .bind(&f.pattern)
            .fetch_one(pool)
            .await
            .is_err()
    {
        return Err(AppError::Validation("error.invalid_field_pattern"));
    }
    let identity_key = f
        .identity_key
        .as_deref()
        .map(str::trim)
        .filter(|k| !k.is_empty());
    if identity_key.is_some_and(|k| {
        !IDENTITY_KEYS.contains(&k) || !matches!(f.kind.as_str(), "text" | "number")
    }) {
        return Err(AppError::Validation("error.invalid_field_identity"));
    }
    let module_key: Option<String> = match id {
        Some(fid) => sqlx::query_scalar(
            "SELECT module_key FROM product_custom_fields WHERE id = $1 AND product_id = $2",
        )
        .bind(fid)
        .bind(product_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?,
        None => None,
    };
    // A module-declared field keeps the module's key.
    let field_key = match module_key {
        Some(_) => None,
        None => Some(
            crate::field_kit::resolve_key(pool, product_id, id, f.key.as_deref(), identity_key, n)
                .await?,
        ),
    };
    let choices = serde_json::to_value(&choices).unwrap_or_default();
    // A new field with no explicit position goes after the existing ones.
    let row = match id {
        Some(_) => sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE product_custom_fields SET name=$3, description=$4, kind=$5, choices=$6, pattern=$7, required=$8, admin_only=$9, sort_order=$10, identity_key=$11, field_key=$12
             WHERE id=$1 AND product_id=$2 RETURNING {FIELD_COLS}"
        ))),
        None => sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO product_custom_fields (id, product_id, name, description, kind, choices, pattern, required, admin_only, sort_order, identity_key, field_key)
             SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,
                    CASE WHEN $10 = 0 THEN COALESCE((SELECT MAX(sort_order) + 1 FROM product_custom_fields WHERE product_id = $2), 0) ELSE $10 END,
                    $11,$12 WHERE EXISTS (SELECT 1 FROM products WHERE id = $2)
             RETURNING {FIELD_COLS}"
        ))),
    }
    .bind(id.unwrap_or_else(Uuid::new_v4)).bind(product_id).bind(n).bind(&f.description).bind(&f.kind).bind(choices)
    .bind(&f.pattern).bind(f.required).bind(f.admin_only).bind(f.sort_order).bind(identity_key).bind(field_key)
    .fetch_optional(pool).await.map_err(db_err)?;
    row.ok_or(AppError::NotFound)
}

pub async fn delete_field(pool: &PgPool, product_id: Uuid, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM product_custom_fields WHERE id = $1 AND product_id = $2")
        .bind(id)
        .bind(product_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() > 0)
}

// ordering

/// One priced addon on an order line.
#[derive(Debug, Clone)]
pub struct AddonQuote {
    pub addon_id: Uuid,
    pub name: String,
    pub cycle: String,
    pub amount: Decimal,
    pub setup_fee: Decimal,
}

/// Price the selected addons for a product line. Prefers the line's cycle,
/// falls back to a one-time price; an addon not linked to the product or with
/// no usable price is rejected.
pub async fn quote(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    cycle: &str,
    currency: &str,
    ids: &[Uuid],
) -> AppResult<Vec<AddonQuote>> {
    if ids.len() > MAX_ADDONS_PER_ITEM {
        return Err(AppError::Validation("error.invalid_addon"));
    }
    let mut uniq = ids.to_vec();
    uniq.sort();
    uniq.dedup();
    if uniq.len() != ids.len() {
        return Err(AppError::Validation("error.invalid_addon"));
    }
    let rows: Vec<(Uuid, String, String, Decimal, Decimal)> = sqlx::query_as(
        "SELECT DISTINCT ON (a.id) a.id, a.name, ap.cycle, ap.amount, ap.setup_fee
         FROM addons a
         JOIN addon_products l ON l.addon_id = a.id AND l.product_id = $1
         JOIN addon_prices ap ON ap.addon_id = a.id AND ap.currency = $3 AND ap.cycle IN ($2, 'one_time')
         WHERE a.id = ANY($4) AND a.status = 'active'
         ORDER BY a.id, (ap.cycle = $2) DESC",
    )
    .bind(product_id).bind(cycle).bind(currency).bind(&uniq)
    .fetch_all(&mut **tx).await.map_err(db_err)?;
    if rows.len() != uniq.len() {
        return Err(AppError::Validation("error.invalid_addon"));
    }
    let mut out: Vec<AddonQuote> = rows
        .into_iter()
        .map(|(addon_id, name, cycle, amount, setup_fee)| AddonQuote {
            addon_id,
            name,
            cycle,
            amount,
            setup_fee,
        })
        .collect();
    out.sort_by_key(|q| ids.iter().position(|i| *i == q.addon_id));
    Ok(out)
}

/// Validate customer-supplied custom field values against the product's
/// customer-visible fields. Returns the normalized `field_id -> value` map.
pub async fn check_fields(
    tx: &mut Transaction<'_, Postgres>,
    product_id: Uuid,
    values: &HashMap<Uuid, String>,
) -> AppResult<serde_json::Value> {
    let defs: Vec<CustomField> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {FIELD_COLS} FROM product_custom_fields WHERE product_id = $1 AND NOT admin_only"
    )))
    .bind(product_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if values.keys().any(|k| !defs.iter().any(|d| d.id == *k)) {
        return Err(AppError::Validation("error.invalid_field"));
    }
    let mut out = serde_json::Map::new();
    for d in &defs {
        // Passwords keep their spaces (only line breaks are refused).
        let raw = values.get(&d.id).map(String::as_str).unwrap_or("");
        let v = if d.kind == "password" {
            raw
        } else {
            raw.trim()
        };
        if v.chars().count() > MAX_FIELD_LEN || (d.kind != "textarea" && v.contains(['\n', '\r'])) {
            return Err(AppError::Validation("error.invalid_field"));
        }
        let empty = v.trim().is_empty() || (d.kind == "checkbox" && v == "false");
        if empty {
            if d.required {
                return Err(AppError::Validation("error.field_required"));
            }
            continue;
        }
        let ok = match d.kind.as_str() {
            "number" => v.parse::<Decimal>().is_ok(),
            "checkbox" => v == "true",
            "email" => crate::field_kit::email_ok(v),
            "select" => d
                .choices
                .as_array()
                .is_some_and(|c| c.iter().any(|x| x.as_str() == Some(v))),
            _ => true,
        };
        if !ok {
            return Err(AppError::Validation("error.invalid_field"));
        }
        if !d.pattern.is_empty() {
            let m: bool = sqlx::query_scalar("SELECT $1 ~ $2")
                .bind(v)
                .bind(&d.pattern)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            if !m {
                return Err(AppError::Validation("error.invalid_field"));
            }
        }
        let stored = if d.kind == "password" {
            crate::field_kit::seal(v)
        } else {
            v.to_string()
        };
        out.insert(d.id.to_string(), serde_json::Value::String(stored));
    }
    Ok(serde_json::Value::Object(out))
}

#[derive(Debug, Clone, FromRow, Serialize, utoipa::ToSchema)]
pub struct ServiceAddon {
    pub id: Uuid,
    pub addon_id: Uuid,
    pub name: String,
    pub status: String,
    pub cycle: String,
    #[schema(value_type = String)]
    pub recurring_amount: Decimal,
}

pub async fn for_service(pool: &PgPool, service_id: Uuid) -> AppResult<Vec<ServiceAddon>> {
    sqlx::query_as(
        "SELECT sa.id, sa.addon_id, a.name, sa.status, sa.cycle, sa.recurring_amount
         FROM service_addons sa JOIN addons a ON a.id = sa.addon_id WHERE sa.service_id = $1 ORDER BY a.sort_order, a.name",
    )
    .bind(service_id).fetch_all(pool).await.map_err(db_err)
}
