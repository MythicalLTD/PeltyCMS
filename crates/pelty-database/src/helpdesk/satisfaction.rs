//! Support satisfaction (CSAT) report from ticket ratings.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Summary {
    pub ratings: i64,
    /// Average 1-5, two decimals, `None` without ratings.
    pub avg_rating: Option<String>,
    /// Share of 4-5 ratings, percent (two decimals).
    pub csat: Option<String>,
    pub rated_tickets_share: Option<String>,
    pub r1: i64,
    pub r2: i64,
    pub r3: i64,
    pub r4: i64,
    pub r5: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Group {
    pub id: Option<Uuid>,
    pub name: String,
    pub ratings: i64,
    pub avg_rating: Option<String>,
    pub csat: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Comment {
    pub ticket_id: Uuid,
    pub number: i64,
    pub subject: String,
    pub customer_name: String,
    pub staff_name: Option<String>,
    pub rating: i16,
    pub comment: String,
    #[serde(with = "time::serde::rfc3339")]
    pub rated_at: OffsetDateTime,
}

#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct Report {
    pub summary: Summary,
    pub by_department: Vec<Group>,
    pub by_staff: Vec<Group>,
    pub comments: Vec<Comment>,
}

const AGG: &str = "count(t.rating) AS ratings,
    to_char(round(avg(t.rating)::numeric, 2), 'FM990.00') AS avg_rating,
    CASE WHEN count(t.rating) = 0 THEN NULL ELSE to_char(round(100.0 * count(*) FILTER (WHERE t.rating >= 4) / count(t.rating), 2), 'FM990.00') END AS csat";

/// `only_low` limits comments to 1-2 star ratings.
pub async fn report(pool: &PgPool, since: OffsetDateTime, department: Option<Uuid>, only_low: bool) -> AppResult<Report> {
    let filter = "t.rating IS NOT NULL AND t.rated_at >= $1 AND ($2::uuid IS NULL OR t.department_id = $2) AND t.merged_into IS NULL";
    let summary: Summary = sqlx::query_as(&format!(
        "SELECT {AGG},
            (SELECT CASE WHEN count(*) = 0 THEN NULL ELSE to_char(round(100.0 * count(rating) / count(*), 2), 'FM990.00') END
               FROM tickets x WHERE x.first_response_at IS NOT NULL AND x.created_at >= $1 AND ($2::uuid IS NULL OR x.department_id = $2) AND x.merged_into IS NULL) AS rated_tickets_share,
            count(*) FILTER (WHERE t.rating = 1) AS r1, count(*) FILTER (WHERE t.rating = 2) AS r2, count(*) FILTER (WHERE t.rating = 3) AS r3,
            count(*) FILTER (WHERE t.rating = 4) AS r4, count(*) FILTER (WHERE t.rating = 5) AS r5
         FROM tickets t WHERE {filter}"
    ))
    .bind(since).bind(department).fetch_one(pool).await.map_err(db_err)?;
    let by_department: Vec<Group> = sqlx::query_as(&format!(
        "SELECT d.id AS id, d.name AS name, {AGG} FROM tickets t JOIN ticket_departments d ON d.id = t.department_id
         WHERE {filter} GROUP BY d.id ORDER BY count(t.rating) DESC, d.name"
    ))
    .bind(since).bind(department).fetch_all(pool).await.map_err(db_err)?;
    let by_staff: Vec<Group> = sqlx::query_as(&format!(
        "SELECT u.id AS id, COALESCE(NULLIF(u.display_name, ''), u.email::text, '') AS name, {AGG}
         FROM tickets t LEFT JOIN users u ON u.id = t.assigned_to
         WHERE {filter} GROUP BY u.id ORDER BY count(t.rating) DESC, 2"
    ))
    .bind(since).bind(department).fetch_all(pool).await.map_err(db_err)?;
    let comments: Vec<Comment> = sqlx::query_as(&format!(
        "SELECT t.id AS ticket_id, t.number::bigint AS number, t.subject,
            COALESCE(NULLIF(trim(c.first_name || ' ' || c.last_name), ''), c.organization, c.email::text) AS customer_name,
            NULLIF(u.display_name, '') AS staff_name, t.rating, t.rating_comment AS comment, t.rated_at
         FROM tickets t JOIN customers c ON c.id = t.customer_id LEFT JOIN users u ON u.id = t.assigned_to
         WHERE {filter} AND t.rating_comment <> '' AND (NOT $3 OR t.rating <= 2)
         ORDER BY t.rated_at DESC LIMIT 50"
    ))
    .bind(since).bind(department).bind(only_low).fetch_all(pool).await.map_err(db_err)?;
    Ok(Report { summary, by_department, by_staff, comments })
}
