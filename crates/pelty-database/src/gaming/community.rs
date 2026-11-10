//! Server community (module `server_community`): cached server status, vote
//! sites (HTTP vote callbacks verified with a per-site token, rewards queued
//! through game delivery), monthly top voters / donors and a monthly goal.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::str::FromStr;
use time::OffsetDateTime;
use uuid::Uuid;

pub const MODULE: &str = "server_community";

/// Module settings (stored in `feature_modules.settings`).
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
#[schema(as = CommunityConfig)]
pub struct Config {
    /// Monthly goal; `0` hides the goal bar.
    #[schema(value_type = String)]
    pub goal_amount: Decimal,
    pub goal_currency: String,
    pub goal_label: String,
    /// Rows in each leaderboard (1–50).
    pub leaderboard_size: i64,
    /// Show the top donors list.
    pub show_donors: bool,
    /// Show donor totals next to their names.
    pub show_donor_amounts: bool,
}

impl Config {
    pub fn from_value(v: &Value) -> Self {
        let s = |k: &str| {
            v.get(k)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
        let goal = match v.get("goal_amount") {
            Some(Value::String(x)) => Decimal::from_str(x.trim()).unwrap_or_default(),
            Some(Value::Number(n)) => Decimal::from_str(&n.to_string()).unwrap_or_default(),
            _ => Decimal::ZERO,
        };
        let cur = s("goal_currency").to_ascii_uppercase();
        Self {
            goal_amount: goal.max(Decimal::ZERO),
            goal_currency: if cur.len() == 3 && cur.bytes().all(|c| c.is_ascii_uppercase()) {
                cur
            } else {
                "EUR".into()
            },
            goal_label: s("goal_label").chars().take(120).collect(),
            leaderboard_size: v
                .get("leaderboard_size")
                .and_then(Value::as_i64)
                .unwrap_or(10)
                .clamp(1, 50),
            show_donors: b("show_donors", false),
            show_donor_amounts: b("show_donor_amounts", false),
        }
    }
}

pub async fn config(pool: &PgPool) -> AppResult<Config> {
    Ok(Config::from_value(
        &crate::feature_modules::settings(pool, MODULE).await?,
    ))
}

// ------------------------------------------------------------ status

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct ServerStatus {
    pub server_id: Uuid,
    pub name: String,
    pub slug: String,
    pub game: String,
    /// Address players connect to (`host` or `host:port`).
    pub address: String,
    pub online: bool,
    pub players: i32,
    pub max_players: i32,
    pub version: String,
    pub motd: String,
    pub sample: Vec<String>,
    pub latency_ms: Option<i32>,
    pub peak_players: i32,
    #[serde(with = "time::serde::rfc3339::option")]
    pub checked_at: Option<OffsetDateTime>,
}

/// Enabled servers with their last status (never-checked servers show offline).
pub async fn statuses(pool: &PgPool) -> AppResult<Vec<ServerStatus>> {
    sqlx::query_as(
        "SELECT s.id AS server_id, s.name, s.slug, s.game,
                CASE WHEN s.port IS NULL THEN s.host ELSE s.host || ':' || s.port END AS address,
                COALESCE(st.online, false) AS online, COALESCE(st.players, 0) AS players,
                COALESCE(st.max_players, 0) AS max_players, COALESCE(st.version, '') AS version,
                COALESCE(st.motd, '') AS motd, COALESCE(st.sample, '{}') AS sample, st.latency_ms,
                COALESCE(st.peak_players, 0) AS peak_players, st.checked_at
           FROM game_servers s LEFT JOIN game_server_status st ON st.server_id = s.id
          WHERE s.enabled ORDER BY s.sort_order, s.name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Servers to query: (id, game, host, port, query port).
pub async fn targets(
    pool: &PgPool,
) -> AppResult<Vec<(Uuid, String, String, Option<i32>, Option<i32>)>> {
    sqlx::query_as(
        "SELECT id, game, host, port, query_port FROM game_servers WHERE enabled AND host <> ''",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub struct Observed<'a> {
    pub players: i32,
    pub max_players: i32,
    pub version: &'a str,
    pub motd: &'a str,
    pub sample: &'a [String],
    pub latency_ms: i32,
}

pub async fn record_status(
    pool: &PgPool,
    server: Uuid,
    r: Result<Observed<'_>, &str>,
) -> AppResult<()> {
    let clip = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    match r {
        Ok(o) => sqlx::query(
            "INSERT INTO game_server_status (server_id, online, players, max_players, version, motd, sample, latency_ms, error, peak_players, checked_at)
             VALUES ($1, true, $2, $3, $4, $5, $6, $7, '', $2, now())
             ON CONFLICT (server_id) DO UPDATE SET online = true, players = $2, max_players = $3, version = $4, motd = $5,
               sample = $6, latency_ms = $7, error = '', checked_at = now(),
               peak_players = GREATEST(CASE WHEN date_trunc('month', game_server_status.checked_at) = date_trunc('month', now())
                                            THEN game_server_status.peak_players ELSE 0 END, $2)",
        )
        .bind(server)
        .bind(o.players.max(0))
        .bind(o.max_players.max(0))
        .bind(clip(o.version, 100))
        .bind(clip(o.motd, 300))
        .bind(o.sample.iter().take(12).map(|x| clip(x, 64)).collect::<Vec<_>>())
        .bind(o.latency_ms)
        .execute(pool)
        .await,
        Err(e) => sqlx::query(
            "INSERT INTO game_server_status (server_id, online, error, checked_at) VALUES ($1, false, $2, now())
             ON CONFLICT (server_id) DO UPDATE SET online = false, players = 0, sample = '{}', latency_ms = NULL, error = $2, checked_at = now(),
               peak_players = CASE WHEN date_trunc('month', game_server_status.checked_at) = date_trunc('month', now())
                                   THEN game_server_status.peak_players ELSE 0 END",
        )
        .bind(server)
        .bind(clip(e, 300))
        .execute(pool)
        .await,
    }
    .map_err(db_err)?;
    Ok(())
}

/// Last error per server (staff only).
pub async fn errors(pool: &PgPool) -> AppResult<Vec<(Uuid, String)>> {
    sqlx::query_as("SELECT server_id, error FROM game_server_status WHERE error <> ''")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

// ------------------------------------------------------------ vote sites

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoteSite {
    pub id: Uuid,
    pub name: String,
    pub url: String,
    pub has_token: bool,
    pub server_id: Option<Uuid>,
    pub reward_command: String,
    pub require_online: bool,
    pub cooldown_hours: i32,
    pub enabled: bool,
    pub sort_order: i32,
}

const SITE_COLS: &str = "id, name, url, token_hash IS NOT NULL AS has_token, server_id, reward_command, require_online, cooldown_hours, enabled, sort_order";

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct VoteSiteInput {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub server_id: Option<Uuid>,
    #[serde(default)]
    pub reward_command: String,
    #[serde(default)]
    pub require_online: bool,
    #[serde(default = "day")]
    pub cooldown_hours: i32,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i32,
}
fn day() -> i32 {
    24
}
fn yes() -> bool {
    true
}

impl VoteSiteInput {
    fn check(&self) -> AppResult<()> {
        let url_ok = (self.url.starts_with("https://") || self.url.starts_with("http://"))
            && self.url.len() <= 500
            && !self.url.chars().any(char::is_whitespace);
        if self.name.trim().is_empty()
            || self.name.chars().count() > 80
            || !url_ok
            || self.reward_command.chars().count() > 1000
            || self.reward_command.contains(['\r', '\n', '\0'])
            || !(0..=168).contains(&self.cooldown_hours)
        {
            return Err(AppError::Validation("error.vote_site_invalid"));
        }
        let vote_vars = ["player", "site", "server"];
        if crate::game_delivery::placeholders(&self.reward_command)
            .iter()
            .any(|p| {
                !crate::game_delivery::PLACEHOLDERS.contains(&p.as_str())
                    && !vote_vars.contains(&p.as_str())
            })
        {
            return Err(AppError::Validation("error.game_placeholder_unknown"));
        }
        Ok(())
    }
}

pub async fn sites(pool: &PgPool, only_enabled: bool) -> AppResult<Vec<VoteSite>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SITE_COLS} FROM vote_sites WHERE (NOT $1 OR enabled) ORDER BY sort_order, name"
    )))
    .bind(only_enabled)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn site(pool: &PgPool, id: Uuid) -> AppResult<VoteSite> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SITE_COLS} FROM vote_sites WHERE id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn save_site(pool: &PgPool, id: Option<Uuid>, i: &VoteSiteInput) -> AppResult<VoteSite> {
    i.check()?;
    let q = match id {
        None => format!(
            "INSERT INTO vote_sites (id, name, url, server_id, reward_command, require_online, cooldown_hours, enabled, sort_order)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING {SITE_COLS}"
        ),
        Some(_) => format!(
            "UPDATE vote_sites SET name=$2, url=$3, server_id=$4, reward_command=$5, require_online=$6, cooldown_hours=$7, enabled=$8, sort_order=$9
             WHERE id = $1 RETURNING {SITE_COLS}"
        ),
    };
    sqlx::query_as(sqlx::AssertSqlSafe(q))
        .bind(id.unwrap_or_else(Uuid::new_v4))
        .bind(i.name.trim())
        .bind(i.url.trim())
        .bind(i.server_id)
        .bind(i.reward_command.trim())
        .bind(i.require_online)
        .bind(i.cooldown_hours)
        .bind(i.enabled)
        .bind(i.sort_order)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_site(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM vote_sites WHERE id = $1")
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

pub async fn set_token(pool: &PgPool, id: Uuid, hash: &[u8]) -> AppResult<()> {
    let n = sqlx::query("UPDATE vote_sites SET token_hash = $2 WHERE id = $1")
        .bind(id)
        .bind(hash)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn token_hash(pool: &PgPool, id: Uuid) -> AppResult<Option<Vec<u8>>> {
    Ok(sqlx::query_scalar::<_, Option<Vec<u8>>>(
        "SELECT token_hash FROM vote_sites WHERE id = $1 AND enabled",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .flatten())
}

// ------------------------------------------------------------ votes

pub fn username_ok(u: &str) -> bool {
    (1..=64).contains(&u.chars().count())
        && u.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ' '))
}

/// Record a vote unless the same name voted on this site within the cooldown
/// (`Ok(None)`). Serialized per site and name with an advisory lock.
pub async fn record_vote(
    pool: &PgPool,
    site: &VoteSite,
    username: &str,
    ip: &str,
) -> AppResult<Option<(Uuid, Option<Uuid>)>> {
    let name = username.trim();
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('vote:' || $1::text || ':' || lower($2)))")
        .bind(site.id)
        .bind(name)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let recent: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM votes WHERE site_id = $1 AND lower(username) = lower($2)
                         AND created_at > now() - make_interval(hours => $3))",
    )
    .bind(site.id)
    .bind(name)
    .bind(site.cooldown_hours)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if recent && site.cooldown_hours > 0 {
        return Ok(None);
    }
    // Link to the customer whose player name (never an ID field) matches, on
    // the target server's game when the site rewards a single server.
    let customer: Option<Uuid> = sqlx::query_scalar(
        "SELECT a.customer_id FROM customer_game_accounts a
          WHERE a.field = 'username' AND lower(a.value) = lower($1)
            AND ($2::uuid IS NULL OR a.game = (SELECT game FROM game_servers WHERE id = $2))
          ORDER BY a.created_at LIMIT 1",
    )
    .bind(name)
    .bind(site.server_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(db_err)?;
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO votes (id, site_id, username, customer_id, ip) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(id)
    .bind(site.id)
    .bind(name)
    .bind(customer)
    .bind(ip.chars().take(64).collect::<String>())
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok(Some((id, customer)))
}

