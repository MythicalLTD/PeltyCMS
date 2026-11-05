//! Invoice operations that WHMCS staff use daily: merge, split, duplicate,
//! notes. Finalized invoices are immutable, so merge/split create new
//! finalized invoices holding *copies* of the lines (`copied_from`) and void
//! the sources (`merged_into` / `split_from` keep the trail). Copies do not
//! re-claim a service period, so renewals stay idempotent.
use crate::db_err;
use crate::invoices::{self, Invoice, Tx};
use pelty_core::{AppError, AppResult};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

struct Src {
    id: Uuid,
    customer_id: Uuid,
    currency: String,
    status: String,
    amount_paid: rust_decimal::Decimal,
    due_at: Option<OffsetDateTime>,
}

async fn lock(tx: &mut Tx<'_>, ids: &[Uuid]) -> AppResult<Vec<Src>> {
    let rows: Vec<(
        Uuid,
        Uuid,
        String,
        String,
        rust_decimal::Decimal,
        Option<OffsetDateTime>,
    )> = sqlx::query_as(
        "SELECT id, customer_id, currency::text, status, amount_paid, due_at FROM invoices
         WHERE id = ANY($1) AND kind = 'invoice' ORDER BY created_at, id FOR UPDATE",
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if rows.len() != ids.len() {
        return Err(AppError::NotFound);
    }
    Ok(rows
        .into_iter()
        .map(
            |(id, customer_id, currency, status, amount_paid, due_at)| Src {
                id,
                customer_id,
                currency,
                status,
                amount_paid,
                due_at,
            },
        )
        .collect())
}

fn mergeable(s: &Src) -> bool {
    s.status == "unpaid" && s.amount_paid.is_zero()
}

/// Copy lines (all, or only `only`) of `from` into draft `to`.
async fn copy_lines(
    tx: &mut Tx<'_>,
    from: Uuid,
    to: Uuid,
    only: Option<&[Uuid]>,
) -> AppResult<u64> {
    let r = sqlx::query(
        "INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, tax_rate, amount, tax_amount,
            service_id, period_start, period_end, tax_auto, sort_order, copied_from)
         SELECT gen_random_uuid(), $2, description, quantity, unit_price, tax_rate, amount, tax_amount,
            service_id, period_start, period_end, FALSE,
            (SELECT COALESCE(max(sort_order), -1) FROM invoice_items WHERE invoice_id = $2) + row_number() OVER (ORDER BY sort_order, id),
            COALESCE(copied_from, id)
         FROM invoice_items WHERE invoice_id = $1 AND ($3::uuid[] IS NULL OR id = ANY($3))",
    )
    .bind(from)
    .bind(to)
    .bind(only)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected())
}

