//! CSV import of clients, services and invoices with column mapping, preview
//! and a real dry-run (the whole import runs in a transaction that is rolled
//! back). A live run commits only when every row is valid.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use std::str::FromStr;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

pub const MAX_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_ROWS: usize = 20_000;
const MAX_ERRORS: usize = 200;

/// Import kinds and their target fields (`*` = required).
pub fn fields(kind: &str) -> Option<&'static [(&'static str, bool)]> {
    Some(match kind {
        "clients" => &[
            ("email", true),
            ("first_name", false),
            ("last_name", false),
            ("organization", false),
            ("phone", false),
            ("address_line1", false),
            ("city", false),
            ("region", false),
            ("postal_code", false),
            ("country", false),
            ("currency", false),
        ],
        "services" => &[
            ("email", true),
            ("product", true),
            ("cycle", true),
            ("amount", true),
            ("status", false),
            ("next_due_date", false),
            ("currency", false),
        ],
        "invoices" => &[
            ("email", true),
            ("number", true),
            ("amount", true),
            ("description", false),
            ("status", false),
            ("issued_at", false),
            ("due_at", false),
            ("currency", false),
        ],
        _ => return None,
    })
}

/// RFC 4180 parser: quoted fields, doubled quotes, CRLF/LF, optional BOM.
pub fn parse(src: &str) -> AppResult<Vec<Vec<String>>> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let delim = detect_delim(src);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    cell.push('"');
                }
                '"' => quoted = false,
                _ => cell.push(c),
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            c if c == delim => row.push(std::mem::take(&mut cell)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                row.push(std::mem::take(&mut cell));
                if !(row.len() == 1 && row[0].trim().is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
                if rows.len() > MAX_ROWS + 1 {
                    return Err(AppError::Validation("error.import_too_large"));
                }
            }
            _ => cell.push(c),
        }
    }
    if quoted {
        return Err(AppError::Validation("error.import_malformed"));
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    if rows.len() > MAX_ROWS + 1 {
        return Err(AppError::Validation("error.import_too_large"));
    }
    Ok(rows)
}

/// `;` when the header line has more semicolons than commas (Excel EU).
fn detect_delim(src: &str) -> char {
    let first = src.lines().next().unwrap_or("");
    if first.matches(';').count() > first.matches(',').count() {
        ';'
    } else {
        ','
    }
}

fn norm(s: &str) -> String {
    s.trim().to_ascii_lowercase().replace([' ', '-', '.'], "_")
}

/// Guess field → header by normalized name and a few common aliases.
pub fn suggest(kind: &str, headers: &[String]) -> HashMap<String, String> {
    let alias = |f: &str| -> &'static [&'static str] {
        match f {
            "email" => &["email", "e_mail", "email_address", "client_email"],
            "first_name" => &["first_name", "firstname", "first"],
            "last_name" => &["last_name", "lastname", "last", "surname"],
            "organization" => &["organization", "company", "companyname", "company_name"],
            "phone" => &["phone", "phonenumber", "phone_number", "telephone"],
            "address_line1" => &["address_line1", "address", "address1", "street"],
            "postal_code" => &["postal_code", "postcode", "zip", "zip_code"],
            "region" => &["region", "state", "county"],
            "product" => &["product", "product_name", "package", "plan"],
            "cycle" => &["cycle", "billing_cycle", "billingcycle"],
            "amount" => &["amount", "price", "total", "recurring_amount"],
            "next_due_date" => &["next_due_date", "nextduedate", "due_date"],
            "number" => &["number", "invoice_number", "invoicenum", "invoice"],
            "issued_at" => &["issued_at", "date", "invoice_date"],
            "due_at" => &["due_at", "duedate", "due_date"],
            _ => &[],
        }
    };
    let mut out = HashMap::new();
    for (f, _) in fields(kind).unwrap_or(&[]) {
        let names: Vec<String> = std::iter::once(f.to_string())
            .chain(alias(f).iter().map(|s| s.to_string()))
            .collect();
        if let Some(h) = headers.iter().find(|h| names.contains(&norm(h))) {
            out.insert(f.to_string(), h.clone());
        }
    }
    out
}

