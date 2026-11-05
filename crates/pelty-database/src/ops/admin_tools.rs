//! Admin tools: global search, system health, My Notes, notification rules.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

pub const PROVIDERS: [&str; 7] = [
    "slack",
    "discord",
    "teams",
    "webhook",
    "telegram",
    "sms_webhook",
    "sms_email",
];
pub const EVENTS: [&str; 11] = [
    "order.created",
    "invoice.paid",
    "invoice.overdue",
    "ticket.opened",
    "ticket.customer_reply",
    "service.suspended",
    "service.provision_failed",
    "customer.registered",
    "domain.expiring",
    "cancellation.requested",
    "chat.started",
];

async fn arr(pool: &PgPool, sql: &str, q: &str, limit: i64) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(q)
        .bind(limit)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok(v.unwrap_or_else(|| json!([])))
}

/// Case-insensitive search across the main admin entities (max `limit` per group).
pub async fn search(pool: &PgPool, q: &str, limit: i64) -> AppResult<Value> {
    let q = q.trim();
    if q.chars().count() < 2 || q.len() > 100 {
        return Err(AppError::Validation("error.search_query"));
    }
    let like = format!(
        "%{}%",
        q.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let customers = arr(pool, "SELECT json_agg(r) FROM (SELECT id, trim(first_name||' '||last_name) AS name, organization, email::text AS email, status FROM customers
        WHERE email::text ILIKE $1 OR first_name||' '||last_name ILIKE $1 OR organization ILIKE $1 OR phone ILIKE $1 OR id::text = $1
        ORDER BY created_at DESC LIMIT $2) r", &like, limit).await?;
    let invoices = arr(pool, "SELECT json_agg(r) FROM (SELECT i.id, i.number, i.status, i.total::text AS total, i.currency, c.email::text AS customer FROM invoices i JOIN customers c ON c.id=i.customer_id
        WHERE i.number ILIKE $1 OR i.id::text ILIKE $1 ORDER BY i.created_at DESC LIMIT $2) r", &like, limit).await?;
    let services = arr(pool, "SELECT json_agg(r) FROM (SELECT s.id, p.name AS product, s.status, c.email::text AS customer FROM services s JOIN products p ON p.id=s.product_id JOIN customers c ON c.id=s.customer_id
        WHERE p.name ILIKE $1 OR s.id::text ILIKE $1 OR c.email::text ILIKE $1 ORDER BY s.created_at DESC LIMIT $2) r", &like, limit).await?;
    let tickets = arr(pool, "SELECT json_agg(r) FROM (SELECT t.id, t.number, t.subject, t.status FROM tickets t
        WHERE t.subject ILIKE $1 OR t.number::text ILIKE $1 ORDER BY t.last_reply_at DESC LIMIT $2) r", &like, limit).await?;
    let domains = arr(
        pool,
        "SELECT json_agg(r) FROM (SELECT d.id, d.name, d.status, d.expires_on FROM domains d
        WHERE d.name ILIKE $1 ORDER BY d.created_at DESC LIMIT $2) r",
        &like,
        limit,
    )
    .await?;
    let orders = arr(pool, "SELECT json_agg(r) FROM (SELECT o.id, o.number, o.status, o.total::text AS total, o.currency FROM orders o JOIN customers c ON c.id=o.customer_id
        WHERE o.number::text ILIKE $1 OR c.email::text ILIKE $1 ORDER BY o.created_at DESC LIMIT $2) r", &like, limit).await?;
    Ok(
        json!({ "customers": customers, "invoices": invoices, "services": services, "tickets": tickets, "domains": domains, "orders": orders }),
    )
}

async fn arr_c(pool: &PgPool, sql: &str, q: &str, cid: uuid::Uuid, limit: i64) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .bind(q)
        .bind(limit)
        .bind(cid)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    Ok(v.unwrap_or_else(|| json!([])))
}

