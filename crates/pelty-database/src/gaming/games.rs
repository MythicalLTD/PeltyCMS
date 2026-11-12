//! Game profiles (module `game_profiles`): the games PeltyCMS knows, their
//! player identity fields, game servers with a store each, and the game
//! accounts customers link (Minecraft name/UUID, Steam ID, FiveM license, ...).
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, utoipa::ToSchema)]
pub struct GameDef {
    pub id: &'static str,
    pub name: &'static str,
    /// Identity fields a customer can link; the first one is the player name.
    pub fields: &'static [&'static str],
    /// Status query protocol: `minecraft`, `bedrock`, `a2s`, `fivem` or `none`.
    pub query: &'static str,
    pub default_port: u16,
    /// Suggested command transport.
    pub transport: &'static str,
}

const fn g(
    id: &'static str,
    name: &'static str,
    fields: &'static [&'static str],
    query: &'static str,
    default_port: u16,
    transport: &'static str,
) -> GameDef {
    GameDef {
        id,
        name,
        fields,
        query,
        default_port,
        transport,
    }
}

pub const GAMES: &[GameDef] = &[
    g(
        "minecraft_java",
        "Minecraft: Java Edition",
        &["username", "uuid", "discord_id"],
        "minecraft",
        25565,
        "rcon",
    ),
    g(
        "minecraft_bedrock",
        "Minecraft: Bedrock Edition",
        &["username", "xuid", "discord_id"],
        "bedrock",
        19132,
        "agent",
    ),
    g(
        "fivem",
        "FiveM (GTA V)",
        &["username", "license", "steamid", "discord_id", "fivem"],
        "fivem",
        30120,
        "agent",
    ),
    g(
        "redm",
        "RedM (Red Dead Redemption 2)",
        &["username", "license", "steamid", "discord_id", "fivem"],
        "fivem",
        30120,
        "agent",
    ),
    g(
        "rust",
        "Rust",
        &["username", "steamid", "discord_id"],
        "a2s",
        28015,
        "rcon",
    ),
    g(
        "gmod",
        "Garry's Mod",
        &["username", "steamid", "discord_id"],
        "a2s",
        27015,
        "rcon",
    ),
    g(
        "ark",
        "ARK: Survival Evolved / Ascended",
        &["username", "steamid", "discord_id"],
        "a2s",
        27015,
        "rcon",
    ),
    g(
        "cs2",
        "Counter-Strike 2",
        &["username", "steamid", "discord_id"],
        "a2s",
        27015,
        "rcon",
    ),
    g(
        "unturned",
        "Unturned",
        &["username", "steamid", "discord_id"],
        "a2s",
        27015,
        "agent",
    ),
    g(
        "generic",
        "Other game",
        &["username", "discord_id"],
        "none",
        0,
        "agent",
    ),
];

pub fn game(id: &str) -> Option<&'static GameDef> {
    GAMES.iter().find(|g| g.id == id)
}

fn all(s: &str, f: impl Fn(char) -> bool) -> bool {
    !s.is_empty() && s.chars().all(f)
}

