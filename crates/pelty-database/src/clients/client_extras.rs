//! Staff notes on clients, custom client fields (with per-client values) and
//! the admin to-do list.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{FromRow, PgPool};
use std::collections::HashMap;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

fn v() -> AppError {
    AppError::Validation("error.validation")
}

// notes
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct NoteInput {
    pub body: String,
    #[serde(default)]
    pub sticky: bool,
}

pub async fn notes(pool: &PgPool, customer: Uuid) -> AppResult<Value> {
    sqlx::query_scalar(
        "SELECT COALESCE(json_agg(json_build_object('id', n.id, 'body', n.body, 'sticky', n.sticky, 'created_at', n.created_at,
                'updated_at', n.updated_at, 'author_id', n.author_id, 'author_email', u.email)
                ORDER BY n.sticky DESC, n.created_at DESC), '[]'::json)::jsonb
         FROM customer_notes n LEFT JOIN users u ON u.id = n.author_id WHERE n.customer_id = $1",
    )
    .bind(customer)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

fn note_body(b: &NoteInput) -> AppResult<&str> {
    let t = b.body.trim();
    if t.is_empty() || t.chars().count() > 10_000 {
        return Err(v());
    }
    Ok(t)
}

pub async fn add_note(
    pool: &PgPool,
    customer: Uuid,
    author: Uuid,
    b: &NoteInput,
) -> AppResult<Uuid> {
    let body = note_body(b)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE id = $1)")
        .bind(customer)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !exists {
        return Err(AppError::NotFound);
    }
    sqlx::query_scalar("INSERT INTO customer_notes (customer_id, author_id, body, sticky) VALUES ($1,$2,$3,$4) RETURNING id")
        .bind(customer).bind(author).bind(body).bind(b.sticky)
        .fetch_one(pool).await.map_err(db_err)
}