/// Maps a bare record-type word (en/ro, singular/plural) to its result group.
fn keyword_group(term: &str) -> Option<&'static str> {
    let t = term.to_lowercase();
    let is = |words: &[&str]| words.iter().any(|w| t == *w);
    if is(&[
        "ticket", "tickets", "tichet", "tichete", "support", "suport",
    ]) {
        Some("tickets")
    } else if is(&["invoice", "invoices", "factura", "factură", "facturi"]) {
        Some("invoices")
    } else if is(&["service", "services", "serviciu", "servicii"]) {
        Some("services")
    } else if is(&["domain", "domains", "domeniu", "domenii"]) {
        Some("domains")
    } else {
        None
    }
}

/// Client-area search, strictly scoped to one customer's own records (KB is public).
pub async fn client_search(
    pool: &PgPool,
    cid: uuid::Uuid,
    q: &str,
    limit: i64,
) -> AppResult<Value> {
    let q = q.trim();
    if q.chars().count() < 2 || q.len() > 100 {
        return Err(AppError::Validation("error.search_query"));
    }
    // "#1001" searches by number; a bare type word ("ticket", "facturi") lists recent records of that type.
    let term = q.trim_start_matches('#').trim();
    if term.is_empty() {
        return Err(AppError::Validation("error.search_query"));
    }
    let like = format!(
        "%{}%",
        term.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    let kw = keyword_group(term);
    let pick = |g: &str| if kw == Some(g) { "%" } else { like.as_str() };
    let invoices = arr_c(pool, "SELECT json_agg(r) FROM (SELECT i.id, i.number, i.status, i.total::text AS total, i.currency FROM invoices i
        WHERE i.customer_id = $3 AND i.status <> 'draft' AND (i.number ILIKE $1 OR i.id::text ILIKE $1) ORDER BY i.created_at DESC LIMIT $2) r", pick("invoices"), cid, limit).await?;
    let services = arr_c(pool, "SELECT json_agg(r) FROM (SELECT s.id, p.name AS product, s.status FROM services s JOIN products p ON p.id=s.product_id
        WHERE s.customer_id = $3 AND (p.name ILIKE $1 OR s.id::text ILIKE $1) ORDER BY s.created_at DESC LIMIT $2) r", pick("services"), cid, limit).await?;
    let tickets = arr_c(pool, "SELECT json_agg(r) FROM (SELECT t.id, t.number, t.subject, t.status FROM tickets t
        WHERE t.customer_id = $3 AND (t.subject ILIKE $1 OR t.number::text ILIKE $1) ORDER BY t.last_reply_at DESC LIMIT $2) r", pick("tickets"), cid, limit).await?;
    let domains = arr_c(
        pool,
        "SELECT json_agg(r) FROM (SELECT d.id, d.name, d.status FROM domains d
        WHERE d.customer_id = $3 AND d.name ILIKE $1 ORDER BY d.created_at DESC LIMIT $2) r",
        pick("domains"),
        cid,
        limit,
    )
    .await?;
    let kb = arr(pool, "SELECT json_agg(r) FROM (SELECT a.id, a.slug, a.title FROM kb_articles a
        WHERE a.is_published AND (a.title ILIKE $1 OR a.body ILIKE $1) ORDER BY a.views DESC LIMIT $2) r", &like, limit).await?;
    Ok(
        json!({ "invoices": invoices, "services": services, "tickets": tickets, "domains": domains, "kb": kb }),
    )
}