/// Validate and normalize one identity value. `None` when it is invalid.
pub fn normalize_identity(field: &str, value: &str) -> Option<String> {
    let v = value.trim();
    let ok = match field {
        // Minecraft Java names are 3-16 of [A-Za-z0-9_]; other games allow
        // more, but never characters that could break a console command.
        "username" => {
            (1..=32).contains(&v.chars().count())
                && all(v, |c| {
                    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')
                })
        }
        "uuid" => {
            let h: String = v.chars().filter(|c| *c != '-').collect();
            return (h.len() == 32 && all(&h, |c| c.is_ascii_hexdigit())).then(|| {
                let h = h.to_ascii_lowercase();
                format!(
                    "{}-{}-{}-{}-{}",
                    &h[0..8],
                    &h[8..12],
                    &h[12..16],
                    &h[16..20],
                    &h[20..]
                )
            });
        }
        "steamid" => v.len() == 17 && v.starts_with("7656119") && all(v, |c| c.is_ascii_digit()),
        "xuid" => (15..=17).contains(&v.len()) && all(v, |c| c.is_ascii_digit()),
        "discord_id" => (17..=20).contains(&v.len()) && all(v, |c| c.is_ascii_digit()),
        "license" => {
            let h = v.strip_prefix("license:").unwrap_or(v);
            return (h.len() == 40 && all(h, |c| c.is_ascii_hexdigit()))
                .then(|| h.to_ascii_lowercase());
        }
        // Cfx.re account id (`fivem:123456` in-game); stored without prefix
        // like `license`.
        "fivem" => {
            let h = v.strip_prefix("fivem:").unwrap_or(v);
            return ((1..=12).contains(&h.len()) && all(h, |c| c.is_ascii_digit()))
                .then(|| h.to_string());
        }
        _ => false,
    };
    ok.then(|| v.to_string())
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(as = GameServerInput)]
pub struct ServerInput {
    pub game: String,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: Option<i32>,
    #[serde(default)]
    pub query_port: Option<i32>,
    #[serde(default = "none")]
    pub transport: String,
    #[serde(default)]
    pub rcon_host: String,
    #[serde(default)]
    pub rcon_port: Option<i32>,
    /// New RCON password; omit to keep, empty string to clear.
    #[serde(default)]
    pub rcon_password: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i32,
    /// Module `guest_checkout`: buyers may check out on this store with only
    /// a player name / game identifier and an email.
    #[serde(default)]
    pub guest_checkout: bool,
}
fn none() -> String {
    "none".into()
}
fn yes() -> bool {
    true
}

/// Host names and IPs only: no scheme, path, spaces or credentials.
pub fn host_ok(h: &str) -> bool {
    h.is_empty()
        || (h.len() <= 255
            && all(h, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '[' | ']')
            }))
}

impl ServerInput {
    pub fn validate(&self) -> AppResult<()> {
        let bad = game(&self.game).is_none()
            || self.name.trim().is_empty()
            || self.name.chars().count() > 120
            || self.description.chars().count() > 2000
            || !matches!(self.transport.as_str(), "none" | "rcon" | "agent")
            || !host_ok(&self.host)
            || !host_ok(&self.rcon_host)
            || [self.port, self.query_port, self.rcon_port]
                .iter()
                .flatten()
                .any(|p| !(1..=65535).contains(p))
            || self.rcon_password.as_ref().is_some_and(|p| p.len() > 200)
            || self.slug.is_empty()
            || self.slug.len() > 64
            || !all(&self.slug, |c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'
            })
            || self.slug.starts_with('-');
        if bad {
            return Err(AppError::Validation("error.validation"));
        }
        if self.transport == "rcon" && (self.rcon_host.is_empty() && self.host.is_empty()) {
            return Err(AppError::Validation("error.game_rcon_host"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = GameServer)]
pub struct Server {
    pub id: Uuid,
    pub game: String,
    pub name: String,
    pub slug: String,
    pub description: String,
    pub host: String,
    pub port: Option<i32>,
    pub query_port: Option<i32>,
    pub transport: String,
    pub rcon_host: String,
    pub rcon_port: Option<i32>,
    pub has_rcon_password: bool,
    pub has_agent_token: bool,
    pub enabled: bool,
    pub sort_order: i32,
    pub guest_checkout: bool,
    pub product_ids: Vec<Uuid>,
}

const SERVER_COLS: &str = "s.id, s.game, s.name, s.slug, s.description, s.host, s.port, s.query_port, s.transport, s.rcon_host, s.rcon_port, s.rcon_password IS NOT NULL AS has_rcon_password, s.agent_token IS NOT NULL AS has_agent_token, s.enabled, s.sort_order, s.guest_checkout, COALESCE((SELECT array_agg(p.product_id ORDER BY p.sort_order, p.product_id) FROM game_server_products p WHERE p.server_id = s.id), '{}') AS product_ids";

pub async fn servers(pool: &PgPool, only_enabled: bool) -> AppResult<Vec<Server>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SERVER_COLS} FROM game_servers s WHERE ($1 = false OR s.enabled) ORDER BY s.sort_order, s.name"
    )))
    .bind(only_enabled)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn server(pool: &PgPool, id: Uuid) -> AppResult<Server> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SERVER_COLS} FROM game_servers s WHERE s.id = $1"
    )))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn server_by_slug(pool: &PgPool, slug: &str) -> AppResult<Server> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SERVER_COLS} FROM game_servers s WHERE s.slug = $1 AND s.enabled"
    )))
    .bind(slug)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

