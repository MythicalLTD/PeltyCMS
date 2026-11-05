//! Support desk extras on top of tickets: canned replies, tags, client
//! ratings, merge/split, escalation rules, SLA metrics and e-mail piping log.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct CannedReply {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    pub department_id: Option<Uuid>,
    pub category: String,
}

pub async fn canned(pool: &PgPool, department: Option<Uuid>) -> AppResult<Vec<CannedReply>> {
    sqlx::query_as(
        "SELECT id, title, body, department_id, category FROM ticket_canned_replies
         WHERE ($1::uuid IS NULL OR department_id IS NULL OR department_id = $1) ORDER BY category, title",
    )
    .bind(department)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn upsert_canned(
    pool: &PgPool,
    id: Option<Uuid>,
    title: &str,
    body: &str,
    department: Option<Uuid>,
    category: &str,
) -> AppResult<CannedReply> {
    let q = match id {
        Some(_) => "UPDATE ticket_canned_replies SET title=$2, body=$3, department_id=$4, category=$5 WHERE id=$1 RETURNING id, title, body, department_id, category",
        None => "INSERT INTO ticket_canned_replies (id, title, body, department_id, category) VALUES (COALESCE($1, gen_random_uuid()),$2,$3,$4,$5) RETURNING id, title, body, department_id, category",
    };
    sqlx::query_as(q)
        .bind(id)
        .bind(title)
        .bind(body)
        .bind(department)
        .bind(category.trim())
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_canned(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM ticket_canned_replies WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Normalise a tag: lowercase, spaces to dashes; `None` if invalid.
pub fn normalize_tag(t: &str) -> Option<String> {
    let t = t.trim().to_lowercase().replace(' ', "-");
    let mut ch = t.chars();
    let first_ok = ch.next().is_some_and(|c| c.is_ascii_alphanumeric());
    let rest_ok = ch.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    (first_ok && rest_ok && t.len() <= 40).then_some(t)
}

pub async fn tags(pool: &PgPool, ticket: Uuid) -> AppResult<Vec<String>> {
    sqlx::query_scalar("SELECT tag FROM ticket_tags WHERE ticket_id = $1 ORDER BY tag")
        .bind(ticket)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Replace a ticket's tag set.
pub async fn set_tags(pool: &PgPool, ticket: Uuid, tags: &[String]) -> AppResult<()> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM ticket_tags WHERE ticket_id = $1")
        .bind(ticket)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO ticket_tags (ticket_id, tag) SELECT $1, unnest($2::text[]) ON CONFLICT DO NOTHING")
        .bind(ticket)
        .bind(tags)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Client rating, only on the customer's own closed or answered ticket.
pub async fn rate(
    pool: &PgPool,
    ticket: Uuid,
    customer: Uuid,
    rating: i16,
    comment: &str,
) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE tickets SET rating = $3, rating_comment = $4, rated_at = now()
         WHERE id = $1 AND customer_id = $2 AND first_response_at IS NOT NULL AND merged_into IS NULL",
    )
    .bind(ticket)
    .bind(customer)
    .bind(rating)
    .bind(comment)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::Validation("error.ticket_not_rateable"));
    }
    Ok(())
}

