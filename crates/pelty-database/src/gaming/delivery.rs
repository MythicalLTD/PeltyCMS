//! In-game delivery queue (module `game_delivery`), Tebex/CraftingStore
//! style: commands attached to a package (or global) are rendered with the
//! buyer's linked game identity and queued per game server, then sent over
//! RCON by the worker or pulled by the server's agent plugin.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::BTreeMap;
use uuid::Uuid;

pub const TRIGGERS: [&str; 5] = ["purchase", "renewal", "expiry", "refund", "chargeback"];

/// Placeholders a command may use. Identity placeholders put the delivery
/// on hold (`waiting_identity`) until the customer links that field.
pub const PLACEHOLDERS: [&str; 16] = [
    "username",
    "uuid",
    "uuid_nodash",
    "xuid",
    "steamid",
    "license",
    "discord_id",
    "fivem",
    "package",
    "package_id",
    "service_id",
    "order_id",
    "customer_id",
    "amount",
    "currency",
    "server",
];
pub const IDENTITY: [&str; 8] = [
    "username",
    "uuid",
    "uuid_nodash",
    "xuid",
    "steamid",
    "license",
    "discord_id",
    "fivem",
];
/// Attempts before a delivery is marked failed.
pub const MAX_ATTEMPTS: i32 = 10;

#[derive(Debug, Clone, Serialize, Deserialize, utoipa::ToSchema)]
pub struct CommandInput {
    /// Only this server; omit for every server whose store sells the package.
    #[serde(default)]
    pub server_id: Option<Uuid>,
    pub trigger: String,
    pub command: String,
    #[serde(default)]
    pub require_online: bool,
    #[serde(default)]
    pub delay_secs: i32,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Command {
    pub id: Uuid,
    pub product_id: Option<Uuid>,
    pub server_id: Option<Uuid>,
    pub trigger: String,
    pub command: String,
    pub require_online: bool,
    pub delay_secs: i32,
}

/// Placeholder names used by a template, in order of appearance.
pub fn placeholders(tpl: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = tpl;
    while let Some(i) = rest.find('{') {
        rest = &rest[i + 1..];
        if let Some(j) = rest.find('}') {
            let name = &rest[..j];
            if name.starts_with(|c: char| c.is_ascii_lowercase())
                && name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                out.push(name.to_string());
            }
            rest = &rest[j + 1..];
        }
    }
    out
}

impl CommandInput {
    pub fn validate(&self) -> AppResult<()> {
        self.validate_with(&[])
    }

    /// Like [`validate`](Self::validate), also accepting the package's
    /// custom-field keys as placeholders.
    pub fn validate_with(&self, field_keys: &[String]) -> AppResult<()> {
        let c = self.command.trim();
        if c.is_empty()
            || c.len() > 1000
            || c.contains(['\r', '\n', '\0'])
            || !TRIGGERS.contains(&self.trigger.as_str())
            || !(0..=2_592_000).contains(&self.delay_secs)
        {
            return Err(AppError::Validation("error.validation"));
        }
        if placeholders(c)
            .iter()
            .any(|p| !PLACEHOLDERS.contains(&p.as_str()) && !field_keys.contains(p))
        {
            return Err(AppError::Validation("error.game_placeholder_unknown"));
        }
        Ok(())
    }
}

/// Fill placeholders. `Err(field)` when an identity field is missing.
pub fn render(tpl: &str, vars: &BTreeMap<String, String>) -> Result<String, String> {
    let mut out = tpl.trim().to_string();
    for p in placeholders(tpl) {
        let v = match vars.get(&p) {
            Some(v) => v.clone(),
            None if IDENTITY.contains(&p.as_str()) => return Err(p),
            None => String::new(),
        };
        // Admin-controlled values (package names) must not add lines.
        let v: String = v.chars().filter(|c| !c.is_control()).collect();
        out = out.replace(&format!("{{{p}}}"), &v);
    }
    Ok(out)
}

/// Add the service's custom-field values (`{key}`) to `vars` without
/// replacing identity or built-in values. Password fields stay as their
/// `{key}` placeholder in the stored command and are filled only when the
/// command is handed to the server ([`fill_secrets`]).
pub async fn add_field_vars(
    pool: &PgPool,
    service: Uuid,
    vars: &mut BTreeMap<String, String>,
) -> AppResult<()> {
    let (vals, secret) = crate::field_kit::command_values(pool, service).await?;
    for (k, v) in vals {
        vars.entry(k).or_insert(v);
    }
    for k in secret {
        vars.insert(k.clone(), format!("{{{k}}}"));
    }
    Ok(())
}