#[derive(Debug, Serialize)]
pub struct RowError {
    /// 1-based data row (header excluded).
    pub row: usize,
    pub field: Option<String>,
    pub code: &'static str,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub total: usize,
    pub created: usize,
    pub skipped: usize,
    pub errors: Vec<RowError>,
    pub error_count: usize,
    pub committed: bool,
}

struct Row<'a> {
    cells: &'a [String],
    idx: &'a HashMap<&'a str, usize>,
}

impl Row<'_> {
    fn get(&self, f: &str) -> &str {
        self.idx
            .get(f)
            .and_then(|&i| self.cells.get(i))
            .map(|s| s.trim())
            .unwrap_or("")
    }
    fn opt(&self, f: &str) -> Option<&str> {
        Some(self.get(f)).filter(|s| !s.is_empty())
    }
}

type Fail = (Option<&'static str>, &'static str);
fn bad(f: &'static str, code: &'static str) -> Fail {
    (Some(f), code)
}

fn date(v: Option<&str>, f: &'static str) -> Result<Option<Date>, Fail> {
    let fmt = time::macros::format_description!("[year]-[month]-[day]");
    v.map(|s| Date::parse(s, &fmt).map_err(|_| bad(f, "error.import_bad_date")))
        .transpose()
}

fn money(v: &str, f: &'static str) -> Result<Decimal, Fail> {
    let d =
        Decimal::from_str(&v.replace(' ', "")).map_err(|_| bad(f, "error.import_bad_amount"))?;
    if d < Decimal::ZERO || d > Decimal::from(1_000_000_000) {
        return Err(bad(f, "error.import_bad_amount"));
    }
    Ok(d.round_dp(4))
}

fn valid_email(e: &str) -> bool {
    e.len() <= 254
        && e.split_once('@').is_some_and(|(a, d)| {
            !a.is_empty() && d.contains('.') && !e.contains(char::is_whitespace)
        })
}

async fn currency(
    tx: &mut Transaction<'_, Postgres>,
    v: Option<&str>,
    fallback: &str,
) -> Result<String, Fail> {
    let c = v.unwrap_or(fallback).to_ascii_uppercase();
    let ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM currencies WHERE code = $1)")
        .bind(&c)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| bad("currency", "error.import_bad_currency"))?;
    if ok {
        Ok(c)
    } else {
        Err(bad("currency", "error.import_bad_currency"))
    }
}

async fn customer(tx: &mut Transaction<'_, Postgres>, email: &str) -> Result<(Uuid, String), Fail> {
    let r: Option<(Uuid, String)> =
        sqlx::query_as("SELECT id, currency::text FROM customers WHERE email = $1::citext")
            .bind(email)
            .fetch_optional(&mut **tx)
            .await
            .map_err(|_| bad("email", "error.import_db"))?;
    r.ok_or(bad("email", "error.import_unknown_client"))
}

enum Outcome {
    Created,
    Skipped,
}

async fn client_row(
    tx: &mut Transaction<'_, Postgres>,
    r: &Row<'_>,
    default_cur: &str,
) -> Result<Outcome, Fail> {
    let email = r.get("email").to_ascii_lowercase();
    if !valid_email(&email) {
        return Err(bad("email", "error.import_bad_email"));
    }
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE email = $1::citext)")
            .bind(&email)
            .fetch_one(&mut **tx)
            .await
            .map_err(|_| (None, "error.import_db"))?;
    if exists {
        return Ok(Outcome::Skipped);
    }
    let country = r.get("country").to_ascii_uppercase();
    if !(country.is_empty()
        || (country.len() == 2 && country.bytes().all(|b| b.is_ascii_uppercase())))
    {
        return Err(bad("country", "error.invalid_country"));
    }
    let cur = currency(tx, r.opt("currency"), default_cur).await?;
    let clip = |f: &str, n: usize| r.get(f).chars().take(n).collect::<String>();
    sqlx::query(
        "INSERT INTO customers (id, email, first_name, last_name, organization, phone, address_line1, city, region, postal_code, country, currency)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(Uuid::new_v4()).bind(&email).bind(clip("first_name", 100)).bind(clip("last_name", 100)).bind(clip("organization", 200))
    .bind(clip("phone", 40)).bind(clip("address_line1", 200)).bind(clip("city", 100)).bind(clip("region", 100)).bind(clip("postal_code", 20))
    .bind(&country).bind(&cur)
    .execute(&mut **tx)
    .await
    .map_err(|_| (None, "error.import_db"))?;
    Ok(Outcome::Created)
}