/// Move every message (and attachment) of `source` into `target`, close the
/// source and point it at the target. Both must belong to the same customer.
pub async fn merge(pool: &PgPool, source: Uuid, target: Uuid) -> AppResult<()> {
    if source == target {
        return Err(AppError::Validation("error.ticket_merge_self"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let same: Option<bool> = sqlx::query_scalar(
        "SELECT COALESCE(a.customer_id = b.customer_id, a.customer_id IS NULL AND b.customer_id IS NULL AND lower(a.guest_email) = lower(b.guest_email)) FROM tickets a, tickets b
         WHERE a.id = $1 AND b.id = $2 AND a.merged_into IS NULL AND b.merged_into IS NULL FOR UPDATE",
    )
    .bind(source)
    .bind(target)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    match same {
        None => return Err(AppError::NotFound),
        Some(false) => return Err(AppError::Validation("error.ticket_merge_customer")),
        Some(true) => {}
    }
    for q in [
        "UPDATE ticket_messages SET ticket_id = $2 WHERE ticket_id = $1",
        "UPDATE ticket_attachments SET ticket_id = $2 WHERE ticket_id = $1",
        "INSERT INTO ticket_tags (ticket_id, tag) SELECT $2, tag FROM ticket_tags WHERE ticket_id = $1 ON CONFLICT DO NOTHING",
        "UPDATE tickets SET status = 'closed', closed_at = now(), merged_into = $2 WHERE id = $1",
        "UPDATE tickets t SET last_reply_at = GREATEST(t.last_reply_at, s.last_reply_at),
            first_response_at = LEAST(t.first_response_at, s.first_response_at)
         FROM tickets s WHERE t.id = $2 AND s.id = $1",
    ] {
        sqlx::query(q)
            .bind(source)
            .bind(target)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)
}

/// Split the given messages of `ticket` into a new ticket with `subject`.
/// At least one message must stay behind. Returns the new ticket id.
pub async fn split(
    pool: &PgPool,
    ticket: Uuid,
    messages: &[Uuid],
    subject: &str,
) -> AppResult<Uuid> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let (total, picked): (i64, i64) = sqlx::query_as(
        "SELECT count(*), count(*) FILTER (WHERE id = ANY($2)) FROM ticket_messages WHERE ticket_id = $1",
    )
    .bind(ticket)
    .bind(messages)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if picked == 0 || picked != messages.len() as i64 || picked >= total {
        return Err(AppError::Validation("error.ticket_split_messages"));
    }
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO tickets (customer_id, opened_by, department_id, service_id, assigned_to, subject, status, priority, guest_email)
         SELECT customer_id, opened_by, department_id, service_id, assigned_to, $2, 'open', priority, guest_email FROM tickets WHERE id = $1 RETURNING id",
    )
    .bind(ticket)
    .bind(subject)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)?;
    for q in [
        "UPDATE ticket_messages SET ticket_id = $2 WHERE ticket_id = $1 AND id = ANY($3)",
        "UPDATE ticket_attachments SET ticket_id = $2 WHERE ticket_id = $1 AND message_id = ANY($3)",
    ] {
        sqlx::query(q)
            .bind(ticket)
            .bind(id)
            .bind(messages)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    sqlx::query(
        "UPDATE tickets SET last_reply_at = COALESCE((SELECT max(created_at) FROM ticket_messages WHERE ticket_id = $1), last_reply_at),
            first_response_at = (SELECT min(created_at) FROM ticket_messages WHERE ticket_id = $1 AND is_staff AND NOT is_internal)
         WHERE id = $1",
    )
    .bind(id)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(id)
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct EscalationRule {
    pub id: Uuid,
    pub name: String,
    pub department_id: Option<Uuid>,
    pub statuses: Vec<String>,
    pub priority: Option<String>,
    pub after_minutes: i32,
    pub set_priority: Option<String>,
    pub set_department: Option<Uuid>,
    pub assign_to: Option<Uuid>,
    pub add_tag: Option<String>,
    pub is_active: bool,
}

const RULE_COLS: &str = "id, name, department_id, statuses, priority, after_minutes, set_priority, set_department, assign_to, add_tag, is_active";

pub async fn rules(pool: &PgPool) -> AppResult<Vec<EscalationRule>> {
    sqlx::query_as(&format!(
        "SELECT {RULE_COLS} FROM ticket_escalation_rules ORDER BY name"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn upsert_rule(
    pool: &PgPool,
    id: Option<Uuid>,
    r: &EscalationRule,
) -> AppResult<EscalationRule> {
    let q = match id {
        Some(_) => format!("UPDATE ticket_escalation_rules SET name=$2, department_id=$3, statuses=$4, priority=$5, after_minutes=$6, set_priority=$7, set_department=$8, assign_to=$9, add_tag=$10, is_active=$11 WHERE id=$1 RETURNING {RULE_COLS}"),
        None => format!("INSERT INTO ticket_escalation_rules (id, name, department_id, statuses, priority, after_minutes, set_priority, set_department, assign_to, add_tag, is_active) VALUES (COALESCE($1, gen_random_uuid()),$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) RETURNING {RULE_COLS}"),
    };
    sqlx::query_as(&q)
        .bind(id)
        .bind(&r.name)
        .bind(r.department_id)
        .bind(&r.statuses)
        .bind(&r.priority)
        .bind(r.after_minutes)
        .bind(&r.set_priority)
        .bind(r.set_department)
        .bind(r.assign_to)
        .bind(&r.add_tag)
        .bind(r.is_active)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_rule(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM ticket_escalation_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Apply every active rule to tickets idle (no reply) for longer than its
/// threshold. Each rule fires at most once per ticket; returns the number of
/// (rule, ticket) escalations applied. Safe to run concurrently: the insert
/// into `ticket_escalations` is the claim.
pub async fn run_escalations(pool: &PgPool) -> AppResult<i64> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let fired: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "INSERT INTO ticket_escalations (rule_id, ticket_id)
         SELECT r.id, t.id FROM ticket_escalation_rules r JOIN tickets t
           ON (r.department_id IS NULL OR r.department_id = t.department_id)
          AND t.status = ANY(r.statuses)
          AND (r.priority IS NULL OR r.priority = t.priority)
          AND t.merged_into IS NULL
          AND t.last_reply_at <= now() - make_interval(mins => r.after_minutes)
         WHERE r.is_active
         ON CONFLICT DO NOTHING RETURNING rule_id, ticket_id",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(db_err)?;
    for (rule, ticket) in &fired {
        sqlx::query(
            "UPDATE tickets t SET priority = COALESCE(r.set_priority, t.priority),
                department_id = COALESCE(r.set_department, t.department_id),
                assigned_to = COALESCE(r.assign_to, t.assigned_to),
                escalated_at = now()
             FROM ticket_escalation_rules r WHERE r.id = $1 AND t.id = $2",
        )
        .bind(rule)
        .bind(ticket)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO ticket_tags (ticket_id, tag) SELECT $2, add_tag FROM ticket_escalation_rules
             WHERE id = $1 AND add_tag IS NOT NULL ON CONFLICT DO NOTHING",
        )
        .bind(rule)
        .bind(ticket)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok(fired.len() as i64)
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SlaRow {
    pub department_id: Uuid,
    pub department_name: String,
    pub sla_first_response_minutes: Option<i32>,
    pub tickets: i64,
    pub responded: i64,
    /// Average minutes to first staff response (whole minutes).
    pub avg_first_response_minutes: Option<i64>,
    /// Responded tickets within the department SLA.
    pub within_sla: Option<i64>,
    /// Open tickets already past the SLA without a response.
    pub breached_open: Option<i64>,
    pub ratings: i64,
    /// Average rating as text with two decimals (exact, no float).
    pub avg_rating: Option<String>,
}

/// Per-department SLA and satisfaction since `since`.
pub async fn sla(pool: &PgPool, since: OffsetDateTime) -> AppResult<Vec<SlaRow>> {
    sqlx::query_as(
        "SELECT d.id AS department_id, d.name AS department_name, d.sla_first_response_minutes,
            count(t.id) AS tickets,
            count(t.first_response_at) AS responded,
            round(avg(EXTRACT(EPOCH FROM t.first_response_at - t.created_at)) / 60)::bigint AS avg_first_response_minutes,
            CASE WHEN d.sla_first_response_minutes IS NULL THEN NULL ELSE
              count(*) FILTER (WHERE t.first_response_at <= t.created_at + make_interval(mins => d.sla_first_response_minutes)) END AS within_sla,
            CASE WHEN d.sla_first_response_minutes IS NULL THEN NULL ELSE
              count(*) FILTER (WHERE t.first_response_at IS NULL AND t.status <> 'closed' AND now() > t.created_at + make_interval(mins => d.sla_first_response_minutes)) END AS breached_open,
            count(t.rating) AS ratings,
            to_char(round(avg(t.rating)::numeric, 2), 'FM990.00') AS avg_rating
         FROM ticket_departments d
         LEFT JOIN tickets t ON t.department_id = d.id AND t.created_at >= $1 AND t.merged_into IS NULL
         GROUP BY d.id ORDER BY d.sort_order, d.name",
    )
    .bind(since)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Record an inbound message id; `false` if it was already processed.
pub async fn pipe_claim(
    pool: &PgPool,
    message_id: &str,
    sender: &str,
    subject: &str,
) -> AppResult<bool> {
    let r = sqlx::query(
        "INSERT INTO ticket_pipe_log (message_id, sender, subject, outcome) VALUES ($1,$2,$3,'rejected') ON CONFLICT DO NOTHING",
    )
    .bind(message_id)
    .bind(sender)
    .bind(subject)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(r.rows_affected() == 1)
}

pub async fn pipe_outcome(
    pool: &PgPool,
    message_id: &str,
    outcome: &str,
    ticket: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query("UPDATE ticket_pipe_log SET outcome = $2, ticket_id = $3 WHERE message_id = $1")
        .bind(message_id)
        .bind(outcome)
        .bind(ticket)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Customer with this login or billing e-mail.
pub async fn customer_by_email(pool: &PgPool, email: &str) -> AppResult<Option<(Uuid, Uuid)>> {
    sqlx::query_as(
        "SELECT c.id, u.id FROM customers c JOIN users u ON u.id = c.owner_user_id
         WHERE lower(c.email::text) = lower($1) OR lower(u.email::text) = lower($1) LIMIT 1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn ticket_by_number(
    pool: &PgPool,
    number: i64,
    customer: Uuid,
) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar(
        "SELECT COALESCE(merged_into, id) FROM tickets WHERE number = $1 AND customer_id = $2",
    )
    .bind(number)
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Department whose piping address matches (case-insensitive).
pub async fn department_by_email(pool: &PgPool, email: &str) -> AppResult<Option<Uuid>> {
    sqlx::query_scalar("SELECT id FROM ticket_departments WHERE email <> '' AND lower(email) = lower($1) AND is_active LIMIT 1")
        .bind(email)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::normalize_tag;
    #[test]
    fn tags_normalise() {
        assert_eq!(normalize_tag(" VIP Client ").as_deref(), Some("vip-client"));
        assert_eq!(normalize_tag("-x"), None);
        assert_eq!(normalize_tag(""), None);
        assert_eq!(normalize_tag("a!"), None);
        assert_eq!(normalize_tag(&"a".repeat(41)), None);
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Staff {
    pub id: Uuid,
    pub email: String,
    pub display_name: String,
}

/// Active users holding `tickets.manage` (valid assignees).
pub async fn staff(pool: &PgPool) -> AppResult<Vec<Staff>> {
    sqlx::query_as(
        "SELECT DISTINCT u.id, u.email::text AS email, u.display_name FROM users u
         JOIN user_roles ur ON ur.user_id = u.id JOIN role_permissions rp ON rp.role_id = ur.role_id
         JOIN permissions p ON p.id = rp.permission_id
         WHERE p.name = 'tickets.manage' AND u.is_active ORDER BY u.display_name, email",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn set_department_sla(
    pool: &PgPool,
    id: Uuid,
    email: &str,
    sla_minutes: Option<i32>,
) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE ticket_departments SET email = $2, sla_first_response_minutes = $3 WHERE id = $1",
    )
    .bind(id)
    .bind(email)
    .bind(sla_minutes)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct DepartmentSla {
    pub id: Uuid,
    pub email: String,
    pub sla_first_response_minutes: Option<i32>,
}

pub async fn department_slas(pool: &PgPool) -> AppResult<Vec<DepartmentSla>> {
    sqlx::query_as("SELECT id, email, sla_first_response_minutes FROM ticket_departments ORDER BY sort_order, name")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// WHMCS "Close Inactive Tickets": settings stored under this key.
pub const AUTO_CLOSE_KEY: &str = "support.auto_close";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AutoCloseSettings {
    /// Hours without a reply before closing (0 = off).
    pub hours: i32,
    /// Statuses that may auto-close (on hold / in progress never should).
    pub statuses: Vec<String>,
}

impl Default for AutoCloseSettings {
    fn default() -> Self {
        Self {
            hours: 0,
            statuses: vec!["answered".into(), "customer_reply".into()],
        }
    }
}

impl AutoCloseSettings {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !(0..=24 * 365).contains(&self.hours)
            || self.statuses.len() > 20
            || self
                .statuses
                .iter()
                .any(|x| x.is_empty() || x.len() > 32 || x == "closed")
        {
            return Err("error.validation");
        }
        Ok(())
    }
}

pub async fn get_auto_close(pool: &PgPool) -> AppResult<AutoCloseSettings> {
    Ok(crate::settings::get(pool, AUTO_CLOSE_KEY)
        .await?
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default())
}

pub async fn put_auto_close(pool: &PgPool, s: &AutoCloseSettings) -> AppResult<()> {
    crate::settings::set(
        pool,
        AUTO_CLOSE_KEY,
        serde_json::to_value(s).map_err(|e| AppError::Internal(e.to_string()))?,
    )
    .await
}

/// Close tickets idle for `hours` in one of `statuses`; returns their ids.
pub async fn close_inactive(
    pool: &PgPool,
    hours: i32,
    statuses: &[String],
) -> AppResult<Vec<Uuid>> {
    if hours <= 0 || statuses.is_empty() {
        return Ok(vec![]);
    }
    sqlx::query_scalar(
        "UPDATE tickets SET status = 'closed', closed_at = now()
         WHERE status = ANY($2) AND merged_into IS NULL AND last_reply_at <= now() - make_interval(hours => $1)
         RETURNING id",
    )
    .bind(hours)
    .bind(statuses)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}
