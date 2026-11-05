//! Legal entities (selling companies) with per-year invoice numbering.
use crate::db_err;
use crate::invoices::Tx;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

const COLS: &str = "id, name, address, tax_id, bank_name, iban, swift, footer, terms, invoice_prefix, credit_prefix, number_format, due_days, is_default, created_at";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct LegalEntity {
    pub id: Uuid,
    pub name: String,
    pub address: String,
    pub tax_id: String,
    pub bank_name: String,
    pub iban: String,
    pub swift: String,
    pub footer: String,
    pub terms: String,
    pub invoice_prefix: String,
    pub credit_prefix: String,
    pub number_format: String,
    pub due_days: Option<i32>,
    pub is_default: bool,
    #[serde(with = "time::serde::rfc3339")]
    #[schema(value_type = String)]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, Default, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EntityInput {
    pub name: String,
    pub address: String,
    pub tax_id: String,
    pub bank_name: String,
    pub iban: String,
    pub swift: String,
    pub footer: String,
    pub terms: String,
    pub invoice_prefix: String,
    pub credit_prefix: String,
    /// Tokens: `{prefix}`, `{year}`, `{yy}`, `{seq}` or `{seq:N}` (zero-padded, N 1..=12).
    pub number_format: String,
    pub due_days: Option<i32>,
}

fn valid_prefix(p: &str) -> bool {
    (1..=12).contains(&p.len())
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'/')
}

/// Render a document number. `None` if the format is invalid.
pub fn format_number(fmt: &str, prefix: &str, year: i32, seq: i64) -> Option<String> {
    let mut out = String::new();
    let mut rest = fmt;
    let mut has_seq = false;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let j = rest[i..].find('}')? + i;
        let tok = &rest[i + 1..j];
        match tok {
            "prefix" => out.push_str(prefix),
            "year" => out.push_str(&format!("{year:04}")),
            "yy" => out.push_str(&format!("{:02}", year.rem_euclid(100))),
            "seq" => {
                has_seq = true;
                out.push_str(&seq.to_string());
            }
            _ => {
                let n: usize = tok
                    .strip_prefix("seq:")?
                    .parse()
                    .ok()
                    .filter(|n| (1..=12).contains(n))?;
                has_seq = true;
                out.push_str(&format!("{seq:0n$}"));
            }
        }
        rest = &rest[j + 1..];
    }
    if rest.contains('}') {
        return None;
    }
    out.push_str(rest);
    (has_seq && out.len() <= 64 && !out.chars().any(char::is_whitespace)).then_some(out)
}

impl EntityInput {
    pub fn normalized(mut self) -> Result<Self, &'static str> {
        for (v, max) in [
            (&mut self.name, 200),
            (&mut self.address, 1000),
            (&mut self.tax_id, 100),
            (&mut self.bank_name, 200),
            (&mut self.iban, 64),
            (&mut self.swift, 16),
            (&mut self.footer, 1000),
            (&mut self.terms, 2000),
            (&mut self.number_format, 64),
        ] {
            *v = v.trim().to_string();
            if v.chars().count() > max {
                return Err("error.field_too_long");
            }
        }
        if self.name.is_empty() {
            return Err("error.name_required");
        }
        self.iban = self.iban.replace(' ', "").to_ascii_uppercase();
        if !self.iban.is_empty()
            && !(self.iban.len() >= 15 && self.iban.bytes().all(|b| b.is_ascii_alphanumeric()))
        {
            return Err("error.invalid_iban");
        }
        self.swift = self.swift.to_ascii_uppercase();
        self.invoice_prefix = self.invoice_prefix.trim().to_string();
        self.credit_prefix = self.credit_prefix.trim().to_string();
        if !valid_prefix(&self.invoice_prefix)
            || !valid_prefix(&self.credit_prefix)
            || self
                .invoice_prefix
                .eq_ignore_ascii_case(&self.credit_prefix)
        {
            return Err("error.invalid_prefix");
        }
        if self.number_format.is_empty() {
            self.number_format = "{prefix}-{year}-{seq:5}".into();
        }
        if !self.number_format.contains("{prefix}")
            || format_number(&self.number_format, "P", 2026, 1).is_none()
        {
            return Err("error.invalid_number_format");
        }
        if self.due_days.is_some_and(|d| !(0..=365).contains(&d)) {
            return Err("error.invalid_due_days");
        }
        Ok(self)
    }
}