/// Undo a vote whose reward could not be queued, so a retry is not blocked
/// by the cooldown.
/// Returns `None` when the vote was removed. When some of its deliveries were
/// already queued the vote is kept (a retry would get a new vote id and queue
/// the reward again) and the number queued is returned.
pub async fn delete_vote(pool: &PgPool, vote: Uuid) -> AppResult<Option<i64>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let queued: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM game_deliveries WHERE trigger = 'vote' AND dedupe_key LIKE 'vote:' || $1::text || ':%'",
    )
    .bind(vote)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if queued > 0 {
        sqlx::query("UPDATE votes SET rewarded = $2 WHERE id = $1")
            .bind(vote)
            .bind(i32::try_from(queued).unwrap_or(i32::MAX))
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    } else {
        sqlx::query("DELETE FROM votes WHERE id = $1")
            .bind(vote)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    Ok((queued > 0).then_some(queued))
}

pub async fn set_rewarded(pool: &PgPool, vote: Uuid, n: i32) -> AppResult<()> {
    sqlx::query("UPDATE votes SET rewarded = $2 WHERE id = $1")
        .bind(vote)
        .bind(n)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct VoteRow {
    pub id: Uuid,
    pub site: String,
    pub username: String,
    pub customer_id: Option<Uuid>,
    pub ip: String,
    pub rewarded: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub async fn recent_votes(pool: &PgPool, limit: i64) -> AppResult<Vec<VoteRow>> {
    sqlx::query_as(
        "SELECT v.id, s.name AS site, v.username, v.customer_id, v.ip, v.rewarded, v.created_at
           FROM votes v JOIN vote_sites s ON s.id = v.site_id ORDER BY v.created_at DESC LIMIT $1",
    )
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ------------------------------------------------------------ leaderboards

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TopVoter {
    pub username: String,
    pub votes: i64,
}

pub async fn top_voters(pool: &PgPool, limit: i64) -> AppResult<Vec<TopVoter>> {
    sqlx::query_as(
        "SELECT min(username) AS username, count(*) AS votes FROM votes
          WHERE created_at >= date_trunc('month', now()) GROUP BY lower(username)
          ORDER BY votes DESC, min(created_at) LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct TopDonor {
    /// Linked in-game name, else "First L.".
    pub name: String,
    #[schema(value_type = String)]
    pub total: Decimal,
}

/// Paid invoices this month in `currency`, per customer.
pub async fn top_donors(pool: &PgPool, currency: &str, limit: i64) -> AppResult<Vec<TopDonor>> {
    sqlx::query_as(
        "SELECT COALESCE((SELECT a.value FROM customer_game_accounts a WHERE a.customer_id = c.id AND a.field = 'username' ORDER BY a.created_at LIMIT 1),
                         NULLIF(trim(c.first_name || ' ' || left(c.last_name, 1) || CASE WHEN c.last_name <> '' THEN '.' ELSE '' END), ''),
                         'Anonymous') AS name,
                sum(i.total) AS total
           FROM invoices i JOIN customers c ON c.id = i.customer_id
          WHERE i.status = 'paid' AND i.kind = 'invoice' AND i.currency = $1 AND i.paid_at >= date_trunc('month', now())
          GROUP BY c.id ORDER BY total DESC LIMIT $2",
    )
    .bind(currency)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Total paid this month in `currency`.
pub async fn month_total(pool: &PgPool, currency: &str) -> AppResult<Decimal> {
    sqlx::query_scalar(
        "SELECT COALESCE(sum(total), 0) FROM invoices
          WHERE status = 'paid' AND kind = 'invoice' AND currency = $1 AND paid_at >= date_trunc('month', now())",
    )
    .bind(currency)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}