/// Fill password placeholders of claimed commands from the sealed service
/// fields. The stored command (admin log) keeps `{key}`.
async fn fill_secrets(pool: &PgPool, mut rows: Vec<Claimed>) -> AppResult<Vec<Claimed>> {
    for r in rows.iter_mut() {
        let Some(sid) = r.service_id.filter(|_| r.command.contains('{')) else {
            continue;
        };
        for (k, v) in crate::field_kit::secret_values(pool, sid).await? {
            let v: String = v.chars().filter(|c| !c.is_control()).collect();
            r.command = r.command.replace(&format!("{{{k}}}"), &v);
        }
    }
    Ok(rows)
}

/// Identity variables from linked accounts (`uuid_nodash` derived).
pub fn identity_vars(id: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    let mut v = id.clone();
    if let Some(u) = id.get("uuid") {
        v.insert("uuid_nodash".into(), u.replace('-', ""));
    }
    v
}

pub async fn commands(pool: &PgPool, product: Option<Uuid>) -> AppResult<Vec<Command>> {
    sqlx::query_as("SELECT id, product_id, server_id, trigger, command, require_online, delay_secs FROM game_commands WHERE product_id IS NOT DISTINCT FROM $1 ORDER BY sort_order, id")
        .bind(product)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Replace the commands of a package (`None`: global commands).
pub async fn replace_commands(
    pool: &PgPool,
    product: Option<Uuid>,
    list: &[CommandInput],
) -> AppResult<Vec<Command>> {
    if list.len() > 100 {
        return Err(AppError::Validation("error.validation"));
    }
    let keys: Vec<String> = match product {
        Some(p) => sqlx::query_scalar(
            "SELECT COALESCE(module_key, field_key) FROM product_custom_fields WHERE product_id = $1 AND COALESCE(module_key, field_key) IS NOT NULL",
        )
        .bind(p)
        .fetch_all(pool)
        .await
        .map_err(db_err)?,
        None => vec![],
    };
    for c in list {
        c.validate_with(&keys)?;
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM game_commands WHERE product_id IS NOT DISTINCT FROM $1")
        .bind(product)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    for (i, c) in list.iter().enumerate() {
        sqlx::query("INSERT INTO game_commands (product_id, server_id, trigger, command, require_online, delay_secs, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(product)
            .bind(c.server_id)
            .bind(&c.trigger)
            .bind(c.command.trim())
            .bind(c.require_online)
            .bind(c.delay_secs)
            .bind(i as i32)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    commands(pool, product).await
}

/// What a delivery is about.
#[derive(Debug, Clone)]
pub struct ServiceCtx {
    pub service_id: Uuid,
    pub customer_id: Uuid,
    pub product_id: Uuid,
    pub package: String,
    pub amount: String,
    pub currency: String,
    pub order_id: Option<Uuid>,
    /// In-game name of the gift recipient (gift purchase); commands go to
    /// that player instead of the buyer's linked game account.
    pub gift_to: Option<String>,
}

pub async fn service_ctx(pool: &PgPool, service: Uuid) -> AppResult<Option<ServiceCtx>> {
    type Row = (
        Uuid,
        Uuid,
        String,
        String,
        String,
        Option<Uuid>,
        Option<String>,
    );
    let r: Option<Row> = sqlx::query_as(
        "SELECT s.customer_id, s.product_id, p.name, s.recurring_amount::text, s.currency::text, oi.order_id, oi.gift_to
         FROM services s JOIN products p ON p.id = s.product_id
         LEFT JOIN order_items oi ON oi.id = s.order_item_id WHERE s.id = $1",
    )
    .bind(service)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(r.map(
        |(customer_id, product_id, package, amount, currency, order_id, gift_to)| ServiceCtx {
            service_id: service,
            customer_id,
            product_id,
            package,
            amount,
            currency,
            order_id,
            gift_to,
        },
    ))
}

/// Services on an order / invoice / payment (for refunds and chargebacks).
pub async fn services_for(pool: &PgPool, kind: &str, id: Uuid) -> AppResult<Vec<Uuid>> {
    let sql = match kind {
        "order" => "SELECT s.id FROM services s JOIN order_items oi ON oi.id = s.order_item_id WHERE oi.order_id = $1",
        "invoice" => "SELECT DISTINCT ii.service_id FROM invoice_items ii JOIN services s ON s.id = ii.service_id WHERE ii.invoice_id = $1
                      UNION SELECT s.id FROM invoices i JOIN order_items oi ON oi.order_id = i.order_id JOIN services s ON s.order_item_id = oi.id WHERE i.id = $1",
        "payment" => "SELECT DISTINCT ii.service_id FROM payments pm JOIN invoice_items ii ON ii.invoice_id = pm.invoice_id JOIN services s ON s.id = ii.service_id WHERE pm.id = $1
                      UNION SELECT s.id FROM payments pm JOIN invoices i ON i.id = pm.invoice_id JOIN order_items oi ON oi.order_id = i.order_id JOIN services s ON s.order_item_id = oi.id WHERE pm.id = $1",
        _ => return Ok(vec![]),
    };
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Renewal invoices: items of already-existing services on a paid invoice.
pub async fn renewed_services(pool: &PgPool, invoice: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar(
        "SELECT DISTINCT s.id FROM invoice_items ii JOIN invoices i ON i.id = ii.invoice_id
         JOIN services s ON s.id = ii.service_id
         WHERE ii.invoice_id = $1 AND s.created_at < i.created_at",
    )
    .bind(invoice)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Queue the commands of `trigger` for a service. `occurrence` makes the
/// dedupe key unique per event (e.g. the renewal invoice id).
pub async fn enqueue(
    pool: &PgPool,
    ctx: &ServiceCtx,
    trigger: &str,
    occurrence: &str,
) -> AppResult<usize> {
    let cmds: Vec<Command> = sqlx::query_as(
        "SELECT id, product_id, server_id, trigger, command, require_online, delay_secs FROM game_commands
         WHERE (product_id = $1 OR product_id IS NULL) AND trigger = $2 ORDER BY product_id NULLS LAST, sort_order",
    )
    .bind(ctx.product_id)
    .bind(trigger)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    if cmds.is_empty() {
        return Ok(0);
    }
    let selling = crate::games::servers_for_product(pool, ctx.product_id).await?;
    let servers: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, game, name FROM game_servers WHERE enabled AND transport <> 'none'",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut n = 0;
    for c in &cmds {
        let targets: Vec<&(Uuid, String, String)> = servers
            .iter()
            .filter(|s| match c.server_id {
                Some(only) => s.0 == only,
                None => selling.contains(&s.0),
            })
            .collect();
        for (sid, game, sname) in targets {
            // Gift: only the recipient's name is known (no linked account),
            // so `{username}`/`{player}` are filled and nothing else.
            let gift = ctx.gift_to.as_deref().map(|g| vote_player_arg(game, g));
            let mut vars = match &gift {
                Some(Some(name)) => BTreeMap::from([
                    ("username".to_string(), name.clone()),
                    ("player".to_string(), name.clone()),
                ]),
                _ => identity_vars(&crate::games::identity(pool, ctx.customer_id, game).await?),
            };
            if gift.is_none() {
                add_field_vars(pool, ctx.service_id, &mut vars).await?;
            }
            for (k, v) in [
                ("package", ctx.package.clone()),
                ("package_id", ctx.product_id.to_string()),
                ("service_id", ctx.service_id.to_string()),
                (
                    "order_id",
                    ctx.order_id.map(|o| o.to_string()).unwrap_or_default(),
                ),
                ("customer_id", ctx.customer_id.to_string()),
                ("amount", ctx.amount.clone()),
                ("currency", ctx.currency.clone()),
                ("server", sname.clone()),
            ] {
                vars.insert(k.into(), v);
            }
            let (status, command, err) = match (render(&c.command, &vars), &gift) {
                (_, Some(None)) => (
                    "failed",
                    String::new(),
                    "gift: name not valid for this game".to_string(),
                ),
                (Ok(cmd), _) => ("pending", cmd, String::new()),
                // A gift never waits for the buyer to link an account: the
                // command would then go to the buyer, not the recipient.
                (Err(field), Some(Some(_))) => {
                    ("failed", String::new(), format!("gift: missing {field}"))
                }
                (Err(field), None) => (
                    "waiting_identity",
                    String::new(),
                    format!("missing {field}"),
                ),
            };
            let player = vars.get("username").cloned().unwrap_or_default();
            let key = format!(
                "{}:{}:{}:{}:{}",
                c.id, ctx.service_id, sid, trigger, occurrence
            );
            let r = sqlx::query(
                "INSERT INTO game_deliveries (server_id, command_id, customer_id, service_id, product_id, trigger, dedupe_key, template, command, player, require_online, status, last_error, next_attempt_at)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13, now() + make_interval(secs => $14))
                 ON CONFLICT (dedupe_key) DO NOTHING",
            )
            .bind(sid)
            .bind(c.id)
            .bind(ctx.customer_id)
            .bind(ctx.service_id)
            .bind(ctx.product_id)
            .bind(trigger)
            .bind(&key)
            .bind(&c.command)
            .bind(&command)
            .bind(&player)
            .bind(c.require_online)
            .bind(status)
            .bind(&err)
            .bind(c.delay_secs as f64)
            .execute(pool)
            .await
            .map_err(db_err)?;
            n += r.rows_affected() as usize;
        }
    }
    Ok(n)
}

/// A vote reward to queue (module `server_community`).
pub struct VoteReward<'a> {
    pub vote_id: Uuid,
    pub site: &'a str,
    pub username: &'a str,
    pub customer: Option<Uuid>,
    pub template: &'a str,
    /// One server, or every enabled server that can receive commands.
    pub server: Option<Uuid>,
    pub require_online: bool,
}

/// Queue a vote reward on the target servers. Returns how many were queued;
/// servers whose command needs an identity the voter has not linked are skipped.
pub async fn enqueue_vote(pool: &PgPool, r: &VoteReward<'_>) -> AppResult<usize> {
    if r.template.trim().is_empty() {
        return Ok(0);
    }
    let servers: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, game, name FROM game_servers WHERE enabled AND transport <> 'none' AND ($1::uuid IS NULL OR id = $1)",
    )
    .bind(r.server)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut n = 0;
    for (sid, game, sname) in servers {
        // The name comes from an outside vote site: never let it add command
        // arguments. Bedrock gamertags may contain spaces and are quoted.
        let Some(player) = vote_player_arg(&game, r.username) else {
            continue;
        };
        let mut vars = match r.customer {
            Some(c) => identity_vars(&crate::games::identity(pool, c, &game).await?),
            None => BTreeMap::new(),
        };
        vars.insert("username".into(), player.clone());
        vars.insert("player".into(), player);
        vars.insert("site".into(), r.site.to_string());
        vars.insert("server".into(), sname);
        if let Some(c) = r.customer {
            vars.insert("customer_id".into(), c.to_string());
        }
        let Ok(command) = render(r.template, &vars) else {
            continue;
        };
        let q = sqlx::query(
            "INSERT INTO game_deliveries (server_id, customer_id, trigger, dedupe_key, template, command, player, require_online, status, next_attempt_at)
             VALUES ($1,$2,'vote',$3,$4,$5,$6,$7,'pending', now()) ON CONFLICT (dedupe_key) DO NOTHING",
        )
        .bind(sid)
        .bind(r.customer)
        .bind(format!("vote:{}:{sid}", r.vote_id))
        .bind(r.template)
        .bind(&command)
        .bind(r.username)
        .bind(r.require_online)
        .execute(pool)
        .await
        .map_err(db_err)?;
        n += q.rows_affected() as usize;
    }
    Ok(n)
}

/// A vote name as one command argument for `game`: as is without whitespace;
/// quoted on Bedrock (gamertags may contain spaces); otherwise rejected.
/// Callers only pass names that passed `community::username_ok`, so the name
/// holds no quotes or control characters.
pub fn vote_player_arg(game: &str, name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.contains(['"', '\\']) || name.chars().any(char::is_control) {
        return None;
    }
    if !name.chars().any(char::is_whitespace) {
        return Some(name.to_string());
    }
    (game == "minecraft_bedrock").then(|| {
        format!(
            "\"{}\"",
            name.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    })
}

/// (delivery, template, game, server name, service) of a waiting delivery.
type WaitingRow = (Uuid, String, String, String, Option<Uuid>);

/// Re-render deliveries waiting for a game identity of this customer.
pub async fn release_waiting(pool: &PgPool, customer: Uuid) -> AppResult<usize> {
    let rows: Vec<WaitingRow> = sqlx::query_as(
        "SELECT d.id, d.template, s.game, s.name, d.service_id FROM game_deliveries d JOIN game_servers s ON s.id = d.server_id
         WHERE d.customer_id = $1 AND d.status = 'waiting_identity'",
    )
    .bind(customer)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut n = 0;
    for (id, tpl, game, server_name, service) in rows {
        let mut vars = identity_vars(&crate::games::identity(pool, customer, &game).await?);
        if let Some(ctx) = match service {
            Some(s) => service_ctx(pool, s).await?,
            None => None,
        } {
            vars.insert("package".into(), ctx.package);
            vars.insert("package_id".into(), ctx.product_id.to_string());
            vars.insert("service_id".into(), ctx.service_id.to_string());
            vars.insert(
                "order_id".into(),
                ctx.order_id.map(|o| o.to_string()).unwrap_or_default(),
            );
            vars.insert("amount".into(), ctx.amount);
            vars.insert("currency".into(), ctx.currency);
            add_field_vars(pool, ctx.service_id, &mut vars).await?;
        }
        vars.insert("customer_id".into(), customer.to_string());
        vars.insert("server".into(), server_name);
        if let Ok(cmd) = render(&tpl, &vars) {
            sqlx::query("UPDATE game_deliveries SET status='pending', command=$2, player=$3, last_error='', next_attempt_at=now() WHERE id=$1 AND status='waiting_identity'")
                .bind(id)
                .bind(&cmd)
                .bind(vars.get("username").cloned().unwrap_or_default())
                .execute(pool)
                .await
                .map_err(db_err)?;
            n += 1;
        }
    }
    Ok(n)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
pub struct Claimed {
    pub id: Uuid,
    pub server_id: Uuid,
    pub command: String,
    pub player: String,
    pub require_online: bool,
    pub attempts: i32,
    /// Used to fill password placeholders; not sent to the server.
    #[serde(skip_serializing)]
    pub service_id: Option<Uuid>,
}

/// Claim due RCON deliveries (lease: 2 minutes). Safe across instances.
pub async fn claim_rcon(pool: &PgPool, limit: i64) -> AppResult<Vec<Claimed>> {
    let rows = sqlx::query_as(
        "UPDATE game_deliveries d SET attempts = attempts + 1, next_attempt_at = now() + interval '2 minutes'
         WHERE d.id IN (SELECT d2.id FROM game_deliveries d2 JOIN game_servers s ON s.id = d2.server_id
                        WHERE d2.status = 'pending' AND d2.next_attempt_at <= now() AND s.transport = 'rcon' AND s.enabled
                          AND d2.attempts < $2
                        ORDER BY d2.next_attempt_at LIMIT $1 FOR UPDATE OF d2 SKIP LOCKED)
         RETURNING d.id, d.server_id, d.command, d.player, d.require_online, d.attempts, d.service_id",
    )
    .bind(limit)
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    fill_secrets(pool, rows).await
}

/// Mark leased deliveries that used every attempt without an answer (e.g.
/// an agent that never acknowledged) as failed once their lease ran out.
pub async fn sweep_exhausted(pool: &PgPool) -> AppResult<u64> {
    Ok(sqlx::query(
        "UPDATE game_deliveries SET status = 'failed',
                last_error = CASE WHEN last_error = '' THEN 'no acknowledgement' ELSE last_error END
          WHERE status = 'pending' AND attempts >= $1 AND next_attempt_at <= now()",
    )
    .bind(MAX_ATTEMPTS)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected())
}

/// Put a claimed but unsent delivery back without charging the attempt.
pub async fn release(pool: &PgPool, id: Uuid, delay_secs: i64, note: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE game_deliveries SET attempts = GREATEST(attempts - 1, 0), last_error = $3,
                next_attempt_at = now() + make_interval(secs => $2)
          WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .bind(delay_secs as f64)
    .bind(note)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Claim due deliveries for an agent. With `online`, commands that need the
/// player online are included only for those players; without it they wait.
pub async fn claim_agent(
    pool: &PgPool,
    server: Uuid,
    online: Option<&[String]>,
    limit: i64,
) -> AppResult<Vec<Claimed>> {
    let online: Vec<String> = online
        .unwrap_or_default()
        .iter()
        .map(|s| s.to_lowercase())
        .collect();
    let rows = sqlx::query_as(
        "UPDATE game_deliveries d SET attempts = attempts + 1, next_attempt_at = now() + interval '2 minutes'
         WHERE d.id IN (SELECT d2.id FROM game_deliveries d2
                        WHERE d2.server_id = $1 AND d2.status = 'pending' AND d2.next_attempt_at <= now()
                          AND d2.attempts < $4
                          AND (NOT d2.require_online OR lower(d2.player) = ANY($2)
                               OR EXISTS (SELECT 1 FROM customer_game_accounts a WHERE a.customer_id = d2.customer_id AND lower(a.value) = ANY($2)))
                        ORDER BY d2.next_attempt_at LIMIT $3 FOR UPDATE SKIP LOCKED)
         RETURNING d.id, d.server_id, d.command, d.player, d.require_online, d.attempts, d.service_id",
    )
    .bind(server)
    .bind(&online)
    .bind(limit)
    .bind(MAX_ATTEMPTS)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    fill_secrets(pool, rows).await
}

pub async fn mark_delivered(
    pool: &PgPool,
    id: Uuid,
    server: Option<Uuid>,
    response: &str,
) -> AppResult<bool> {
    let r: String = response.chars().take(2000).collect();
    let n = sqlx::query("UPDATE game_deliveries SET status='delivered', delivered_at=now(), response=$2, last_error='' WHERE id=$1 AND status='pending' AND ($3::uuid IS NULL OR server_id=$3)")
        .bind(id)
        .bind(r)
        .bind(server)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    Ok(n > 0)
}

/// Backoff `min(2^attempts, 60)` minutes; failed after [`MAX_ATTEMPTS`].
pub async fn mark_retry(
    pool: &PgPool,
    id: Uuid,
    server: Option<Uuid>,
    error: &str,
) -> AppResult<bool> {
    let e: String = error.chars().take(1000).collect();
    let n = sqlx::query(
        "UPDATE game_deliveries SET last_error=$2,
            status = CASE WHEN attempts >= $4 THEN 'failed' ELSE 'pending' END,
            next_attempt_at = now() + make_interval(mins => LEAST(power(2, attempts)::int, 60))
         WHERE id=$1 AND status='pending' AND ($3::uuid IS NULL OR server_id=$3)",
    )
    .bind(id)
    .bind(e)
    .bind(server)
    .bind(MAX_ATTEMPTS)
    .execute(pool)
    .await
    .map_err(db_err)?
    .rows_affected();
    Ok(n > 0)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, utoipa::ToSchema)]
#[schema(as = GameDelivery)]
pub struct Delivery {
    pub id: Uuid,
    pub server_id: Uuid,
    pub server_name: String,
    pub customer_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
    pub product_id: Option<Uuid>,
    pub package: Option<String>,
    pub trigger: String,
    pub command: String,
    pub player: String,
    pub require_online: bool,
    pub status: String,
    pub attempts: i32,
    pub last_error: String,
    pub response: String,
    #[serde(with = "time::serde::rfc3339")]
    pub next_attempt_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339::option")]
    pub delivered_at: Option<time::OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
pub struct Filter {
    pub status: Option<String>,
    pub server_id: Option<Uuid>,
    pub customer_id: Option<Uuid>,
    pub service_id: Option<Uuid>,
}

pub async fn list(pool: &PgPool, f: &Filter, limit: i64) -> AppResult<Vec<Delivery>> {
    sqlx::query_as(
        "SELECT d.id, d.server_id, s.name AS server_name, d.customer_id, d.service_id, d.product_id, p.name AS package, d.trigger, d.command, d.player,
                d.require_online, d.status, d.attempts, d.last_error, d.response, d.next_attempt_at, d.delivered_at, d.created_at
         FROM game_deliveries d JOIN game_servers s ON s.id = d.server_id LEFT JOIN products p ON p.id = d.product_id
         WHERE ($1::text IS NULL OR d.status = $1) AND ($2::uuid IS NULL OR d.server_id = $2)
           AND ($3::uuid IS NULL OR d.customer_id = $3) AND ($4::uuid IS NULL OR d.service_id = $4)
         ORDER BY d.created_at DESC LIMIT $5",
    )
    .bind(&f.status)
    .bind(f.server_id)
    .bind(f.customer_id)
    .bind(f.service_id)
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Put a failed/cancelled delivery back in the queue now.
pub async fn retry(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("UPDATE game_deliveries SET status='pending', attempts=0, next_attempt_at=now(), last_error='' WHERE id=$1 AND status IN ('failed','cancelled','pending')")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("error.game_delivery_state"));
    }
    Ok(())
}

pub async fn cancel(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let n = sqlx::query("UPDATE game_deliveries SET status='cancelled' WHERE id=$1 AND status IN ('pending','waiting_identity','failed')")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?
        .rows_affected();
    if n == 0 {
        return Err(AppError::Conflict("error.game_delivery_state"));
    }
    Ok(())
}

/// Counts per status (dashboard).
pub async fn stats(pool: &PgPool) -> AppResult<Value> {
    let rows: Vec<(String, i64)> =
        sqlx::query_as("SELECT status, count(*) FROM game_deliveries GROUP BY status")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    Ok(Value::Object(
        rows.into_iter().map(|(k, v)| (k, Value::from(v))).collect(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn vote_names_stay_one_argument() {
        assert_eq!(
            vote_player_arg("minecraft_java", "Steve").as_deref(),
            Some("Steve")
        );
        assert_eq!(
            vote_player_arg("minecraft_java", "Bob parent add admin"),
            None
        );
        assert_eq!(vote_player_arg("rust", "a b"), None);
        assert_eq!(
            vote_player_arg("minecraft_bedrock", "Cool  Gamer").as_deref(),
            Some("\"Cool Gamer\"")
        );
        assert_eq!(vote_player_arg("minecraft_bedrock", "x\" op"), None);
    }

    #[test]
    fn renders_placeholders() {
        let v = vars(&[("username", "Steve"), ("package", "VIP\nop all")]);
        assert_eq!(
            render("lp user {username} parent add vip", &v).unwrap(),
            "lp user Steve parent add vip"
        );
        assert_eq!(render("say {package}", &v).unwrap(), "say VIPop all");
        assert_eq!(render("give {steamid} 1", &v), Err("steamid".into()));
        assert_eq!(render("say {amount}!", &v).unwrap(), "say !");
        assert_eq!(placeholders("a {x} {y_z} {} {Bad}"), vec!["x", "y_z"]);
        assert_eq!(placeholders("{rank_2} {2x}"), vec!["rank_2"]);
        // A password placeholder survives rendering (filled at send time).
        let v = vars(&[("account_password", "{account_password}"), ("rank", "vip")]);
        assert_eq!(
            render("create {rank} {account_password}", &v).unwrap(),
            "create vip {account_password}"
        );
        let c = CommandInput {
            server_id: None,
            trigger: "purchase".into(),
            command: "say {rank}".into(),
            require_online: false,
            delay_secs: 0,
        };
        assert!(c.validate().is_err());
        assert!(c.validate_with(&["rank".into()]).is_ok());
    }

    #[test]
    fn validates_commands() {
        let ok = CommandInput {
            server_id: None,
            trigger: "purchase".into(),
            command: "give {username} diamond 1".into(),
            require_online: true,
            delay_secs: 0,
        };
        assert!(ok.validate().is_ok());
        for (trigger, command, delay) in [
            ("buy", "x", 0),
            ("purchase", "a\nb", 0),
            ("purchase", " ", 0),
            ("purchase", "x {nope}", 0),
            ("purchase", "x", -1),
        ] {
            let c = CommandInput {
                trigger: trigger.into(),
                command: command.into(),
                delay_secs: delay,
                ..ok.clone()
            };
            assert!(c.validate().is_err(), "{command}");
        }
    }

    #[test]
    fn identity_vars_derive_nodash() {
        let v = identity_vars(&vars(&[("uuid", "069a79f4-44e9-4726-a5be-fca90e38aaf5")]));
        assert_eq!(v["uuid_nodash"], "069a79f444e94726a5befca90e38aaf5");
    }
}
