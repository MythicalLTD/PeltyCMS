//! Customers: billing profile owned by a user account (or staff-created).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

const COLS: &str = "id, owner_user_id, organization, first_name, last_name, email::text AS email, phone, address_line1, \
    address_line2, city, region, postal_code, country, tax_id, vat_number, tax_exempt, currency::text AS currency, status, notes, created_at, updated_at, \
    (SELECT u.avatar_version FROM users u WHERE u.id = customers.owner_user_id) AS owner_avatar_version";

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Customer {
    pub id: Uuid,
    pub owner_user_id: Option<Uuid>,
    pub organization: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: String,
    pub address_line1: String,
    pub address_line2: String,
    pub city: String,
    pub region: String,
    pub postal_code: String,
    pub country: String,
    pub tax_id: String,
    /// EU VAT number incl. country prefix (format-validated).
    pub vat_number: String,
    /// Staff-set: no tax rules apply.
    pub tax_exempt: bool,
    pub currency: String,
    pub status: String,
    /// Staff-only. Client endpoints must use [`Customer::for_client`].
    pub notes: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
    /// Avatar cache-buster of the linked login, if it has a profile picture.
    pub owner_avatar_version: Option<i64>,
}

impl Customer {
    /// Customer-facing view: drops staff notes.
    pub fn for_client(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap_or_default();
        if let Some(m) = v.as_object_mut() {
            m.remove("notes");
        }
        v
    }
}

/// Editable profile fields (customer and staff).
#[derive(Debug, Clone, Default, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub organization: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
    pub tax_id: Option<String>,
    pub vat_number: Option<String>,
    pub currency: Option<String>,
}