fn conflict(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.code().as_deref() == Some("23505") => {
            AppError::Conflict("error.prefix_taken")
        }
        _ => db_err(e),
    }
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<LegalEntity>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM legal_entities ORDER BY is_default DESC, name"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Option<LegalEntity>> {
    sqlx::query_as(&format!("SELECT {COLS} FROM legal_entities WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// Create. The default entity is chosen explicitly (no default = customers
/// without an entity keep the legacy global numbering).
pub async fn create(pool: &PgPool, e: &EntityInput) -> AppResult<LegalEntity> {
    sqlx::query_as(&format!(
        "INSERT INTO legal_entities (id, name, address, tax_id, bank_name, iban, swift, footer, terms, invoice_prefix, credit_prefix, number_format, due_days, is_default)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13, false) RETURNING {COLS}"
    ))
    .bind(Uuid::new_v4())
    .bind(&e.name).bind(&e.address).bind(&e.tax_id).bind(&e.bank_name).bind(&e.iban).bind(&e.swift)
    .bind(&e.footer).bind(&e.terms).bind(&e.invoice_prefix).bind(&e.credit_prefix).bind(&e.number_format).bind(e.due_days)
    .fetch_one(pool)
    .await
    .map_err(conflict)
}

pub async fn update(pool: &PgPool, id: Uuid, e: &EntityInput) -> AppResult<LegalEntity> {
    sqlx::query_as(&format!(
        "UPDATE legal_entities SET name=$2, address=$3, tax_id=$4, bank_name=$5, iban=$6, swift=$7, footer=$8, terms=$9,
            invoice_prefix=$10, credit_prefix=$11, number_format=$12, due_days=$13 WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id)
    .bind(&e.name).bind(&e.address).bind(&e.tax_id).bind(&e.bank_name).bind(&e.iban).bind(&e.swift)
    .bind(&e.footer).bind(&e.terms).bind(&e.invoice_prefix).bind(&e.credit_prefix).bind(&e.number_format).bind(e.due_days)
    .fetch_optional(pool)
    .await
    .map_err(conflict)?
    .ok_or(AppError::NotFound)
}

/// Remove the default (unassigned customers fall back to legacy numbering).
pub async fn clear_default(pool: &PgPool) -> AppResult<()> {
    sqlx::query("UPDATE legal_entities SET is_default = false WHERE is_default")
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn set_default(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    if get(pool, id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    sqlx::query("UPDATE legal_entities SET is_default = false WHERE is_default AND id <> $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE legal_entities SET is_default = true WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Delete an entity that never issued a document.
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let used: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM invoices WHERE legal_entity_id = $1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
    if used {
        return Err(AppError::Conflict("error.entity_in_use"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM legal_entity_sequences WHERE entity_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let n = sqlx::query("DELETE FROM legal_entities WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    tx.commit().await.map_err(db_err)
}

pub async fn assign_customer(
    pool: &PgPool,
    customer_id: Uuid,
    entity: Option<Uuid>,
) -> AppResult<()> {
    if let Some(e) = entity {
        if get(pool, e).await?.is_none() {
            return Err(AppError::Validation("error.entity_not_found"));
        }
    }
    let n = sqlx::query("UPDATE customers SET legal_entity_id = $2 WHERE id = $1")
        .bind(customer_id)
        .bind(entity)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn of_customer(pool: &PgPool, customer_id: Uuid) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT legal_entity_id FROM customers WHERE id = $1")
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
        .map(Option::flatten)
}

/// Entity for an invoice being finalized: the credited invoice's entity for
/// credit notes, else the customer's, else the default.
pub async fn resolve(tx: &mut Tx<'_>, invoice_id: Uuid) -> AppResult<Option<LegalEntity>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM legal_entities WHERE id = (
            SELECT COALESCE(i.legal_entity_id, o.legal_entity_id, c.legal_entity_id, (SELECT id FROM legal_entities WHERE is_default))
            FROM invoices i JOIN customers c ON c.id = i.customer_id LEFT JOIN invoices o ON o.id = i.credit_for
            WHERE i.id = $1)"
    ))
    .bind(invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)
}

/// Next gapless per-entity, per-year number (row-locked like the legacy sequence).
pub async fn next_number(
    tx: &mut Tx<'_>,
    e: &LegalEntity,
    kind: &str,
    year: i32,
) -> AppResult<String> {
    let seq: i64 = sqlx::query_scalar(
        "INSERT INTO legal_entity_sequences (entity_id, kind, year, next) VALUES ($1, $2, $3, 2)
         ON CONFLICT (entity_id, kind, year) DO UPDATE SET next = legal_entity_sequences.next + 1 RETURNING next - 1",
    )
    .bind(e.id)
    .bind(kind)
    .bind(year)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    let debit = format!("{}D", e.invoice_prefix);
    let prefix = match kind {
        "credit_note" => &e.credit_prefix,
        "debit_note" => &debit,
        _ => &e.invoice_prefix,
    };
    format_number(&e.number_format, prefix, year, seq)
        .ok_or(AppError::Validation("error.invalid_number_format"))
}

/// Preview of the next invoice number (does not consume it).
pub async fn preview(pool: &PgPool, e: &LegalEntity, year: i32) -> AppResult<String> {
    let next: Option<i64> = sqlx::query_scalar("SELECT next FROM legal_entity_sequences WHERE entity_id = $1 AND kind = 'invoice' AND year = $2")
        .bind(e.id)
        .bind(year)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok(
        format_number(&e.number_format, &e.invoice_prefix, year, next.unwrap_or(1))
            .unwrap_or_default(),
    )
}

/// Frozen seller snapshot of an issued document, if any.
pub async fn seller_snapshot(pool: &PgPool, invoice_id: Uuid) -> AppResult<Option<LegalEntity>> {
    let v: Option<serde_json::Value> =
        sqlx::query_scalar("SELECT seller_snapshot FROM invoices WHERE id = $1")
            .bind(invoice_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .flatten();
    Ok(v.and_then(|v| serde_json::from_value(v).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(
            format_number("{prefix}-{year}-{seq:5}", "RO", 2026, 7).as_deref(),
            Some("RO-2026-00007")
        );
        assert_eq!(
            format_number("{prefix}{yy}/{seq}", "F", 2026, 123).as_deref(),
            Some("F26/123")
        );
        assert_eq!(format_number("{prefix}-{year}", "F", 2026, 1), None);
        assert_eq!(format_number("{prefix}-{seq:0}", "F", 2026, 1), None);
        assert_eq!(format_number("{prefix}-{bad}-{seq}", "F", 2026, 1), None);
        assert_eq!(format_number("{prefix} {seq}", "F", 2026, 1), None);
        assert_eq!(format_number("{prefix}-{seq", "F", 2026, 1), None);
    }

    #[test]
    fn validates_input() {
        let ok = EntityInput {
            name: "Pelty SRL".into(),
            invoice_prefix: "PS".into(),
            credit_prefix: "PSC".into(),
            iban: "ro49 aaaa 1b31 0075 9384 0000".into(),
            ..Default::default()
        };
        let n = ok.clone().normalized().unwrap();
        assert_eq!(n.iban, "RO49AAAA1B31007593840000");
        assert_eq!(n.number_format, "{prefix}-{year}-{seq:5}");
        assert_eq!(
            EntityInput {
                credit_prefix: "ps".into(),
                ..ok.clone()
            }
            .normalized()
            .unwrap_err(),
            "error.invalid_prefix"
        );
        assert_eq!(
            EntityInput {
                number_format: "{year}-{seq}".into(),
                ..ok.clone()
            }
            .normalized()
            .unwrap_err(),
            "error.invalid_number_format"
        );
        assert_eq!(
            EntityInput {
                due_days: Some(400),
                ..ok.clone()
            }
            .normalized()
            .unwrap_err(),
            "error.invalid_due_days"
        );
        assert_eq!(
            EntityInput {
                iban: "x".into(),
                ..ok
            }
            .normalized()
            .unwrap_err(),
            "error.invalid_iban"
        );
    }
}
