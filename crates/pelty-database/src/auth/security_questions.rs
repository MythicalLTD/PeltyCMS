//! Security questions (WHMCS Clients > Security Questions). Answers are
//! stored as password hashes of a normalised form and never returned.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Serialize;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Question {
    pub id: Uuid,
    pub question: String,
}

/// Case, surrounding and repeated whitespace do not matter.
pub fn normalize(answer: &str) -> String {
    answer
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub async fn list(pool: &PgPool) -> AppResult<Vec<Question>> {
    sqlx::query_as("SELECT id, question FROM security_questions ORDER BY created_at, id")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn create(pool: &PgPool, question: &str) -> AppResult<Uuid> {
    let q = question.trim();
    if q.is_empty() || q.chars().count() > 200 {
        return Err(AppError::Validation("error.validation"));
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO security_questions (id, question) VALUES ($1, $2)")
        .bind(id)
        .bind(q)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(id)
}

/// Deleting a question also deletes the answers given to it.
pub async fn delete(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM security_questions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Current {
    pub question_id: Uuid,
    pub question: String,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

pub async fn current(pool: &PgPool, user: Uuid) -> AppResult<Option<Current>> {
    sqlx::query_as(
        "SELECT a.question_id, q.question, a.updated_at FROM user_security_answers a JOIN security_questions q ON q.id = a.question_id WHERE a.user_id = $1",
    )
    .bind(user)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Validated, normalised answer ready for hashing.
pub fn prepare(answer: &str) -> AppResult<String> {
    let a = normalize(answer);
    if a.chars().count() < 2 || a.chars().count() > 200 {
        return Err(AppError::Validation("error.security_answer_length"));
    }
    Ok(a)
}

/// Store `hash` (a password hash of [`prepare`]d text) as the user's answer.
pub async fn set_answer(pool: &PgPool, user: Uuid, question: Uuid, hash: &str) -> AppResult<()> {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM security_questions WHERE id = $1)")
            .bind(question)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
    if !exists {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query(
        "INSERT INTO user_security_answers (user_id, question_id, answer_hash) VALUES ($1, $2, $3)
         ON CONFLICT (user_id) DO UPDATE SET question_id = EXCLUDED.question_id, answer_hash = EXCLUDED.answer_hash, updated_at = now()",
    )
    .bind(user)
    .bind(question)
    .bind(hash)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

pub async fn clear(pool: &PgPool, user: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM user_security_answers WHERE user_id = $1")
        .bind(user)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn answer_hash(pool: &PgPool, user: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar("SELECT answer_hash FROM user_security_answers WHERE user_id = $1")
        .bind(user)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalizes_answers() {
        assert_eq!(super::normalize("  Fluffy   The\tCat "), "fluffy the cat");
    }
}