async fn service_row(tx: &mut Transaction<'_, Postgres>, r: &Row<'_>) -> Result<Outcome, Fail> {
    let (cid, ccur) = customer(tx, r.get("email")).await?;
    let product: Option<Uuid> = sqlx::query_scalar("SELECT id FROM products WHERE slug = $1 OR lower(name) = lower($1) ORDER BY (slug = $1) DESC LIMIT 1")
        .bind(r.get("product"))
        .fetch_optional(&mut **tx)
        .await
        .map_err(|_| (None, "error.import_db"))?;
    let product = product.ok_or(bad("product", "error.import_unknown_product"))?;
    let cycle = r.get("cycle").to_ascii_lowercase().replace(['-', ' '], "_");
    if !pelty_billing::cycle::is_valid(&cycle) {
        return Err(bad("cycle", "error.invalid_cycle"));
    }
    let amount = money(r.get("amount"), "amount")?;
    let status = r.opt("status").unwrap_or("active").to_ascii_lowercase();
    if !["pending", "active", "suspended", "terminated", "cancelled"].contains(&status.as_str()) {
        return Err(bad("status", "error.import_bad_status"));
    }
    let due = date(r.opt("next_due_date"), "next_due_date")?;
    let cur = currency(tx, r.opt("currency"), &ccur).await?;
    sqlx::query(
        "INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date, activated_at, terminated_at)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8, CASE WHEN $4 <> 'pending' THEN now() END, CASE WHEN $4 IN ('terminated','cancelled') THEN now() END)",
    )
    .bind(Uuid::new_v4()).bind(cid).bind(product).bind(&status).bind(&cycle).bind(&cur).bind(amount).bind(due)
    .execute(&mut **tx)
    .await
    .map_err(|_| (None, "error.import_db"))?;
    Ok(Outcome::Created)
}

async fn invoice_row(tx: &mut Transaction<'_, Postgres>, r: &Row<'_>) -> Result<Outcome, Fail> {
    let (cid, ccur) = customer(tx, r.get("email")).await?;
    let number = r.get("number");
    if number.chars().count() > 60 {
        return Err(bad("number", "error.field_too_long"));
    }
    let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM invoices WHERE number = $1)")
        .bind(number)
        .fetch_one(&mut **tx)
        .await
        .map_err(|_| (None, "error.import_db"))?;
    if taken {
        return Ok(Outcome::Skipped);
    }
    let amount = money(r.get("amount"), "amount")?;
    let status = r.opt("status").unwrap_or("unpaid").to_ascii_lowercase();
    if !["unpaid", "paid", "void", "refunded"].contains(&status.as_str()) {
        return Err(bad("status", "error.import_bad_status"));
    }
    let to_ts = |d: Option<Date>| d.map(|d| d.midnight().assume_utc());
    let issued =
        to_ts(date(r.opt("issued_at"), "issued_at")?).unwrap_or_else(OffsetDateTime::now_utc);
    let due = to_ts(date(r.opt("due_at"), "due_at")?).unwrap_or(issued);
    let cur = currency(tx, r.opt("currency"), &ccur).await?;
    let desc: String = r
        .opt("description")
        .unwrap_or("Imported invoice")
        .chars()
        .take(500)
        .collect();
    let paid = if status == "paid" {
        amount
    } else {
        Decimal::ZERO
    };
    let id = Uuid::new_v4();
    // Insert as draft so the item guard allows the line, then finalize.
    sqlx::query("INSERT INTO invoices (id, customer_id, currency, status, subtotal, total, amount_paid, issued_at, due_at, paid_at, created_at) VALUES ($1,$2,$3,'draft',$4,$4,$5,$6,$7,$8,$6)")
        .bind(id).bind(cid).bind(&cur).bind(amount).bind(paid).bind(issued).bind(due).bind((status == "paid").then_some(issued))
        .execute(&mut **tx).await.map_err(|_| (None, "error.import_db"))?;
    sqlx::query("INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, amount) VALUES ($1,$2,$3,1,$4,$4)")
        .bind(Uuid::new_v4()).bind(id).bind(&desc).bind(amount)
        .execute(&mut **tx).await.map_err(|_| (None, "error.import_db"))?;
    sqlx::query(
        "UPDATE invoices SET number = $2, status = $3, finalized_at = issued_at WHERE id = $1",
    )
    .bind(id)
    .bind(number)
    .bind(&status)
    .execute(&mut **tx)
    .await
    .map_err(|_| (None, "error.import_db"))?;
    Ok(Outcome::Created)
}

