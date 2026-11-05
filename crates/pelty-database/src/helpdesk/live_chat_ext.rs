//! Settings, assignment and atomic transcript conversion for live chat.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct OpeningHours {
    /// ISO weekday: Monday = 1, Sunday = 7. UTC, no implicit server timezone.
    pub day: u8,
    pub start_minute: u16,
    pub end_minute: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChatSettings {
    pub enabled: bool,
    /// Empty means all active ticket departments.
    pub departments: Vec<Uuid>,
    /// Empty means always open. Windows do not span midnight.
    pub opening_hours: Vec<OpeningHours>,
}
impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            departments: vec![],
            opening_hours: vec![],
        }
    }
}
impl ChatSettings {
    pub fn validate(&self) -> AppResult<()> {
        if self.departments.len() > 100
            || self.opening_hours.len() > 28
            || self.opening_hours.iter().any(|h| {
                !(1..=7).contains(&h.day) || h.start_minute >= h.end_minute || h.end_minute > 1440
            })
        {
            return Err(AppError::Validation("error.validation"));
        }
        Ok(())
    }
    pub fn is_open(&self, now: OffsetDateTime) -> bool {
        let minute = u16::from(now.hour()) * 60 + u16::from(now.minute());
        self.enabled
            && (self.opening_hours.is_empty()
                || self.opening_hours.iter().any(|h| {
                    h.day == now.weekday().number_from_monday()
                        && h.start_minute <= minute
                        && minute < h.end_minute
                }))
    }
}
pub async fn settings(pool: &PgPool) -> AppResult<ChatSettings> {
    crate::settings::get(pool, "support.chat")
        .await?
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| AppError::Internal(e.to_string()))
        .map(|s| s.unwrap_or_default())
}
pub async fn save_settings(pool: &PgPool, s: &ChatSettings) -> AppResult<()> {
    s.validate()?;
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM ticket_departments WHERE id = ANY($1) AND is_active",
    )
    .bind(&s.departments)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if count as usize != s.departments.len() {
        return Err(AppError::Validation("error.invalid_department"));
    }
    crate::settings::set(pool, "support.chat", serde_json::json!(s)).await
}

pub async fn assign(pool: &PgPool, id: Uuid, staff: Option<Uuid>) -> AppResult<()> {
    if let Some(staff) = staff {
        if !crate::support::staff(pool)
            .await?
            .iter()
            .any(|u| u.id == staff)
        {
            return Err(AppError::Validation("error.validation"));
        }
    }
    let n =
        sqlx::query("UPDATE chat_sessions SET assigned_to = $2 WHERE id = $1 AND status = 'open'")
            .bind(id)
            .bind(staff)
            .execute(pool)
            .await
            .map_err(db_err)?
            .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("error.chat_closed"));
    }
    Ok(())
}
/// The row lock serializes conversion with message writes and other conversions.
/// Never infer customer ownership from the unverified visitor email address.
pub async fn convert(
    pool: &PgPool,
    id: Uuid,
    staff: Uuid,
    customer: Uuid,
    department: Uuid,
    subject: &str,
) -> AppResult<(Uuid, bool)> {
    let subject = subject.trim();
    if subject.is_empty() || subject.chars().count() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    let (existing, linked, assigned): (Option<Uuid>, Option<Uuid>, Option<Uuid>) = sqlx::query_as(
        "SELECT ticket_id, customer_id, assigned_to FROM chat_sessions WHERE id = $1 FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)?;
    if let Some(ticket) = existing {
        return Ok((ticket, false));
    }
    if linked.is_some_and(|c| c != customer) {
        return Err(AppError::Validation("error.validation"));
    }
    let valid: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM customers WHERE id = $1) AND EXISTS(SELECT 1 FROM ticket_departments WHERE id = $2 AND is_active)")
        .bind(customer).bind(department).fetch_one(&mut *tx).await.map_err(db_err)?;
    if !valid {
        return Err(AppError::Validation("error.validation"));
    }
    let ticket: Uuid = sqlx::query_scalar("INSERT INTO tickets(customer_id, opened_by, department_id, subject, priority, status, assigned_to) VALUES ($1,$2,$3,$4,'medium','open',$5) RETURNING id")
        .bind(customer).bind(staff).bind(department).bind(subject).bind(assigned).fetch_one(&mut *tx).await.map_err(db_err)?;
    // Each original message keeps its timestamp, staff attribution and exact body.
    // Prefix visitor/system entries so anonymous authors remain identifiable.
    sqlx::query("INSERT INTO ticket_messages(ticket_id, author_id, is_staff, is_internal, body, created_at) SELECT $2, m.staff_user_id, m.author = 'staff', false, CASE WHEN m.author = 'visitor' THEN '[Live chat visitor: ' || s.name || ' <' || s.email || '>] ' || m.body WHEN m.author = 'system' THEN '[Live chat system] ' || m.body ELSE m.body END, m.created_at FROM chat_messages m JOIN chat_sessions s ON s.id = m.session_id WHERE m.session_id = $1 ORDER BY m.id")
        .bind(id).bind(ticket).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE chat_sessions SET ticket_id = $2, customer_id = $3, department_id = $4, status = 'closed', closed_at = COALESCE(closed_at, now()) WHERE id = $1")
        .bind(id).bind(ticket).bind(customer).bind(department).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE tickets SET created_at = (SELECT created_at FROM chat_sessions WHERE id = $2), last_reply_at = (SELECT max(created_at) FROM chat_messages WHERE session_id = $2), first_response_at = (SELECT min(created_at) FROM chat_messages WHERE session_id = $2 AND author = 'staff') WHERE id = $1")
        .bind(ticket).bind(id).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("INSERT INTO audit_logs(actor_id, action, target, metadata) VALUES ($1,'chat.converted',$2,$3)")
        .bind(staff).bind(id.to_string()).bind(serde_json::json!({"ticket_id":ticket,"customer_id":customer})).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((ticket, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_hours_bounds() {
        let mut s = ChatSettings::default();
        let now = OffsetDateTime::from_unix_timestamp(0).unwrap();
        assert!(s.is_open(now));
        s.opening_hours.push(OpeningHours {
            day: 4,
            start_minute: 0,
            end_minute: 1,
        });
        assert!(s.validate().is_ok());
        assert!(s.is_open(now));
        assert!(!s.is_open(now + time::Duration::minutes(1)));
        s.opening_hours[0].end_minute = 1441;
        assert!(s.validate().is_err());
    }
}
