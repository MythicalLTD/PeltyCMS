//! Guest checkout (module `guest_checkout`): on stores (game servers) that
//! allow it, a buyer checks out with a player name or game identifier and
//! an email. The order goes to a shadow account keyed by that email, which
//! the buyer claims later by setting a password through an emailed link.
use crate::db_err;
use pelty_core::AppResult;
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MODULE: &str = "guest_checkout";
/// Mojang profile API (requests go through the app's guarded HTTP transport).
pub const MOJANG_API: &str = "https://api.mojang.com";

/// Module settings (tolerant read: bad values fall back to defaults).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// Look Minecraft Java names up with the Mojang API (canonical name + UUID).
    pub verify_minecraft: bool,
    /// Guest checkouts per IP per hour.
    pub ip_per_hour: i64,
    /// Guest checkouts per email per hour.
    pub email_per_hour: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            verify_minecraft: true,
            ip_per_hour: 10,
            email_per_hour: 5,
        }
    }
}

fn limit(v: Option<&Value>, def: i64) -> i64 {
    v.and_then(|v| v.as_i64().or_else(|| v.as_str()?.trim().parse().ok()))
        .filter(|n| (1..=1000).contains(n))
        .unwrap_or(def)
}

pub fn parse(v: &Value) -> Settings {
    let d = Settings::default();
    Settings {
        verify_minecraft: v
            .get("verify_minecraft")
            .and_then(Value::as_bool)
            .unwrap_or(d.verify_minecraft),
        ip_per_hour: limit(v.get("ip_per_hour"), d.ip_per_hour),
        email_per_hour: limit(v.get("email_per_hour"), d.email_per_hour),
    }
}

/// Settings when the module is on, else `None`.
pub async fn enabled(pool: &PgPool) -> AppResult<Option<Settings>> {
    if !crate::feature_modules::is_enabled(pool, MODULE).await? {
        return Ok(None);
    }
    Ok(Some(parse(
        &crate::feature_modules::settings(pool, MODULE).await?,
    )))
}

/// The game a guest checks out for: every product must be sold on an
/// enabled store with guest checkout on, and those stores must share a game.
pub async fn cart_game(pool: &PgPool, products: &[Uuid]) -> AppResult<Option<String>> {
    if products.is_empty() {
        return Ok(None);
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT DISTINCT gp.product_id, s.game FROM game_server_products gp
         JOIN game_servers s ON s.id = gp.server_id
         WHERE gp.product_id = ANY($1) AND s.enabled AND s.guest_checkout",
    )
    .bind(products)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(common_game(products, &rows))
}

/// Pure part of [`cart_game`]: the alphabetically first game every product
/// has a guest-enabled store for.
pub fn common_game(products: &[Uuid], rows: &[(Uuid, String)]) -> Option<String> {
    let mut common: Option<BTreeSet<&str>> = None;
    for p in products {
        let games: BTreeSet<&str> = rows
            .iter()
            .filter(|(id, _)| id == p)
            .map(|(_, g)| g.as_str())
            .collect();
        common = Some(match common {
            None => games,
            Some(c) => c.intersection(&games).copied().collect(),
        });
    }
    common?.into_iter().next().map(String::from)
}

/// The account for `email`, if any: `(user_id, is_unclaimed_shadow)`.
pub async fn account(pool: &PgPool, email: &str) -> AppResult<Option<(Uuid, bool)>> {
    sqlx::query_as(
        "SELECT u.id, (g.user_id IS NOT NULL AND g.claimed_at IS NULL)
         FROM users u LEFT JOIN guest_accounts g ON g.user_id = u.id
         WHERE lower(u.email::text) = lower($1)",
    )
    .bind(email.trim())
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn mark_shadow(pool: &PgPool, user: Uuid) -> AppResult<()> {
    sqlx::query("INSERT INTO guest_accounts (user_id) VALUES ($1) ON CONFLICT DO NOTHING")
        .bind(user)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The buyer set a password (claim link or a normal password reset).
pub async fn mark_claimed(pool: &PgPool, user: Uuid) -> AppResult<bool> {
    Ok(sqlx::query(
        "UPDATE guest_accounts SET claimed_at = now() WHERE user_id = $1 AND claimed_at IS NULL",
    )
    .bind(user)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected()
        > 0)
}

/// Is `user` an unclaimed shadow account?
pub async fn is_shadow(pool: &PgPool, user: Uuid) -> AppResult<bool> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM guest_accounts WHERE user_id = $1 AND claimed_at IS NULL)",
    )
    .bind(user)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Record one attempt and return the attempts in the last hour for this