async fn finish(tx: &mut Tx<'_>, id: Uuid, due: Option<OffsetDateTime>) -> AppResult<Invoice> {
    invoices::recompute(tx, id).await?;
    let inv = invoices::finalize(tx, id, 0).await?;
    if let Some(d) = due {
        sqlx::query("UPDATE invoices SET due_at = $2 WHERE id = $1")
            .bind(id)
            .bind(d)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(Invoice {
        due_at: due.or(inv.due_at),
        ..inv
    })
}

/// Merge 2-50 unpaid, unpaid-to-date invoices of one customer and currency
/// into a new invoice (due = earliest due date). Sources are voided.
pub async fn merge(pool: &PgPool, ids: &[Uuid]) -> AppResult<Invoice> {
    let mut ids = ids.to_vec();
    ids.sort();
    ids.dedup();
    if ids.len() < 2 || ids.len() > 50 {
        return Err(AppError::Validation("error.merge_needs_two"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let src = lock(&mut tx, &ids).await?;
    if !src.iter().all(mergeable) {
        return Err(AppError::Conflict("error.invoice_not_mergeable"));
    }
    if src
        .iter()
        .any(|s| s.customer_id != src[0].customer_id || s.currency != src[0].currency)
    {
        return Err(AppError::Validation("error.merge_mixed"));
    }
    let new = invoices::create_draft(&mut tx, src[0].customer_id, &src[0].currency, None).await?;
    for s in &src {
        copy_lines(&mut tx, s.id, new, None).await?;
    }
    let due = src.iter().filter_map(|s| s.due_at).min();
    let inv = finish(&mut tx, new, due).await?;
    sqlx::query("UPDATE invoices SET status = 'void', merged_into = $2 WHERE id = ANY($1)")
        .bind(&ids)
        .bind(new)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(inv)
}

/// Split the given lines off an unpaid invoice: two new invoices (selected
/// lines, remaining lines), same due date; the original is voided.
pub async fn split(pool: &PgPool, id: Uuid, item_ids: &[Uuid]) -> AppResult<(Invoice, Invoice)> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let src = lock(&mut tx, &[id])
        .await?
        .pop()
        .ok_or(AppError::NotFound)?;
    if !mergeable(&src) {
        return Err(AppError::Conflict("error.invoice_not_mergeable"));
    }
    let all: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM invoice_items WHERE invoice_id = $1")
        .bind(id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    let mut pick: Vec<Uuid> = item_ids
        .iter()
        .copied()
        .filter(|i| all.contains(i))
        .collect();
    pick.sort();
    pick.dedup();
    if pick.is_empty() || pick.len() != item_ids.len() || pick.len() >= all.len() {
        return Err(AppError::Validation("error.split_selection"));
    }
    let rest: Vec<Uuid> = all.into_iter().filter(|i| !pick.contains(i)).collect();
    let mut out = Vec::new();
    for sel in [&pick, &rest] {
        let n = invoices::create_draft(&mut tx, src.customer_id, &src.currency, None).await?;
        copy_lines(&mut tx, id, n, Some(sel)).await?;
        sqlx::query("UPDATE invoices SET split_from = $2 WHERE id = $1")
            .bind(n)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        out.push(finish(&mut tx, n, src.due_at).await?);
    }
    sqlx::query("UPDATE invoices SET status = 'void' WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    let b = out.pop().ok_or(AppError::NotFound)?;
    let a = out.pop().ok_or(AppError::NotFound)?;
    Ok((a, b))
}

/// Duplicate any invoice into a new editable draft (lines copied, service
/// periods not re-claimed).
pub async fn duplicate(pool: &PgPool, id: Uuid) -> AppResult<Uuid> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let r: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT customer_id, currency::text, kind FROM invoices WHERE id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?;
    let (customer, currency, kind) = r.ok_or(AppError::NotFound)?;
    if kind != "invoice" {
        return Err(AppError::Conflict("error.invoice_not_duplicable"));
    }
    let n = invoices::create_draft(&mut tx, customer, &currency, None).await?;
    copy_lines(&mut tx, id, n, None).await?;
    invoices::recompute(&mut tx, n).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(n)
}

/// Staff-only and client-visible notes (allowed on finalized invoices).
pub async fn set_notes(pool: &PgPool, id: Uuid, admin: &str, client: &str) -> AppResult<()> {
    if admin.chars().count() > 10_000 || client.chars().count() > 5_000 {
        return Err(AppError::Validation("error.field_too_long"));
    }
    let r = sqlx::query("UPDATE invoices SET admin_notes = $2, client_notes = $3 WHERE id = $1")
        .bind(id)
        .bind(admin.trim())
        .bind(client.trim())
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn notes(
    pool: &PgPool,
    id: Uuid,
) -> AppResult<(String, String, Option<Uuid>, Option<Uuid>)> {
    sqlx::query_as(
        "SELECT admin_notes, client_notes, merged_into, split_from FROM invoices WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// A customer's invoices that can take part in a merge (unpaid, nothing paid).
pub async fn list_mergeable(
    pool: &PgPool,
    customer: Uuid,
) -> AppResult<
    Vec<(
        Uuid,
        Option<String>,
        String,
        rust_decimal::Decimal,
        Option<OffsetDateTime>,
    )>,
> {
    sqlx::query_as(
        "SELECT id, number, currency::text, total, due_at FROM invoices
         WHERE customer_id = $1 AND kind = 'invoice' AND status = 'unpaid' AND amount_paid = 0 ORDER BY due_at NULLS LAST, id LIMIT 200",
    )
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}
