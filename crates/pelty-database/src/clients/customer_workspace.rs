//! Permission-filtered, allowlisted customer workspace projections.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Access {
    pub finance: bool,
    pub services: bool,
    pub support: bool,
    pub operations: bool,
    pub audit: bool,
    pub manage_customer: bool,
}

// All fragments below are source-controlled projections, never caller SQL.
struct Section {
    name: &'static str,
    kind: &'static str,
    allowed: bool,
    sql: &'static str,
    title: &'static str,
    status: &'static str,
}

fn sections(a: Access) -> Vec<Section> {
    vec![
        Section { name: "contacts", kind: "", allowed: true, sql: "SELECT id,name,email::text,phone,is_billing,created_at FROM customer_contacts WHERE customer_id=$1", title: "'Contact'", status: "NULL::text" },
        Section { name: "services", kind: "service", allowed: a.services, sql: "SELECT s.id,p.name AS product,s.status,s.cycle,s.recurring_amount::text AS amount,s.currency,s.next_due_date,s.created_at FROM services s JOIN products p ON p.id=s.product_id WHERE s.customer_id=$1", title: "product", status: "status" },
        Section { name: "domains", kind: "domain", allowed: a.services, sql: "SELECT id,name,status,expires_on,auto_renew,created_at FROM domains WHERE customer_id=$1", title: "name", status: "status" },
        Section { name: "invoices", kind: "invoice", allowed: a.finance, sql: "SELECT id,number,kind,status,total::text,amount_paid::text,currency,issued_at,due_at,created_at FROM invoices WHERE customer_id=$1", title: "COALESCE(number,'Invoice')", status: "status" },
        Section { name: "transactions", kind: "payment", allowed: a.finance, sql: "SELECT p.id,p.gateway,p.provider_ref,p.amount::text,p.currency,p.status,p.created_at FROM payments p JOIN invoices i ON i.id=p.invoice_id WHERE i.customer_id=$1", title: "'Payment'", status: "status" },
        Section { name: "credit_ledger", kind: "credit", allowed: a.finance, sql: "SELECT id,amount::text,currency,kind,note,created_at FROM credit_ledger WHERE customer_id=$1", title: "'Credit entry'", status: "kind" },
        Section { name: "tickets", kind: "ticket", allowed: a.support, sql: "SELECT id,number,subject,status,priority,last_reply_at,created_at FROM tickets WHERE customer_id=$1", title: "subject", status: "status" },
        Section { name: "notes", kind: "note", allowed: true, sql: "SELECT n.id,n.body,n.sticky,n.created_at,u.email::text AS author FROM customer_notes n LEFT JOIN users u ON u.id=n.author_id WHERE n.customer_id=$1", title: "'Customer note'", status: "NULL::text" },
        Section { name: "refunds", kind: "refund", allowed: a.finance, sql: "SELECT r.id,r.payment_id,r.invoice_id,r.amount::text,r.currency,r.status,r.to_credit,r.created_at FROM refunds r JOIN invoices i ON i.id=r.invoice_id WHERE r.customer_id=$1 AND i.customer_id=$1", title: "'Refund'", status: "status" },
        Section { name: "chats", kind: "chat", allowed: a.support, sql: "SELECT id,status,created_at,last_message_at,closed_at FROM chat_sessions WHERE customer_id=$1", title: "'Customer chat'", status: "status" },
        // email_log has no job foreign key. Do not infer a job or expose mail bodies/errors.
        Section { name: "emails", kind: "email", allowed: true, sql: "SELECT e.id::text AS id,NULL::text AS job_id,e.status,e.template,e.subject,e.created_at FROM email_log e JOIN customers c ON c.owner_user_id=e.user_id WHERE c.id=$1", title: "'Email delivery'", status: "status" },
        // Compare text rather than casting untrusted payload strings to UUID.
        Section { name: "failures", kind: "job", allowed: a.operations && a.services, sql: "SELECT j.id::text AS id,j.kind,j.status,s.id AS service_id,j.created_at FROM jobs j JOIN services s ON s.id::text=j.payload->>'service_id' WHERE s.customer_id=$1 AND j.kind='provision' AND (j.status IN ('failed','dead') OR (j.status='queued' AND j.attempts>0 AND NULLIF(j.last_error,'') IS NOT NULL))", title: "'Provisioning failure'", status: "status" },
        // No actor-based matching: an owner can act on unrelated resources. Resource
        // permissions also apply to audit targets, before either limiting or counting.
        Section { name: "log", kind: "audit", allowed: a.audit, sql: "SELECT a.id::text AS id,a.action,a.target,a.ip,a.created_at,u.email::text AS actor FROM audit_logs a LEFT JOIN users u ON u.id=a.actor_id WHERE a.target=$1::uuid::text", title: "action", status: "NULL::text" },
    ]
}