/// Database-backed health checks (WHMCS "System Health").
pub async fn health(pool: &PgPool) -> AppResult<Value> {
    let row: Value = sqlx::query_scalar(
        "SELECT json_build_object(
            'db_version', current_setting('server_version'),
            'db_size_bytes', pg_database_size(current_database()),
            'migrations', (SELECT count(*) FROM _sqlx_migrations WHERE success),
            'failed_migrations', (SELECT count(*) FROM _sqlx_migrations WHERE NOT success),
            'jobs_pending', (SELECT count(*) FROM jobs WHERE status = 'queued'),
            'jobs_dead', (SELECT count(*) FROM jobs WHERE status = 'dead'),
            'jobs_oldest_pending_secs', (SELECT COALESCE(EXTRACT(EPOCH FROM now()-min(created_at))::bigint,0) FROM jobs WHERE status = 'queued'),
            'admins_without_mfa', (SELECT count(*) FROM users u JOIN user_roles ur ON ur.user_id=u.id
                 WHERE ur.role_id='00000000-0000-0000-0000-000000000001' AND NOT EXISTS (SELECT 1 FROM user_mfa m WHERE m.user_id=u.id)),
            'gateways_enabled', (SELECT count(*) FROM payment_gateways WHERE enabled),
            'open_tickets', (SELECT count(*) FROM tickets WHERE status <> 'closed'))")
        .fetch_one(pool).await.map_err(db_err)?;
    let n = |k: &str| row[k].as_i64().unwrap_or(0);
    let mut checks = vec![
        json!({"key": "database", "status": "ok"}),
        json!({"key": "migrations", "status": if n("failed_migrations") == 0 { "ok" } else { "error" }}),
        json!({"key": "dead_jobs", "status": if n("jobs_dead") == 0 { "ok" } else { "warning" }, "value": n("jobs_dead")}),
        json!({"key": "queue_lag", "status": if n("jobs_oldest_pending_secs") < 900 { "ok" } else { "warning" }, "value": n("jobs_oldest_pending_secs")}),
        json!({"key": "admin_mfa", "status": if n("admins_without_mfa") == 0 { "ok" } else { "warning" }, "value": n("admins_without_mfa")}),
        json!({"key": "gateways", "status": if n("gateways_enabled") > 0 { "ok" } else { "warning" }, "value": n("gateways_enabled")}),
    ];
    checks.retain(|c| !c.is_null());
    Ok(json!({ "metrics": row, "checks": checks }))
}

pub async fn my_notes(pool: &PgPool, user: Uuid) -> AppResult<Value> {
    let r: Option<(String, time::OffsetDateTime)> =
        sqlx::query_as("SELECT body, updated_at FROM admin_my_notes WHERE user_id=$1")
            .bind(user)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    Ok(match r {
        Some((b, t)) => json!({ "body": b, "updated_at": t.unix_timestamp() }),
        None => json!({ "body": "", "updated_at": null }),
    })
}

pub async fn save_my_notes(pool: &PgPool, user: Uuid, body: &str) -> AppResult<()> {
    if body.chars().count() > 20000 {
        return Err(AppError::Validation("error.validation"));
    }
    sqlx::query("INSERT INTO admin_my_notes (user_id, body) VALUES ($1,$2) ON CONFLICT (user_id) DO UPDATE SET body=EXCLUDED.body, updated_at=now()")
        .bind(user).bind(body).execute(pool).await.map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct RuleBody {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub url: String,
    /// Telegram chat id, SMS phone number (E.164) or email-to-SMS address.
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
}
fn yes() -> bool {
    true
}

/// Provider-specific checks on url/target.
pub fn channel_ok(provider: &str, url: &str, target: &str) -> bool {
    let https =
        url.starts_with("https://") && url.len() <= 500 && !url.contains(char::is_whitespace);
    let phone = target.len() >= 6
        && target.len() <= 16
        && target
            .strip_prefix('+')
            .unwrap_or(target)
            .chars()
            .all(|c| c.is_ascii_digit());
    match provider {
        "telegram" => {
            let num = target.strip_prefix('-').unwrap_or(target);
            let chat =
                (!num.is_empty() && num.len() <= 32 && num.chars().all(|c| c.is_ascii_digit()))
                    || target.strip_prefix('@').is_some_and(|n| {
                        !n.is_empty()
                            && n.len() <= 64
                            && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    });
            https && url.starts_with("https://api.telegram.org/bot") && chat
        }
        "sms_webhook" => https && phone,
        "sms_email" => {
            url.is_empty()
                && target.len() <= 254
                && target
                    .split_once('@')
                    .is_some_and(|(a, d)| !a.is_empty() && d.contains('.'))
                && !target.contains(char::is_whitespace)
        }
        _ => https && target.is_empty(),
    }
}

fn validate(b: &RuleBody) -> AppResult<()> {
    if b.name.trim().is_empty()
        || b.name.len() > 120
        || !PROVIDERS.contains(&b.provider.as_str())
        || !channel_ok(&b.provider, &b.url, &b.target)
        || b.events.is_empty()
        || b.events.iter().any(|e| !EVENTS.contains(&e.as_str()))
    {
        return Err(AppError::Validation("error.validation"));
    }
    Ok(())
}

/// URL is masked in listings (it is a bearer secret for Slack/Discord/Teams).
pub async fn list_rules(pool: &PgPool) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar("SELECT json_agg(r ORDER BY r.created_at) FROM (SELECT id, name, provider,
        regexp_replace(url, '^(https://[^/]+/).*$', '\\1…') AS url_masked, target, events, enabled, secret_sealed IS NOT NULL AS signed, created_at,
        (SELECT json_build_object('delivered', count(*) FILTER (WHERE status='delivered'), 'failed', count(*) FILTER (WHERE status='failed'), 'pending', count(*) FILTER (WHERE status='pending'))
           FROM notification_deliveries d WHERE d.rule_id = notification_rules.id) AS stats FROM notification_rules) r")
        .fetch_one(pool).await.map_err(db_err)?;
    Ok(v.unwrap_or_else(|| json!([])))
}

