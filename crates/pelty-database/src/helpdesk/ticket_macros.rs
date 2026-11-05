//! Ticket macros: a canned reply or note plus status, priority, assignee,
//! department and tag changes applied in one step.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, FromRow, utoipa::ToSchema)]
pub struct Macro {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub department_id: Option<Uuid>,
    pub reply_body: String,
    pub reply_internal: bool,
    pub set_status: Option<String>,
    pub set_priority: Option<String>,
    /// `keep` | `self` | `user` | `unassign`.
    pub assign_mode: String,
    pub assign_to: Option<Uuid>,
    pub set_department: Option<Uuid>,
    pub add_tags: Vec<String>,
    pub is_active: bool,
    pub sort_order: i32,
    pub uses: i64,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MacroInput {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub department_id: Option<Uuid>,
    #[serde(default)]
    pub reply_body: String,
    #[serde(default)]
    pub reply_internal: bool,
    #[serde(default)]
    pub set_status: Option<String>,
    #[serde(default)]
    pub set_priority: Option<String>,
    #[serde(default = "keep")]
    pub assign_mode: String,
    #[serde(default)]
    pub assign_to: Option<Uuid>,
    #[serde(default)]
    pub set_department: Option<Uuid>,
    #[serde(default)]
    pub add_tags: Vec<String>,
    #[serde(default = "yes")]
    pub is_active: bool,
    #[serde(default)]
    pub sort_order: i32,
}

fn keep() -> String {
    "keep".into()
}
fn yes() -> bool {
    true
}

pub const ASSIGN_MODES: [&str; 4] = ["keep", "self", "user", "unassign"];

impl MacroInput {
    /// Trims and validates; tags are normalised like ticket tags. Status
    /// existence and the assignee's permission are checked by the caller.
    pub fn normalize(mut self) -> Result<Self, &'static str> {
        self.name = self.name.trim().to_string();
        self.description = self.description.trim().to_string();
        if self.name.is_empty()
            || self.name.chars().count() > 100
            || self.description.chars().count() > 500
        {
            return Err("error.validation");
        }
        if self.reply_body.chars().count() > 20000 {
            return Err("error.invalid_message");
        }
        if self.reply_body.trim().is_empty() {
            self.reply_body.clear();
            self.reply_internal = false;
        }
        if self
            .set_priority
            .as_deref()
            .is_some_and(|p| !crate::ticket_sla::PRIORITIES.contains(&p))
        {
            return Err("error.validation");
        }
        if !ASSIGN_MODES.contains(&self.assign_mode.as_str()) {
            return Err("error.validation");
        }
        if self.assign_mode == "user" {
            if self.assign_to.is_none() {
                return Err("error.invalid_assignee");
            }
        } else {
            self.assign_to = None;
        }
        if self.add_tags.len() > 10 {
            return Err("error.invalid_tag");
        }
        let mut tags = Vec::new();
        for t in &self.add_tags {
            let t = crate::support::normalize_tag(t).ok_or("error.invalid_tag")?;
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        self.add_tags = tags;
        let does_something = !self.reply_body.is_empty()
            || self.set_status.is_some()
            || self.set_priority.is_some()
            || self.assign_mode != "keep"
            || self.set_department.is_some()
            || !self.add_tags.is_empty();
        if !does_something {
            return Err("error.macro_empty");
        }
        Ok(self)
    }
}

const COLS: &str = "id, name, description, department_id, reply_body, reply_internal, set_status, set_priority, assign_mode, assign_to, set_department, add_tags, is_active, sort_order, uses, updated_at";

/// All macros, or (with `department`) the active ones offered for that
/// department.
pub async fn list(pool: &PgPool, department: Option<Uuid>) -> AppResult<Vec<Macro>> {
    sqlx::query_as(&format!(
        "SELECT {COLS} FROM ticket_macros
         WHERE $1::uuid IS NULL OR (is_active AND (department_id IS NULL OR department_id = $1))
         ORDER BY sort_order, name"
    ))
    .bind(department)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get(pool: &PgPool, id: Uuid) -> AppResult<Macro> {
    sqlx::query_as(&format!("SELECT {COLS} FROM ticket_macros WHERE id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn create(pool: &PgPool, m: &MacroInput) -> AppResult<Macro> {
    sqlx::query_as(&format!(
        "INSERT INTO ticket_macros (name, description, department_id, reply_body, reply_internal, set_status, set_priority, assign_mode, assign_to, set_department, add_tags, is_active, sort_order)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13) RETURNING {COLS}"
    ))
    .bind(&m.name).bind(&m.description).bind(m.department_id).bind(&m.reply_body).bind(m.reply_internal).bind(&m.set_status)
    .bind(&m.set_priority).bind(&m.assign_mode).bind(m.assign_to).bind(m.set_department).bind(&m.add_tags).bind(m.is_active).bind(m.sort_order)
    .fetch_one(pool).await.map_err(db_err)
}

pub async fn update(pool: &PgPool, id: Uuid, m: &MacroInput) -> AppResult<Macro> {
    sqlx::query_as(&format!(
        "UPDATE ticket_macros SET name=$2, description=$3, department_id=$4, reply_body=$5, reply_internal=$6, set_status=$7, set_priority=$8,
            assign_mode=$9, assign_to=$10, set_department=$11, add_tags=$12, is_active=$13, sort_order=$14, updated_at=now()
         WHERE id = $1 RETURNING {COLS}"
    ))
    .bind(id).bind(&m.name).bind(&m.description).bind(m.department_id).bind(&m.reply_body).bind(m.reply_internal).bind(&m.set_status)
    .bind(&m.set_priority).bind(&m.assign_mode).bind(m.assign_to).bind(m.set_department).bind(&m.add_tags).bind(m.is_active).bind(m.sort_order)
    .fetch_optional(pool).await.map_err(db_err)?.ok_or(AppError::NotFound)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM ticket_macros WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn count_use(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE ticket_macros SET uses = uses + 1 WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn add_tags(pool: &PgPool, ticket: Uuid, tags: &[String]) -> AppResult<()> {
    if !tags.is_empty() {
        sqlx::query("INSERT INTO ticket_tags (ticket_id, tag) SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING")
            .bind(ticket).bind(tags).execute(pool).await.map_err(db_err)?;
    }
    Ok(())
}

/// Values a macro's reply can reference.
pub struct Vars<'a> {
    pub customer_name: &'a str,
    pub customer_email: &'a str,
    pub ticket_number: i64,
    pub ticket_subject: &'a str,
    pub department: &'a str,
    pub staff_name: &'a str,
}

/// Fill `{customer_name}`, `{customer_first_name}`, `{customer_email}`,
/// `{ticket_number}`, `{ticket_subject}`, `{department}` and `{staff_name}`.
/// Unknown placeholders are left as they are.
pub fn render(body: &str, v: &Vars) -> String {
    let name = if v.customer_name.trim().is_empty() {
        v.customer_email
    } else {
        v.customer_name.trim()
    };
    let first = name.split_whitespace().next().unwrap_or(name);
    body.replace("{customer_name}", name)
        .replace("{customer_first_name}", first)
        .replace("{customer_email}", v.customer_email)
        .replace("{ticket_number}", &v.ticket_number.to_string())
        .replace("{ticket_subject}", v.ticket_subject)
        .replace("{department}", v.department)
        .replace("{staff_name}", v.staff_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> MacroInput {
        serde_json::from_value(
            serde_json::json!({ "name": " Close as solved ", "set_status": "closed" }),
        )
        .unwrap()
    }

    #[test]
    fn normalize_rules() {
        let m = input().normalize().unwrap();
        assert_eq!(m.name, "Close as solved");
        assert_eq!(m.assign_mode, "keep");
        assert!(m.is_active);
        let empty: MacroInput =
            serde_json::from_value(serde_json::json!({ "name": "x", "reply_body": "   " }))
                .unwrap();
        assert_eq!(empty.normalize().unwrap_err(), "error.macro_empty");
        assert_eq!(
            MacroInput {
                set_priority: Some("asap".into()),
                ..input()
            }
            .normalize()
            .unwrap_err(),
            "error.validation"
        );
        assert_eq!(
            MacroInput {
                assign_mode: "user".into(),
                ..input()
            }
            .normalize()
            .unwrap_err(),
            "error.invalid_assignee"
        );
        assert_eq!(
            MacroInput {
                assign_mode: "boss".into(),
                ..input()
            }
            .normalize()
            .unwrap_err(),
            "error.validation"
        );
        assert_eq!(
            MacroInput {
                add_tags: vec!["!bad".into()],
                ..input()
            }
            .normalize()
            .unwrap_err(),
            "error.invalid_tag"
        );
        let m = MacroInput {
            add_tags: vec!["Needs Info".into(), "needs-info".into()],
            assign_mode: "self".into(),
            assign_to: Some(Uuid::nil()),
            ..input()
        }
        .normalize()
        .unwrap();
        assert_eq!(m.add_tags, vec!["needs-info"]);
        assert_eq!(m.assign_to, None, "only `user` keeps an assignee");
        assert_eq!(
            MacroInput {
                name: "  ".into(),
                ..input()
            }
            .normalize()
            .unwrap_err(),
            "error.validation"
        );
        let note = MacroInput {
            reply_body: " ".into(),
            reply_internal: true,
            ..input()
        }
        .normalize()
        .unwrap();
        assert!(!note.reply_internal);
    }

    #[test]
    fn render_placeholders() {
        let v = Vars {
            customer_name: "Ana Pop",
            customer_email: "ana@x.test",
            ticket_number: 42,
            ticket_subject: "Disk",
            department: "Billing",
            staff_name: "Ion",
        };
        assert_eq!(render("Hi {customer_first_name}, #{ticket_number} ({ticket_subject}) {department}. {staff_name} {unknown}", &v), "Hi Ana, #42 (Disk) Billing. Ion {unknown}");
        let v = Vars {
            customer_name: " ",
            ..v
        };
        assert_eq!(
            render("{customer_name}/{customer_first_name}", &v),
            "ana@x.test/ana@x.test"
        );
    }
}