async fn customer(conn: &mut PgConnection, id: Uuid) -> AppResult<Value> {
    sqlx::query_scalar::<_, Value>("SELECT jsonb_build_object('id',id,'email',email,'first_name',first_name,'last_name',last_name,'organization',organization,'status',status,'currency',currency,'phone',phone,'address_line1',address_line1,'address_line2',address_line2,'city',city,'region',region,'postal_code',postal_code,'country',country,'tax_id',tax_id,'created_at',created_at) FROM customers WHERE id=$1")
        .bind(id).fetch_optional(conn).await.map_err(db_err)?.ok_or(AppError::NotFound)
}

pub async fn workspace(pool: &PgPool, id: Uuid, access: Access) -> AppResult<Value> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    let c = customer(&mut tx, id).await?;
    let mut result = json!({"customer":c,"access":access,"limits":{"records":50},"totals":{},"meta":{},"attention":[],"balances":[]});
    for section in sections(access) {
        if !section.allowed {
            result[section.name] = json!([]);
            result["totals"][section.name] = Value::Null;
            result["meta"][section.name] =
                json!({"total":null,"returned":0,"truncated":false,"accessible":false});
            continue;
        }
        // Apply pinned-first ordering before LIMIT as well as in the JSON array,
        // so older sticky notes are not displaced by newer unpinned notes.
        let (order, aggregate_order) = if section.name == "notes" {
            (
                "sticky DESC,created_at DESC,id DESC",
                "r.sticky DESC,r.created_at DESC,r.id DESC",
            )
        } else {
            ("created_at DESC,id DESC", "r.created_at DESC,r.id DESC")
        };
        let sql = format!("SELECT jsonb_build_object('items',COALESCE((SELECT jsonb_agg(to_jsonb(r) ORDER BY {aggregate_order}) FROM (SELECT * FROM ({0}) source ORDER BY {order} LIMIT 50) r),'[]'::jsonb),'total',(SELECT count(*) FROM ({0}) source))", section.sql);
        let page: Value = sqlx::query_scalar(&sql)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        let returned = page["items"].as_array().map_or(0, Vec::len);
        result[section.name] = page["items"].clone();
        result["totals"][section.name] = page["total"].clone();
        result["meta"][section.name] = json!({"total":page["total"],"returned":returned,"truncated":page["total"].as_u64().unwrap_or(0)>returned as u64,"accessible":true});
    }
    let counts: Value = sqlx::query_scalar("SELECT jsonb_build_object('services_active',CASE WHEN $2 THEN (SELECT count(*) FROM services WHERE customer_id=$1 AND status='active') END,'services_total',CASE WHEN $2 THEN (SELECT count(*) FROM services WHERE customer_id=$1) END,'domains_total',CASE WHEN $2 THEN (SELECT count(*) FROM domains WHERE customer_id=$1) END,'tickets_open',CASE WHEN $3 THEN (SELECT count(*) FROM tickets WHERE customer_id=$1 AND status<>'closed') END)")
        .bind(id).bind(access.services).bind(access.support).fetch_one(&mut *tx).await.map_err(db_err)?;
    result["counts"] = counts;
    if access.finance {
        result["balances"] = sqlx::query_scalar::<_, Value>("WITH currencies AS (SELECT currency FROM customers WHERE id=$1 UNION SELECT currency FROM invoices WHERE customer_id=$1 UNION SELECT currency FROM credit_ledger WHERE customer_id=$1), inv AS (SELECT currency,COALESCE(sum(GREATEST(total-amount_paid,0)) FILTER (WHERE status='unpaid'),0)::text AS amount_due,COALESCE(sum(amount_paid),0)::text AS amount_paid,count(*) FILTER (WHERE status='unpaid') AS invoices_unpaid FROM invoices WHERE customer_id=$1 AND kind='invoice' GROUP BY currency), cr AS (SELECT currency,sum(amount)::text AS credit FROM credit_ledger WHERE customer_id=$1 GROUP BY currency) SELECT COALESCE(jsonb_agg(jsonb_build_object('currency',c.currency,'amount_due',COALESCE(i.amount_due,'0'),'amount_paid',COALESCE(i.amount_paid,'0'),'credit',COALESCE(cr.credit,'0'),'invoices_unpaid',COALESCE(i.invoices_unpaid,0)) ORDER BY c.currency),'[]'::jsonb) FROM currencies c LEFT JOIN inv i USING(currency) LEFT JOIN cr USING(currency)")
            .bind(id).fetch_one(&mut *tx).await.map_err(db_err)?;
    }
    let mut attention = Vec::new();
    if access.finance {
        let unpaid: i64 = sqlx::query_scalar("SELECT count(*) FROM invoices WHERE customer_id=$1 AND kind='invoice' AND status='unpaid'")
            .bind(id).fetch_one(&mut *tx).await.map_err(db_err)?;
        if unpaid > 0 {
            attention.push(json!({"kind":"invoice","count":unpaid}));
        }
    }
    for (kind, count) in [
        ("ticket", result["counts"]["tickets_open"].as_i64()),
        ("job", result["totals"]["failures"].as_i64()),
    ] {
        if let Some(count) = count.filter(|n| *n > 0) {
            attention.push(json!({"kind":kind,"count":count}));
        }
    }
    result["attention"] = json!(attention);
    tx.commit().await.map_err(db_err)?;
    Ok(result)
}