/// Public fields only (no RCON or agent details).
pub fn public_view(s: &Server) -> Value {
    json!({
        "id": s.id, "game": s.game, "name": s.name, "slug": s.slug,
        "description": s.description, "host": s.host, "port": s.port,
        "guest_checkout": s.guest_checkout,
    })
}

/// Create (`id = None`) or update a server. `rcon_password`: `Some(None)` clears,
/// `Some(Some(bytes))` replaces, `None` keeps.
pub async fn save_server(
    pool: &PgPool,
    id: Option<Uuid>,
    b: &ServerInput,
    rcon_password: Option<Option<Vec<u8>>>,
) -> AppResult<Server> {
    b.validate()?;
    let sql = if id.is_none() {
        "INSERT INTO game_servers (game,name,slug,description,host,port,query_port,transport,rcon_host,rcon_port,enabled,sort_order,rcon_password,guest_checkout)
         SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$16 WHERE $14 OR NOT $14 OR $15::uuid IS NULL RETURNING id"
    } else {
        "UPDATE game_servers SET game=$1,name=$2,slug=$3,description=$4,host=$5,port=$6,query_port=$7,transport=$8,rcon_host=$9,rcon_port=$10,enabled=$11,sort_order=$12,guest_checkout=$16,
         rcon_password = CASE WHEN $14 THEN $13 ELSE rcon_password END WHERE id=$15 RETURNING id"
    };
    let id: Uuid = sqlx::query_scalar(sql)
        .bind(&b.game)
        .bind(b.name.trim())
        .bind(&b.slug)
        .bind(&b.description)
        .bind(&b.host)
        .bind(b.port)
        .bind(b.query_port)
        .bind(&b.transport)
        .bind(&b.rcon_host)
        .bind(b.rcon_port)
        .bind(b.enabled)
        .bind(b.sort_order)
        .bind(rcon_password.clone().flatten())
        .bind(rcon_password.is_some())
        .bind(id)
        .bind(b.guest_checkout)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)?;
    server(pool, id).await
}

pub async fn delete_server(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM game_servers WHERE id = $1")
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

/// Sealed RCON password of a server.
pub async fn rcon_secret(pool: &PgPool, id: Uuid) -> AppResult<Option<Vec<u8>>> {
    sqlx::query_scalar::<_, Option<Vec<u8>>>("SELECT rcon_password FROM game_servers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
        .map(Option::flatten)
}

pub async fn set_agent_token(pool: &PgPool, id: Uuid, hash: &[u8]) -> AppResult<()> {
    let n = sqlx::query("UPDATE game_servers SET agent_token = $1 WHERE id = $2")
        .bind(hash)
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

/// Server authenticated by its agent token hash.
pub async fn server_for_agent(pool: &PgPool, hash: &[u8]) -> AppResult<Option<Server>> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {SERVER_COLS} FROM game_servers s WHERE s.agent_token = $1 AND s.enabled"
    )))
    .bind(hash)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Replace the packages (catalog products) sold on a server's store.
pub async fn set_products(pool: &PgPool, id: Uuid, products: &[Uuid]) -> AppResult<Server> {
    if products.len() > 500 {
        return Err(AppError::Validation("error.validation"));
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM game_server_products WHERE server_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (i, p) in products.iter().enumerate() {
        sqlx::query("INSERT INTO game_server_products (server_id, product_id, sort_order) VALUES ($1,$2,$3) ON CONFLICT DO NOTHING")
            .bind(id)
            .bind(p)
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    server(pool, id).await
}

/// Active packages of a server's store with their prices.
pub async fn packages(pool: &PgPool, server_id: Uuid) -> AppResult<Vec<Value>> {
    let rows: Vec<(Uuid, String, String, String, String, Uuid, Value)> = sqlx::query_as(
        "SELECT p.id, p.name, p.slug, p.description, p.image_url, p.group_id,
                COALESCE((SELECT jsonb_agg(jsonb_build_object('currency', pp.currency, 'cycle', pp.cycle, 'amount', pp.amount::text, 'setup_fee', pp.setup_fee::text) ORDER BY pp.currency, pp.cycle)
                          FROM product_prices pp WHERE pp.product_id = p.id), '[]'::jsonb)
         FROM game_server_products gp JOIN products p ON p.id = gp.product_id
         WHERE gp.server_id = $1 AND p.status = 'active'
         ORDER BY gp.sort_order, p.sort_order, p.name",
    )
    .bind(server_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, name, slug, description, image_url, group_id, prices)| {
            json!({ "id": id, "name": name, "slug": slug, "description": description, "image_url": image_url, "group_id": group_id, "prices": prices })
        })
        .collect())
}