/// IP and this email (this one included). Rows older than a day go.
pub async fn hit(pool: &PgPool, ip: &str, email: &str) -> AppResult<(i64, i64)> {
    let email = email.trim().to_lowercase();
    sqlx::query("DELETE FROM guest_checkout_attempts WHERE at < now() - interval '1 day'")
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO guest_checkout_attempts (ip, email) VALUES ($1, $2)")
        .bind(ip)
        .bind(&email)
        .execute(pool)
        .await
        .map_err(db_err)?;
    sqlx::query_as(
        "SELECT
           (SELECT count(*) FROM guest_checkout_attempts WHERE ip = $1 AND at > now() - interval '1 hour'),
           (SELECT count(*) FROM guest_checkout_attempts WHERE email = $2 AND at > now() - interval '1 hour')",
    )
    .bind(ip)
    .bind(&email)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// A Minecraft Java name as Mojang accepts it: 3-16 of `[A-Za-z0-9_]`.
pub fn minecraft_name_ok(n: &str) -> bool {
    (3..=16).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Mojang profile `{"id": "<32 hex>", "name": "<canonical>"}` →
/// `(name, dashed uuid)`.
pub fn parse_mojang(v: &Value) -> Option<(String, String)> {
    let name = v.get("name")?.as_str()?;
    let id = v.get("id")?.as_str()?;
    if !minecraft_name_ok(name) {
        return None;
    }
    Some((
        name.to_string(),
        crate::games::normalize_identity("uuid", id)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_are_tolerant() {
        assert_eq!(parse(&json!({})), Settings::default());
        let s = parse(&json!({"verify_minecraft": false, "ip_per_hour": "3", "email_per_hour": 0}));
        assert!(!s.verify_minecraft);
        assert_eq!(s.ip_per_hour, 3);
        assert_eq!(s.email_per_hour, 5);
        assert_eq!(parse(&json!({"ip_per_hour": 5000})).ip_per_hour, 10);
    }

    #[test]
    fn common_game_needs_every_product() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        let rows = vec![
            (a, "minecraft_java".to_string()),
            (a, "fivem".to_string()),
            (b, "fivem".to_string()),
        ];
        assert_eq!(common_game(&[a], &rows).as_deref(), Some("fivem"));
        assert_eq!(common_game(&[a, b], &rows).as_deref(), Some("fivem"));
        let only_mc = vec![(a, "minecraft_java".to_string())];
        assert_eq!(common_game(&[a, b], &only_mc), None);
        assert_eq!(common_game(&[], &rows), None);
    }

    #[test]
    fn mojang_profiles() {
        let p = json!({"id": "069a79f444e94726a5befca90e38aaf5", "name": "Notch"});
        assert_eq!(
            parse_mojang(&p),
            Some((
                "Notch".into(),
                "069a79f4-44e9-4726-a5be-fca90e38aaf5".into()
            ))
        );
        assert_eq!(parse_mojang(&json!({"id": "x", "name": "Notch"})), None);
        assert_eq!(
            parse_mojang(&json!({"id": "069a79f444e94726a5befca90e38aaf5", "name": "a;b"})),
            None
        );
        assert!(minecraft_name_ok("Steve_99"));
        assert!(!minecraft_name_ok("ab"));
        assert!(!minecraft_name_ok("this-has-dash"));
    }
}