pub const KINDS: &[&str] = &[
    "all", "invoice", "payment", "refund", "credit", "service", "domain", "ticket", "chat",
    "email", "job", "note", "audit", "customer",
];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    customer: Uuid,
    kind: String,
    #[serde(with = "time::serde::rfc3339")]
    at: time::OffsetDateTime,
    key: String,
}

/// Cursor contents are only a validated position; never SQL or authorization.
pub async fn activity(
    pool: &PgPool,
    id: Uuid,
    access: Access,
    kind: &str,
    before: Option<&str>,
    limit: i64,
) -> AppResult<Value> {
    if !KINDS.contains(&kind) || !(1..=100).contains(&limit) {
        return Err(AppError::Validation("error.validation"));
    }
    let cursor = before
        .map(|text| {
            if text.len() > 512 {
                return Err(AppError::Validation("error.validation"));
            }
            let c: Cursor =
                serde_json::from_str(text).map_err(|_| AppError::Validation("error.validation"))?;
            let (event_kind, event_id) = c
                .key
                .split_once(':')
                .ok_or(AppError::Validation("error.validation"))?;
            if c.customer != id
                || c.kind != kind
                || !KINDS.contains(&event_kind)
                || event_kind == "all"
                || event_id.len() > 40
                || event_id.is_empty()
                || !event_id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
            {
                return Err(AppError::Validation("error.validation"));
            }
            Ok(c)
        })
        .transpose()?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    customer(&mut tx, id).await?;
    let mut branches = Vec::new();
    for section in sections(access)
        .into_iter()
        .filter(|s| s.allowed && !s.kind.is_empty() && (kind == "all" || kind == s.kind))
    {
        // Titles for notes/mail/jobs are deliberately generic, without bodies/payloads.
        branches.push(format!("SELECT '{0}:'||id::text AS id,'{0}'::text AS kind,created_at AS occurred_at,{1}::text AS title,{2}::text AS status FROM ({3}) source", section.kind, section.title, section.status, section.sql));
    }
    if kind == "all" || kind == "customer" {
        branches.push("SELECT 'customer:'||id::text AS id,'customer'::text AS kind,created_at AS occurred_at,'Customer created'::text AS title,status FROM customers WHERE id=$1".into());
    }
    if branches.is_empty() {
        tx.commit().await.map_err(db_err)?;
        return Ok(json!({"items":[],"next_cursor":null}));
    }
    let sql = format!("SELECT id,kind,occurred_at,title,status FROM ({}) events WHERE ($2::timestamptz IS NULL OR (occurred_at,id COLLATE \"C\") < ($2,$3::text COLLATE \"C\")) ORDER BY occurred_at DESC,id COLLATE \"C\" DESC LIMIT $4", branches.join(" UNION ALL "));
    let mut rows: Vec<(String, String, time::OffsetDateTime, String, Option<String>)> =
        sqlx::query_as(&sql)
            .bind(id)
            .bind(cursor.as_ref().map(|c| c.at))
            .bind(cursor.as_ref().map(|c| c.key.as_str()))
            .bind(limit + 1)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
    let more = rows.len() > limit as usize;
    rows.truncate(limit as usize);
    let next = if more {
        rows.last()
            .map(|r| {
                serde_json::to_string(&Cursor {
                    customer: id,
                    kind: kind.into(),
                    at: r.2,
                    key: r.0.clone(),
                })
            })
            .transpose()
            .map_err(|_| AppError::Internal("cursor".into()))?
    } else {
        None
    };
    let items: Vec<Value> = rows.into_iter().map(|(key,kind,at,title,status)| {
        // Event keys are built from database UUIDs/integers, never titles or payloads.
        let resource_id = key.split_once(':').map(|(_, id)| id).unwrap_or_default();
        let href = match kind.as_str() {
            "invoice" => format!("/admin/invoices/{resource_id}"),
            "domain" => format!("/admin/domains/{resource_id}"),
            "ticket" => format!("/admin/tickets/{resource_id}"),
            "service" => format!("/admin/services?q={resource_id}"),
            _ => {
                let tab = match kind.as_str() {
                    "payment" | "refund" | "credit" => "transactions",
                    "chat" => "chats",
                    "email" => "emails",
                    "note" => "notes",
                    "audit" => "log",
                    "job" => "operations",
                    _ => "summary",
                };
                format!("/admin/customers/{id}?tab={tab}")
            }
        };
        json!({"id":key,"kind":kind,"occurred_at":at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),"title":title,"status":status,"href":href})
    }).collect();
    tx.commit().await.map_err(db_err)?;
    Ok(json!({"items":items,"next_cursor":next}))
}