/// Validate (and unless `dry_run`, apply) `rows[1..]` using `mapping`
/// (field → header). Each row runs in a savepoint so one bad row can't poison
/// the rest; the outer transaction is committed only for a clean live run.
pub async fn run(
    pool: &PgPool,
    kind: &str,
    rows: &[Vec<String>],
    mapping: &HashMap<String, String>,
    dry_run: bool,
) -> AppResult<Report> {
    let spec = fields(kind).ok_or(AppError::Validation("error.import_kind"))?;
    let headers = rows
        .first()
        .ok_or(AppError::Validation("error.import_empty"))?;
    let mut idx: HashMap<&str, usize> = HashMap::new();
    for (f, _) in spec {
        if let Some(h) = mapping.get(*f).filter(|h| !h.is_empty()) {
            let i = headers
                .iter()
                .position(|x| x == h)
                .ok_or(AppError::Validation("error.import_mapping"))?;
            idx.insert(f, i);
        }
    }
    if mapping.keys().any(|k| !spec.iter().any(|(f, _)| f == k))
        || spec.iter().any(|(f, req)| *req && !idx.contains_key(f))
    {
        return Err(AppError::Validation("error.import_mapping"));
    }
    let default_cur: String = sqlx::query_scalar(
        "SELECT code::text FROM currencies ORDER BY is_default DESC, code LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .unwrap_or_else(|| "EUR".into());
    let mut tx = pool.begin().await.map_err(db_err)?;
    let mut rep = Report {
        total: rows.len() - 1,
        created: 0,
        skipped: 0,
        errors: Vec::new(),
        error_count: 0,
        committed: false,
    };
    for (n, cells) in rows.iter().enumerate().skip(1) {
        let r = Row { cells, idx: &idx };
        let missing = spec.iter().find(|(f, req)| *req && r.get(f).is_empty());
        sqlx::query("SAVEPOINT row")
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        let res = match missing {
            Some((f, _)) => Err((Some(*f), "error.field_required")),
            None => match kind {
                "clients" => client_row(&mut tx, &r, &default_cur).await,
                "services" => service_row(&mut tx, &r).await,
                _ => invoice_row(&mut tx, &r).await,
            },
        };
        match res {
            Ok(Outcome::Created) => rep.created += 1,
            Ok(Outcome::Skipped) => rep.skipped += 1,
            Err((field, code)) => {
                sqlx::query("ROLLBACK TO SAVEPOINT row")
                    .execute(&mut *tx)
                    .await
                    .map_err(db_err)?;
                rep.error_count += 1;
                if rep.errors.len() < MAX_ERRORS {
                    rep.errors.push(RowError {
                        row: n,
                        field: field.map(str::to_string),
                        code,
                    });
                }
            }
        }
    }
    if dry_run || rep.error_count > 0 {
        tx.rollback().await.map_err(db_err)?;
    } else {
        tx.commit().await.map_err(db_err)?;
        rep.committed = true;
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quotes_crlf_and_semicolons() {
        let r = parse("\u{feff}a,b\r\n\"x, \"\"y\"\"\",2\n\n3,\"multi\nline\"").unwrap();
        assert_eq!(
            r,
            vec![
                vec!["a", "b"],
                vec!["x, \"y\"", "2"],
                vec!["3", "multi\nline"]
            ]
        );
        assert_eq!(parse("a;b\n1;2").unwrap()[1], vec!["1", "2"]);
        assert!(parse("a,\"open").is_err());
    }

    #[test]
    fn suggests_aliases() {
        let h: Vec<String> = ["E-mail", "Company Name", "Zip"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let m = suggest("clients", &h);
        assert_eq!(m["email"], "E-mail");
        assert_eq!(m["organization"], "Company Name");
        assert_eq!(m["postal_code"], "Zip");
    }
}