impl Profile {
    pub fn validate(&mut self) -> AppResult<()> {
        let lim = |v: &Option<String>, n: usize| v.as_ref().is_none_or(|s| s.chars().count() <= n);
        if !(lim(&self.organization, 200)
            && lim(&self.first_name, 100)
            && lim(&self.last_name, 100)
            && lim(&self.phone, 40)
            && lim(&self.address_line1, 200)
            && lim(&self.address_line2, 200)
            && lim(&self.city, 100)
            && lim(&self.region, 100)
            && lim(&self.postal_code, 20)
            && lim(&self.tax_id, 40))
        {
            return Err(AppError::Validation("error.field_too_long"));
        }
        if let Some(c) = self.country.as_mut() {
            *c = c.trim().to_ascii_uppercase();
            if !(c.is_empty() || (c.len() == 2 && c.bytes().all(|b| b.is_ascii_uppercase()))) {
                return Err(AppError::Validation("error.invalid_country"));
            }
        }
        if let Some(v) = self.vat_number.as_mut() {
            let country = self.country.as_deref().unwrap_or("");
            let raw: String = v
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_uppercase();
            *v = if raw.len() >= 2 && raw.chars().take(2).all(|c| c.is_ascii_uppercase()) {
                raw
            } else {
                pelty_billing::tax::normalize_vat(country, &raw)
            };
            if !v.is_empty() && !pelty_billing::tax::vat_format_ok(v) {
                return Err(AppError::Validation("error.invalid_vat_number"));
            }
        }
        if let Some(c) = self.currency.as_mut() {
            *c = c.trim().to_ascii_uppercase();
            if !(c.len() == 3 && c.bytes().all(|b| b.is_ascii_uppercase())) {
                return Err(AppError::Validation("error.invalid_currency"));
            }
        }
        Ok(())
    }
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<Customer>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM customers WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

pub async fn by_owner(pool: &PgPool, user_id: Uuid) -> AppResult<Option<Customer>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM customers WHERE owner_user_id = $1"
    ))
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Customer id for a user, creating the profile on first use. One round trip
/// in the common case (existing customer).
pub async fn id_for_user(pool: &PgPool, user_id: Uuid) -> AppResult<Uuid> {
    if let Some(id) = sqlx::query_scalar("SELECT id FROM customers WHERE owner_user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
    {
        return Ok(id);
    }
    sqlx::query_scalar(
        "INSERT INTO customers (id, owner_user_id, email, first_name)
         SELECT $1, u.id, u.email, left(u.display_name, 100) FROM users u WHERE u.id = $2
         ON CONFLICT (owner_user_id) DO UPDATE SET owner_user_id = EXCLUDED.owner_user_id
         RETURNING id",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Staff-created customer without a login.
pub async fn create(pool: &PgPool, email: &str, p: &Profile) -> AppResult<Customer> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO customers (id, email) VALUES ($1, $2)")
        .bind(id)
        .bind(email)
        .execute(pool)
        .await
        .map_err(db_err)?;
    update_profile(pool, id, p).await?.ok_or(AppError::NotFound)
}

pub async fn update_profile(pool: &PgPool, id: Uuid, p: &Profile) -> AppResult<Option<Customer>> {
    sqlx::query_as(&format!(
        "UPDATE customers SET
            organization = COALESCE($2, organization), first_name = COALESCE($3, first_name),
            last_name = COALESCE($4, last_name), phone = COALESCE($5, phone),
            address_line1 = COALESCE($6, address_line1), address_line2 = COALESCE($7, address_line2),
            city = COALESCE($8, city), region = COALESCE($9, region), postal_code = COALESCE($10, postal_code),
            country = COALESCE($11, country), tax_id = COALESCE($12, tax_id), currency = COALESCE($13, currency),
            vat_number = COALESCE($14, vat_number),
            updated_at = now()
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(&p.organization)
    .bind(&p.first_name)
    .bind(&p.last_name)
    .bind(&p.phone)
    .bind(&p.address_line1)
    .bind(&p.address_line2)
    .bind(&p.city)
    .bind(&p.region)
    .bind(&p.postal_code)
    .bind(&p.country)
    .bind(&p.tax_id)
    .bind(&p.currency)
    .bind(&p.vat_number)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Staff-only fields.
pub async fn update_admin(
    pool: &PgPool,
    id: Uuid,
    status: Option<&str>,
    notes: Option<&str>,
    tax_exempt: Option<bool>,
) -> AppResult<Option<Customer>> {
    if status.is_some_and(|s| !["active", "inactive", "closed"].contains(&s)) {
        return Err(AppError::Validation("error.invalid_status"));
    }
    if notes.is_some_and(|n| n.len() > 10_000) {
        return Err(AppError::Validation("error.field_too_long"));
    }
    sqlx::query_as(&format!(
        "UPDATE customers SET status = COALESCE($2, status), notes = COALESCE($3, notes), tax_exempt = COALESCE($4, tax_exempt), updated_at = now()
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(status)
    .bind(notes)
    .bind(tax_exempt)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Paginated list with optional search (email / name / organization prefix).
pub async fn list(
    pool: &PgPool,
    q: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<Customer>, i64)> {
    #[derive(FromRow)]
    struct Row {
        #[sqlx(flatten)]
        c: Customer,
        total: i64,
    }
    let pat = q.map(|s| format!("{}%", s.replace(['%', '_', '\\'], "")));
    let rows: Vec<Row> = sqlx::query_as(&format!(
        "SELECT {COLS}, count(*) OVER() AS total FROM customers
         WHERE ($1::text IS NULL OR email::text ILIKE $1 OR organization ILIKE $1 OR last_name ILIKE $1)
           -- Staff whose billing profile was only auto-created (no invoices, no services) are not customers.
           AND NOT (EXISTS (SELECT 1 FROM user_roles ur JOIN roles r ON r.id = ur.role_id
                            WHERE ur.user_id = customers.owner_user_id AND r.name <> 'customer')
                    AND NOT EXISTS (SELECT 1 FROM invoices i WHERE i.customer_id = customers.id)
                    AND NOT EXISTS (SELECT 1 FROM services sv WHERE sv.customer_id = customers.id))
         ORDER BY created_at DESC, id LIMIT $2 OFFSET $3"
    ))
    .bind(pat)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let total = rows.first().map(|r| r.total).unwrap_or(0);
    Ok((rows.into_iter().map(|r| r.c).collect(), total))
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Contact {
    pub id: Uuid,
    pub customer_id: Uuid,
    pub name: String,
    pub email: String,
    pub phone: String,
    pub is_billing: bool,
}

pub async fn contacts(pool: &PgPool, customer_id: Uuid) -> AppResult<Vec<Contact>> {
    sqlx::query_as(
        "SELECT id, customer_id, name, email::text AS email, phone, is_billing FROM customer_contacts
         WHERE customer_id = $1 ORDER BY created_at, id",
    )
    .bind(customer_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_contact(
    pool: &PgPool,
    customer_id: Uuid,
    name: &str,
    email: &str,
    phone: &str,
    is_billing: bool,
) -> AppResult<Contact> {
    if name.trim().is_empty() || name.len() > 200 || phone.len() > 40 {
        return Err(AppError::Validation("error.validation"));
    }
    if !email.contains('@') || email.len() > 254 {
        return Err(AppError::Validation("error.invalid_email"));
    }
    sqlx::query_as(
        "INSERT INTO customer_contacts (id, customer_id, name, email, phone, is_billing) VALUES ($1,$2,$3,$4,$5,$6)
         RETURNING id, customer_id, name, email::text AS email, phone, is_billing",
    )
    .bind(Uuid::new_v4())
    .bind(customer_id)
    .bind(name.trim())
    .bind(email.trim())
    .bind(phone)
    .bind(is_billing)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Delete a contact only if it belongs to `customer_id` (ownership in SQL).
pub async fn delete_contact(pool: &PgPool, customer_id: Uuid, id: Uuid) -> AppResult<bool> {
    let r = sqlx::query("DELETE FROM customer_contacts WHERE id = $1 AND customer_id = $2")
        .bind(id)
        .bind(customer_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

/// Update a contact owned by `customer_id`. Returns `None` if missing.
pub async fn update_contact(
    pool: &PgPool,
    customer_id: Uuid,
    id: Uuid,
    name: &str,
    email: &str,
    phone: &str,
    is_billing: bool,
) -> AppResult<Option<Contact>> {
    if name.trim().is_empty() || name.len() > 200 || phone.len() > 40 {
        return Err(AppError::Validation("error.validation"));
    }
    if !email.contains('@') || email.len() > 254 {
        return Err(AppError::Validation("error.invalid_email"));
    }
    sqlx::query_as(
        "UPDATE customer_contacts SET name = $3, email = $4, phone = $5, is_billing = $6
         WHERE id = $1 AND customer_id = $2
         RETURNING id, customer_id, name, email::text AS email, phone, is_billing",
    )
    .bind(id)
    .bind(customer_id)
    .bind(name.trim())
    .bind(email.trim())
    .bind(phone)
    .bind(is_billing)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}