pub async fn create_rule(pool: &PgPool, b: &RuleBody, by: Uuid) -> AppResult<Uuid> {
    validate(b)?;
    sqlx::query_scalar("INSERT INTO notification_rules (name, provider, url, target, events, enabled, created_by) VALUES ($1,$2,$3,$4,$5,$6,$7) RETURNING id")
        .bind(b.name.trim()).bind(&b.provider).bind(&b.url).bind(b.target.trim()).bind(&b.events).bind(b.enabled).bind(by)
        .fetch_one(pool).await.map_err(db_err)
}

pub async fn update_rule(pool: &PgPool, id: Uuid, b: &RuleBody) -> AppResult<()> {
    validate(b)?;
    let r = sqlx::query("UPDATE notification_rules SET name=$2, provider=$3, url=$4, target=$5, events=$6, enabled=$7, updated_at=now() WHERE id=$1")
        .bind(id).bind(b.name.trim()).bind(&b.provider).bind(&b.url).bind(b.target.trim()).bind(&b.events).bind(b.enabled)
        .execute(pool).await.map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn delete_rule(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM notification_rules WHERE id=$1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// An enabled rule subscribed to an event.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ActiveRule {
    pub id: Uuid,
    pub provider: String,
    pub url: String,
    pub target: String,
    pub secret_sealed: Option<Vec<u8>>,
}

/// Enabled rules subscribed to `event`.
pub async fn rules_for(pool: &PgPool, event: &str) -> AppResult<Vec<ActiveRule>> {
    sqlx::query_as("SELECT id, provider, url, target, secret_sealed FROM notification_rules WHERE enabled AND $1 = ANY(events)")
        .bind(event).fetch_all(pool).await.map_err(db_err)
}

pub async fn rule(pool: &PgPool, id: Uuid) -> AppResult<Option<ActiveRule>> {
    sqlx::query_as(
        "SELECT id, provider, url, target, secret_sealed FROM notification_rules WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub async fn set_rule_secret(pool: &PgPool, id: Uuid, sealed: Option<&[u8]>) -> AppResult<()> {
    let r = sqlx::query(
        "UPDATE notification_rules SET secret_sealed = $2, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(sealed)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn create_delivery(pool: &PgPool, rule: Uuid, event: &str) -> AppResult<Uuid> {
    sqlx::query_scalar(
        "INSERT INTO notification_deliveries (rule_id, event) VALUES ($1,$2) RETURNING id",
    )
    .bind(rule)
    .bind(event)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Record one attempt; `ok` → delivered, `final_fail` → failed, else pending.
pub async fn record_delivery(
    pool: &PgPool,
    id: Uuid,
    status: Option<u16>,
    error: Option<&str>,
    ok: bool,
    final_fail: bool,
) -> AppResult<()> {
    let st = if ok {
        "delivered"
    } else if final_fail {
        "failed"
    } else {
        "pending"
    };
    sqlx::query("UPDATE notification_deliveries SET attempts = attempts + 1, response_status = $2, last_error = $3, status = $4,
        delivered_at = CASE WHEN $4 = 'delivered' THEN now() ELSE delivered_at END WHERE id = $1")
        .bind(id).bind(status.map(i32::from)).bind(error.map(|e| e.chars().take(500).collect::<String>())).bind(st)
        .execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn deliveries(pool: &PgPool, rule: Uuid, limit: i64) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar("SELECT json_agg(d ORDER BY d.created_at DESC) FROM (SELECT id, event, status, attempts, response_status, last_error, created_at, delivered_at
        FROM notification_deliveries WHERE rule_id = $1 ORDER BY created_at DESC LIMIT $2) d")
        .bind(rule).bind(limit.clamp(1, 200)).fetch_one(pool).await.map_err(db_err)?;
    Ok(v.unwrap_or_else(|| json!([])))
}

/// Provider-specific JSON payload for a staff notification.
pub fn payload(provider: &str, event: &str, text: &str, url: Option<&str>) -> Value {
    let line = match url {
        Some(u) => format!("{text}\n{u}"),
        None => text.to_string(),
    };
    match provider {
        "slack" => json!({ "text": format!("*{event}*: {line}") }),
        "discord" => json!({ "content": format!("**{event}**: {line}") }),
        "teams" => {
            json!({ "@type": "MessageCard", "@context": "https://schema.org/extensions", "summary": event, "title": event, "text": line })
        }
        "telegram" => {
            json!({ "text": format!("{event}: {line}"), "disable_web_page_preview": true })
        }
        // SMS: one short plain line; gateways bill per 160 chars.
        "sms_webhook" | "sms_email" => {
            json!({ "text": format!("{event}: {line}").chars().take(160).collect::<String>() })
        }
        _ => json!({ "event": event, "text": text, "url": url }),
    }
}

/// Everything for the WHMCS-style client summary page in one round trip.
pub async fn customer_summary(pool: &PgPool, id: Uuid) -> AppResult<Value> {
    let v: Option<Value> = sqlx::query_scalar(
        "SELECT json_build_object(
          'customer', (SELECT to_jsonb(c) - 'notes' FROM customers c WHERE c.id=$1),
          'stats', json_build_object(
             'services_active', (SELECT count(*) FROM services WHERE customer_id=$1 AND status='active'),
             'services_total', (SELECT count(*) FROM services WHERE customer_id=$1),
             'domains_total', (SELECT count(*) FROM domains WHERE customer_id=$1),
             'invoices_unpaid', (SELECT count(*) FROM invoices WHERE customer_id=$1 AND status='unpaid' AND kind='invoice'),
             'amount_due', (SELECT COALESCE(sum(total-amount_paid),0)::text FROM invoices WHERE customer_id=$1 AND status='unpaid' AND kind='invoice'),
             'amount_paid', (SELECT COALESCE(sum(amount_paid),0)::text FROM invoices WHERE customer_id=$1 AND kind='invoice'),
             'tickets_open', (SELECT count(*) FROM tickets WHERE customer_id=$1 AND status<>'closed'),
             'credit', (SELECT COALESCE(sum(amount),0)::text FROM credit_ledger WHERE customer_id=$1)),
          'contacts', (SELECT COALESCE(json_agg(x ORDER BY x.created_at),'[]') FROM customer_contacts x WHERE x.customer_id=$1),
          'services', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT s.id, p.name AS product, s.status, s.cycle, s.recurring_amount::text AS amount, s.currency, s.next_due_date
                FROM services s JOIN products p ON p.id=s.product_id WHERE s.customer_id=$1 ORDER BY s.created_at DESC LIMIT 200) r),
          'domains', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT id, name, status, expires_on, auto_renew FROM domains WHERE customer_id=$1 ORDER BY created_at DESC LIMIT 200) r),
          'invoices', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT id, number, kind, status, total::text AS total, amount_paid::text AS amount_paid, currency, issued_at, due_at
                FROM invoices WHERE customer_id=$1 ORDER BY created_at DESC LIMIT 200) r),
          'transactions', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT p.id, p.gateway, p.provider_ref, p.amount::text AS amount, p.currency, p.status, p.created_at
                FROM payments p JOIN customers c ON c.owner_user_id=p.user_id WHERE c.id=$1 ORDER BY p.created_at DESC LIMIT 200) r),
          'credit_ledger', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT id, amount::text AS amount, currency, kind, note, created_at FROM credit_ledger WHERE customer_id=$1 ORDER BY created_at DESC LIMIT 200) r),
          'tickets', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT id, number, subject, status, priority, last_reply_at FROM tickets WHERE customer_id=$1 ORDER BY last_reply_at DESC LIMIT 200) r),
          'notes', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT n.id, n.body, n.sticky, n.created_at, u.email::text AS author FROM customer_notes n LEFT JOIN users u ON u.id=n.author_id
                WHERE n.customer_id=$1 ORDER BY n.sticky DESC, n.created_at DESC) r),
          'log', (SELECT COALESCE(json_agg(r),'[]') FROM (SELECT a.id, a.action, a.target, a.ip, a.created_at, u.email::text AS actor FROM audit_logs a LEFT JOIN users u ON u.id=a.actor_id
                WHERE a.target=$1::text OR a.actor_id=(SELECT owner_user_id FROM customers WHERE id=$1)
                   OR a.target IN (SELECT id::text FROM invoices WHERE customer_id=$1 UNION ALL SELECT id::text FROM services WHERE customer_id=$1)
                ORDER BY a.created_at DESC LIMIT 200) r)
        ) WHERE EXISTS (SELECT 1 FROM customers WHERE id=$1)")
        .bind(id).fetch_optional(pool).await.map_err(db_err)?;
    v.ok_or(AppError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_shapes() {
        assert_eq!(
            payload("slack", "invoice.paid", "Paid", None)["text"],
            "*invoice.paid*: Paid"
        );
        assert_eq!(
            payload("discord", "e", "t", Some("https://x"))["content"],
            "**e**: t\nhttps://x"
        );
        assert_eq!(payload("teams", "e", "t", None)["@type"], "MessageCard");
        assert_eq!(payload("webhook", "e", "t", None)["event"], "e");
        assert_eq!(payload("telegram", "e", "t", None)["text"], "e: t");
        assert_eq!(
            payload("sms_webhook", "e", &"x".repeat(300), None)["text"]
                .as_str()
                .unwrap()
                .len(),
            160
        );
    }
    #[test]
    fn channels_validate() {
        assert!(channel_ok(
            "telegram",
            "https://api.telegram.org/bot123:abc/sendMessage",
            "-100123"
        ));
        assert!(channel_ok(
            "telegram",
            "https://api.telegram.org/bot123:abc/sendMessage",
            "@ops"
        ));
        assert!(!channel_ok("telegram", "https://evil.example/bot", "1"));
        assert!(!channel_ok(
            "telegram",
            "https://api.telegram.org/bot1/sendMessage",
            ""
        ));
        assert!(channel_ok(
            "sms_webhook",
            "https://sms.example/send",
            "+40712345678"
        ));
        assert!(!channel_ok(
            "sms_webhook",
            "https://sms.example/send",
            "abc"
        ));
        assert!(channel_ok("sms_email", "", "0712345678@sms.example.com"));
        assert!(!channel_ok("sms_email", "https://x", "a@b.c"));
        assert!(channel_ok("slack", "https://hooks.slack.com/x", ""));
        assert!(!channel_ok("slack", "http://hooks.slack.com/x", ""));
    }
}