/// Servers whose store sells a product.
pub async fn servers_for_product(pool: &PgPool, product: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("SELECT gp.server_id FROM game_server_products gp JOIN game_servers s ON s.id = gp.server_id WHERE gp.product_id = $1 AND s.enabled")
        .bind(product)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct GameAccount {
    pub id: Uuid,
    pub game: String,
    pub field: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct AccountInput {
    pub game: String,
    pub field: String,
    pub value: String,
}

pub async fn accounts(pool: &PgPool, customer: Uuid) -> AppResult<Vec<GameAccount>> {
    sqlx::query_as("SELECT id, game, field, value FROM customer_game_accounts WHERE customer_id = $1 ORDER BY game, field")
        .bind(customer)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn link_account(
    pool: &PgPool,
    customer: Uuid,
    a: &AccountInput,
) -> AppResult<GameAccount> {
    let g = game(&a.game).ok_or(AppError::Validation("error.validation"))?;
    if !g.fields.contains(&a.field.as_str()) {
        return Err(AppError::Validation("error.validation"));
    }
    let v = normalize_identity(&a.field, &a.value)
        .ok_or(AppError::Validation("error.game_identity_invalid"))?;
    sqlx::query_as(
        "INSERT INTO customer_game_accounts (customer_id, game, field, value) VALUES ($1,$2,$3,$4)
         ON CONFLICT (customer_id, game, field) DO UPDATE SET value = $4, updated_at = now()
         RETURNING id, game, field, value",
    )
    .bind(customer)
    .bind(&a.game)
    .bind(&a.field)
    .bind(&v)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn unlink_account(pool: &PgPool, customer: Uuid, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("DELETE FROM customer_game_accounts WHERE id = $1 AND customer_id = $2")
        .bind(id)
        .bind(customer)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// A customer's identity fields for one game (field → value).
pub async fn identity(
    pool: &PgPool,
    customer: Uuid,
    game_id: &str,
) -> AppResult<std::collections::BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT field, value FROM customer_game_accounts WHERE customer_id = $1 AND game = $2",
    )
    .bind(customer)
    .bind(game_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

/// Identity field a sign-in engine proves: Steam → SteamID64, Discord →
/// user id, Cfx.re (Login with FiveM) → `fivem:` account id. The Rockstar
/// `license:` never comes from a sign-in: it is asked at checkout.
pub fn sso_field(engine: &str) -> Option<&'static str> {
    match engine {
        "steam" => Some("steamid"),
        "discord" => Some("discord_id"),
        "cfx" => Some("fivem"),
        _ => None,
    }
}

/// Games with an enabled store whose identity has `field`.
pub async fn store_games_with(pool: &PgPool, field: &str) -> AppResult<Vec<&'static str>> {
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT game FROM game_servers WHERE enabled ORDER BY game")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(ids
        .iter()
        .filter_map(|g| game(g))
        .filter(|g| g.fields.contains(&field))
        .map(|g| g.id)
        .collect())
}

/// Set `field` on each of `games` where the customer has no value yet. An
/// existing (different) value is never replaced. Returns the games written.
pub async fn fill_missing(
    pool: &PgPool,
    customer: Uuid,
    games: &[&str],
    field: &str,
    value: &str,
) -> AppResult<Vec<String>> {
    let Some(v) = normalize_identity(field, value) else {
        return Ok(vec![]);
    };
    let mut out = vec![];
    for g in games {
        if !game(g).is_some_and(|d| d.fields.contains(&field)) {
            continue;
        }
        let n = sqlx::query(
            "INSERT INTO customer_game_accounts (customer_id, game, field, value) VALUES ($1,$2,$3,$4)
             ON CONFLICT (customer_id, game, field) DO NOTHING",
        )
        .bind(customer)
        .bind(g)
        .bind(field)
        .bind(&v)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
        if n > 0 {
            out.push((*g).to_string());
        }
    }
    Ok(out)
}

/// What a cart needs from the buyer, per game: the identity fields the
/// delivery commands of its packages use (`{license}`, `{steamid}`, ...).
/// Gifted lines are skipped (the recipient's name is used). Games come from
/// the enabled stores selling each package.
pub async fn cart_needs(
    pool: &PgPool,
    lines: &[(Uuid, bool)],
) -> AppResult<std::collections::BTreeMap<String, std::collections::BTreeSet<String>>> {
    use std::collections::{BTreeMap, BTreeSet};
    let ids: Vec<Uuid> = lines.iter().map(|(p, _)| *p).collect();
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    // (product, server, game)
    let stores: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT gp.product_id, s.id, s.game FROM game_server_products gp JOIN game_servers s ON s.id = gp.server_id
         WHERE gp.product_id = ANY($1) AND s.enabled",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if stores.is_empty() {
        return Ok(out);
    }
    // (product or global, server or all, template)
    let cmds: Vec<(Option<Uuid>, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT product_id, server_id, command FROM game_commands WHERE product_id = ANY($1) OR product_id IS NULL",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (pid, gift) in lines {
        for (_, sid, g) in stores.iter().filter(|(p, _, _)| p == pid) {
            let Some(def) = game(g) else { continue };
            let need = out.entry(g.clone()).or_default();
            if *gift {
                continue;
            }
            for (cp, cs, tpl) in &cmds {
                if cp.is_some_and(|c| c != *pid) || cs.is_some_and(|c| c != *sid) {
                    continue;
                }
                for ph in crate::game_delivery::placeholders(tpl) {
                    let f = if ph == "uuid_nodash" {
                        "uuid".to_string()
                    } else {
                        ph
                    };
                    if def.fields.contains(&f.as_str()) {
                        need.insert(f);
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sso_engines_map_to_fields() {
        assert_eq!(sso_field("steam"), Some("steamid"));
        assert_eq!(sso_field("discord"), Some("discord_id"));
        assert_eq!(sso_field("cfx"), Some("fivem"));
        assert_eq!(sso_field("google"), None);
        assert!(game("fivem").unwrap().fields.contains(&"fivem"));
        assert!(game("redm").unwrap().fields.contains(&"fivem"));
        assert!(!game("rust").unwrap().fields.contains(&"fivem"));
    }

    #[test]
    fn games_are_unique_and_named() {
        let mut seen = std::collections::HashSet::new();
        for g in GAMES {
            assert!(seen.insert(g.id));
            assert_eq!(g.fields[0], "username", "{}", g.id);
            assert!(matches!(
                g.query,
                "minecraft" | "bedrock" | "a2s" | "fivem" | "none"
            ));
        }
    }

    #[test]
    fn identities_are_validated() {
        assert_eq!(
            normalize_identity("username", " Notch ").as_deref(),
            Some("Notch")
        );
        for bad in ["", "a b", "x;op me", "name\nstop", "\"q\"", "a/b", "$(x)"] {
            assert!(normalize_identity("username", bad).is_none(), "{bad:?}");
        }
        assert_eq!(
            normalize_identity("uuid", "069A79F444E94726A5BEFCA90E38AAF5").as_deref(),
            Some("069a79f4-44e9-4726-a5be-fca90e38aaf5")
        );
        assert!(normalize_identity("steamid", "76561198000000000").is_some());
        assert!(normalize_identity("steamid", "12345678901234567").is_none());
        assert_eq!(
            normalize_identity(
                "license",
                "license:ABCDEF0123456789abcdef0123456789abcdef01"
            )
            .as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef01")
        );
        assert!(normalize_identity("discord_id", "123456789012345678").is_some());
        assert!(normalize_identity("discord_id", "12").is_none());
        assert!(normalize_identity("unknown", "x").is_none());
        assert_eq!(
            normalize_identity("fivem", "fivem:123456").as_deref(),
            Some("123456")
        );
        assert_eq!(normalize_identity("fivem", " 42 ").as_deref(), Some("42"));
        for bad in ["", "fivem:", "abc", "fivem:12a", "1234567890123"] {
            assert!(normalize_identity("fivem", bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn hosts_reject_urls() {
        assert!(host_ok("play.example.com"));
        assert!(host_ok("10.0.0.1"));
        assert!(!host_ok("http://x"));
        assert!(!host_ok("a b"));
        assert!(!host_ok("user@host"));
    }
}