pub async fn update_note(
    pool: &PgPool,
    customer: Uuid,
    note: Uuid,
    b: &NoteInput,
) -> AppResult<()> {
    let body = note_body(b)?;
    let r = sqlx::query("UPDATE customer_notes SET body=$3, sticky=$4, updated_at=now() WHERE id=$1 AND customer_id=$2")
        .bind(note).bind(customer).bind(body).bind(b.sticky)
        .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_note(pool: &PgPool, customer: Uuid, note: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM customer_notes WHERE id=$1 AND customer_id=$2")
        .bind(note)
        .bind(customer)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// fields
pub const FIELD_TYPES: [&str; 5] = ["text", "textarea", "dropdown", "checkbox", "link"];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ClientField {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub field_type: String,
    pub options: Vec<String>,
    pub required: bool,
    pub admin_only: bool,
    pub sort_order: i32,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct FieldInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub field_type: String,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub admin_only: bool,
    #[serde(default)]
    pub sort_order: i32,
}

fn check_field(f: &FieldInput) -> AppResult<Vec<String>> {
    let n = f.name.trim().chars().count();
    if n == 0
        || n > 120
        || f.description.chars().count() > 500
        || !FIELD_TYPES.contains(&f.field_type.as_str())
    {
        return Err(v());
    }
    let opts: Vec<String> = f
        .options
        .iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if f.field_type == "dropdown"
        && (opts.is_empty() || opts.len() > 100 || opts.iter().any(|o| o.chars().count() > 120))
    {
        return Err(v());
    }
    Ok(if f.field_type == "dropdown" {
        opts
    } else {
        Vec::new()
    })
}

/// Validate and normalise one value (pure; unit-tested). Empty = unset.
pub fn check_value(f: &ClientField, raw: &str) -> AppResult<String> {
    let val = raw.trim();
    if val.chars().count() > 2000 {
        return Err(v());
    }
    if val.is_empty() {
        return if f.required {
            Err(AppError::Validation("error.field_required"))
        } else {
            Ok(String::new())
        };
    }
    match f.field_type.as_str() {
        "dropdown" if !f.options.iter().any(|o| o == val) => Err(v()),
        "checkbox" if val != "1" && val != "0" => Err(v()),
        "checkbox" if f.required && val == "0" => Err(AppError::Validation("error.field_required")),
        "link"
            if !(val.starts_with("https://") || val.starts_with("http://"))
                || val.contains(char::is_whitespace) =>
        {
            Err(v())
        }
        "text" | "link" | "dropdown" if val.contains('\n') => Err(v()),
        _ => Ok(val.to_string()),
    }
}

const FCOLS: &str = "id, name, description, field_type, options, required, admin_only, sort_order";

pub async fn fields(pool: &PgPool, include_admin_only: bool) -> AppResult<Vec<ClientField>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {FCOLS} FROM client_fields WHERE ($1 OR NOT admin_only) ORDER BY sort_order, name"
    )))
    .bind(include_admin_only)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_field(pool: &PgPool, f: &FieldInput) -> AppResult<Uuid> {
    let opts = check_field(f)?;
    sqlx::query_scalar("INSERT INTO client_fields (name, description, field_type, options, required, admin_only, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
        .bind(f.name.trim()).bind(f.description.trim()).bind(&f.field_type).bind(&opts).bind(f.required).bind(f.admin_only).bind(f.sort_order)
        .fetch_one(pool).await.map_err(db_err)
}

pub async fn update_field(pool: &PgPool, id: Uuid, f: &FieldInput) -> AppResult<()> {
    let opts = check_field(f)?;
    let r = sqlx::query("UPDATE client_fields SET name=$2, description=$3, field_type=$4, options=$5, required=$6, admin_only=$7, sort_order=$8 WHERE id=$1")
        .bind(id).bind(f.name.trim()).bind(f.description.trim()).bind(&f.field_type).bind(&opts).bind(f.required).bind(f.admin_only).bind(f.sort_order)
        .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_field(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM client_fields WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn values(pool: &PgPool, customer: Uuid) -> AppResult<HashMap<Uuid, String>> {
    let rows: Vec<(Uuid, String)> =
        sqlx::query_as("SELECT field_id, value FROM customer_field_values WHERE customer_id=$1")
            .bind(customer)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

/// Save values for the fields visible to the caller (`staff` = all fields).
/// Fields not in `input` keep their value, but required visible fields must
/// end up non-empty. Unknown / hidden field ids are rejected.
pub async fn set_values(
    pool: &PgPool,
    customer: Uuid,
    staff: bool,
    input: &HashMap<Uuid, String>,
) -> AppResult<()> {
    let fs = fields(pool, staff).await?;
    if input.keys().any(|k| !fs.iter().any(|f| f.id == *k)) {
        return Err(v());
    }
    let current = values(pool, customer).await?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    for f in &fs {
        let raw = match input.get(&f.id) {
            Some(x) => x.as_str(),
            None => current.get(&f.id).map(String::as_str).unwrap_or(""),
        };
        let val = check_value(f, raw)?;
        if !input.contains_key(&f.id) {
            continue;
        }
        if val.is_empty() {
            sqlx::query("DELETE FROM customer_field_values WHERE customer_id=$1 AND field_id=$2")
                .bind(customer)
                .bind(f.id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        } else {
            sqlx::query(
                "INSERT INTO customer_field_values (customer_id, field_id, value) VALUES ($1,$2,$3)
                         ON CONFLICT (customer_id, field_id) DO UPDATE SET value = EXCLUDED.value",
            )
            .bind(customer)
            .bind(f.id)
            .bind(&val)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
    }
    tx.commit().await.map_err(db_err)
}

// todos
pub const TODO_STATUSES: [&str; 3] = ["pending", "in_progress", "done"];

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Todo {
    pub id: Uuid,
    pub title: String,
    pub description: String,
    pub status: String,
    pub due_date: Option<Date>,
    pub assigned_to: Option<Uuid>,
    pub assigned_email: Option<String>,
    pub created_by: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct TodoInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "pending")]
    pub status: String,
    /// YYYY-MM-DD
    #[serde(default)]
    #[schema(value_type = Option<String>)]
    pub due_date: Option<Date>,
    #[serde(default)]
    pub assigned_to: Option<Uuid>,
}
fn pending() -> String {
    "pending".into()
}

fn check_todo(t: &TodoInput) -> AppResult<()> {
    let n = t.title.trim().chars().count();
    if n == 0
        || n > 200
        || t.description.chars().count() > 5000
        || !TODO_STATUSES.contains(&t.status.as_str())
    {
        return Err(v());
    }
    Ok(())
}

async fn check_assignee(pool: &PgPool, id: Option<Uuid>) -> AppResult<()> {
    if let Some(id) = id {
        let staff: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_roles ur JOIN role_permissions rp ON rp.role_id = ur.role_id
             JOIN permissions p ON p.id = rp.permission_id WHERE ur.user_id = $1 AND p.name = 'admin.access')",
        )
        .bind(id).fetch_one(pool).await.map_err(db_err)?;
        if !staff {
            return Err(v());
        }
    }
    Ok(())
}

/// `status`: "open" (not done), a concrete status, or None for all.
pub async fn todos(pool: &PgPool, status: Option<&str>) -> AppResult<Vec<Todo>> {
    sqlx::query_as(
        "SELECT t.id, t.title, t.description, t.status, t.due_date, t.assigned_to, u.email::text AS assigned_email, t.created_by, t.created_at, t.updated_at
         FROM admin_todos t LEFT JOIN users u ON u.id = t.assigned_to
         WHERE ($1::text IS NULL OR ($1 = 'open' AND t.status <> 'done') OR t.status = $1)
         ORDER BY (t.status = 'done'), t.due_date NULLS LAST, t.created_at DESC LIMIT 500",
    )
    .bind(status).fetch_all(pool).await.map_err(db_err)
}

pub async fn create_todo(pool: &PgPool, by: Uuid, t: &TodoInput) -> AppResult<Uuid> {
    check_todo(t)?;
    check_assignee(pool, t.assigned_to).await?;
    sqlx::query_scalar("INSERT INTO admin_todos (title, description, status, due_date, assigned_to, created_by) VALUES ($1,$2,$3,$4,$5,$6) RETURNING id")
        .bind(t.title.trim()).bind(t.description.trim()).bind(&t.status).bind(t.due_date).bind(t.assigned_to).bind(by)
        .fetch_one(pool).await.map_err(db_err)
}

pub async fn update_todo(pool: &PgPool, id: Uuid, t: &TodoInput) -> AppResult<()> {
    check_todo(t)?;
    check_assignee(pool, t.assigned_to).await?;
    let r = sqlx::query("UPDATE admin_todos SET title=$2, description=$3, status=$4, due_date=$5, assigned_to=$6, updated_at=now() WHERE id=$1")
        .bind(id).bind(t.title.trim()).bind(t.description.trim()).bind(&t.status).bind(t.due_date).bind(t.assigned_to)
        .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_todo(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM admin_todos WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn f(t: &str, required: bool) -> ClientField {
        ClientField {
            id: Uuid::new_v4(),
            name: "F".into(),
            description: String::new(),
            field_type: t.into(),
            options: vec!["A".into(), "B".into()],
            required,
            admin_only: false,
            sort_order: 0,
        }
    }
    #[test]
    fn values_are_validated_per_type() {
        assert_eq!(check_value(&f("text", false), "  hi ").unwrap(), "hi");
        assert!(check_value(&f("text", false), "a\nb").is_err());
        assert_eq!(check_value(&f("textarea", false), "a\nb").unwrap(), "a\nb");
        assert!(check_value(&f("text", true), " ").is_err());
        assert_eq!(check_value(&f("text", false), "").unwrap(), "");
        assert!(check_value(&f("dropdown", false), "C").is_err());
        assert_eq!(check_value(&f("dropdown", false), "B").unwrap(), "B");
        assert!(check_value(&f("checkbox", false), "yes").is_err());
        assert!(check_value(&f("checkbox", true), "0").is_err());
        assert!(check_value(&f("link", false), "javascript:alert(1)").is_err());
        assert!(check_value(&f("link", false), "https://ok.example/x").is_ok());
        assert!(check_value(&f("text", false), &"x".repeat(2001)).is_err());
    }
    #[test]
    fn field_definitions() {
        let base = FieldInput {
            name: "VAT".into(),
            description: String::new(),
            field_type: "dropdown".into(),
            options: vec![" ".into()],
            required: false,
            admin_only: false,
            sort_order: 0,
        };
        assert!(check_field(&base).is_err());
        assert_eq!(
            check_field(&FieldInput {
                options: vec![" a ".into(), "".into()],
                ..base
            })
            .unwrap(),
            vec!["a".to_string()]
        );
    }
}
