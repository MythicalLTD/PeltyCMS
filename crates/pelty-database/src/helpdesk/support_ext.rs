//! Support extensions: department auto-reply + mailbox settings, spam
//! rules, time tracking, KB tags and contact form messages.
use crate::db_err;
use pelty_core::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

// ---- Department settings ----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct DeptSettings {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub auto_reply: bool,
    pub auto_reply_text: String,
    pub pipe_protocol: String,
    pub pipe_host: String,
    pub pipe_port: i32,
    pub pipe_tls: bool,
    pub pipe_username: String,
    /// Sealed; never serialised.
    #[serde(skip)]
    pub pipe_password: String,
    pub pipe_last_run: Option<OffsetDateTime>,
    pub pipe_last_error: String,
    pub pipe_unknown_sender: String,
    #[serde(skip)]
    pub pipe_webhook_secret: String,
}

const DEPT_COLS: &str = "id, name, email, auto_reply, auto_reply_text, pipe_protocol, pipe_host, pipe_port, pipe_tls, pipe_username, pipe_password, pipe_last_run, pipe_last_error, pipe_unknown_sender, pipe_webhook_secret";

pub async fn departments(pool: &PgPool) -> AppResult<Vec<DeptSettings>> {
    sqlx::query_as(&format!(
        "SELECT {DEPT_COLS} FROM ticket_departments ORDER BY sort_order, name"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn department(pool: &PgPool, id: Uuid) -> AppResult<Option<DeptSettings>> {
    sqlx::query_as(&format!(
        "SELECT {DEPT_COLS} FROM ticket_departments WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

pub const PROTOCOLS: &[&str] = &["none", "imap", "pop3"];

#[derive(Debug, Clone, Deserialize)]
pub struct DeptUpdate {
    pub auto_reply: bool,
    pub auto_reply_text: String,
    pub pipe_protocol: String,
    pub pipe_host: String,
    pub pipe_port: i32,
    pub pipe_tls: bool,
    pub pipe_username: String,
    /// `None` keeps the stored (sealed) password.
    pub pipe_password_sealed: Option<String>,
    pub pipe_unknown_sender: Option<String>,
}

pub async fn update_department(pool: &PgPool, id: Uuid, u: &DeptUpdate) -> AppResult<DeptSettings> {
    sqlx::query_as(&format!(
        "UPDATE ticket_departments SET auto_reply=$2, auto_reply_text=$3, pipe_protocol=$4, pipe_host=$5, pipe_port=$6, pipe_tls=$7,
           pipe_username=$8, pipe_password=COALESCE($9, pipe_password), pipe_unknown_sender=COALESCE($10, pipe_unknown_sender) WHERE id=$1 RETURNING {DEPT_COLS}"
    ))
    .bind(id)
    .bind(u.auto_reply)
    .bind(&u.auto_reply_text)
    .bind(&u.pipe_protocol)
    .bind(&u.pipe_host)
    .bind(u.pipe_port)
    .bind(u.pipe_tls)
    .bind(&u.pipe_username)
    .bind(&u.pipe_password_sealed)
    .bind(&u.pipe_unknown_sender)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn mark_pipe_run(pool: &PgPool, id: Uuid, error: &str) -> AppResult<()> {
    sqlx::query(
        "UPDATE ticket_departments SET pipe_last_run = now(), pipe_last_error = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(error.chars().take(500).collect::<String>())
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Department of a ticket, for the auto-reply.
pub async fn auto_reply_for(pool: &PgPool, department: Uuid) -> AppResult<Option<String>> {
    sqlx::query_scalar(
        "SELECT auto_reply_text FROM ticket_departments WHERE id = $1 AND auto_reply AND is_active",
    )
    .bind(department)
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

// ---- Spam control ----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SpamRule {
    pub id: Uuid,
    pub kind: String,
    pub value: String,
    pub hits: i64,
    pub created_at: OffsetDateTime,
}

pub const SPAM_KINDS: &[&str] = &["sender", "domain", "subject", "phrase"];

pub async fn spam_rules(pool: &PgPool) -> AppResult<Vec<SpamRule>> {
    sqlx::query_as(
        "SELECT id, kind, value, hits, created_at FROM ticket_spam_rules ORDER BY kind, value",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_spam_rule(pool: &PgPool, kind: &str, value: &str) -> AppResult<SpamRule> {
    sqlx::query_as(
        "INSERT INTO ticket_spam_rules (kind, value) VALUES ($1, $2)
         ON CONFLICT (kind, value) DO UPDATE SET value = EXCLUDED.value RETURNING id, kind, value, hits, created_at",
    )
    .bind(kind)
    .bind(value)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

pub async fn delete_spam_rule(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM ticket_spam_rules WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Pure matcher: the first rule that hits `(sender, subject, body)`.
pub fn spam_match<'a>(
    rules: &'a [SpamRule],
    sender: &str,
    subject: &str,
    body: &str,
) -> Option<&'a SpamRule> {
    let sender = sender.to_lowercase();
    let domain = sender.rsplit_once('@').map(|x| x.1).unwrap_or("");
    let (subject, body) = (subject.to_lowercase(), body.to_lowercase());
    rules.iter().find(|r| {
        let v = r.value.to_lowercase();
        match r.kind.as_str() {
            "sender" => sender == v,
            "domain" => domain == v || domain.ends_with(&format!(".{v}")),
            "subject" => subject.contains(&v),
            "phrase" => body.contains(&v) || subject.contains(&v),
            _ => false,
        }
    })
}

pub async fn spam_hit(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("UPDATE ticket_spam_rules SET hits = hits + 1 WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

// ---- Time tracking ----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TimeEntry {
    pub id: Uuid,
    pub ticket_id: Uuid,
    pub staff_id: Option<Uuid>,
    pub staff_name: Option<String>,
    pub minutes: i32,
    pub note: String,
    pub billable: bool,
    pub created_at: OffsetDateTime,
}

pub async fn time_entries(pool: &PgPool, ticket: Uuid) -> AppResult<Vec<TimeEntry>> {
    sqlx::query_as(
        "SELECT e.id, e.ticket_id, e.staff_id, COALESCE(NULLIF(u.display_name, ''), u.email::text) AS staff_name, e.minutes, e.note, e.billable, e.created_at
         FROM ticket_time_entries e LEFT JOIN users u ON u.id = e.staff_id WHERE e.ticket_id = $1 ORDER BY e.created_at",
    )
    .bind(ticket)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_time(
    pool: &PgPool,
    ticket: Uuid,
    staff: Uuid,
    minutes: i32,
    note: &str,
    billable: bool,
) -> AppResult<Uuid> {
    sqlx::query_scalar("INSERT INTO ticket_time_entries (ticket_id, staff_id, minutes, note, billable) VALUES ($1,$2,$3,$4,$5) RETURNING id")
        .bind(ticket)
        .bind(staff)
        .bind(minutes)
        .bind(note)
        .bind(billable)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn delete_time(pool: &PgPool, ticket: Uuid, id: Uuid) -> AppResult<()> {
    let r = sqlx::query("DELETE FROM ticket_time_entries WHERE id = $1 AND ticket_id = $2")
        .bind(id)
        .bind(ticket)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---- KB tags ----

pub async fn set_article_tags(pool: &PgPool, id: Uuid, tags: &[String]) -> AppResult<()> {
    let r = sqlx::query("UPDATE kb_articles SET tags = $2 WHERE id = $1")
        .bind(id)
        .bind(tags)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

pub async fn article_tags(pool: &PgPool, id: Uuid) -> AppResult<Vec<String>> {
    Ok(
        sqlx::query_scalar::<_, Vec<String>>("SELECT tags FROM kb_articles WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .unwrap_or_default(),
    )
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct TaggedArticle {
    pub id: Uuid,
    pub slug: String,
    pub title: String,
    pub tags: Vec<String>,
}

/// Published articles with this tag.
pub async fn articles_by_tag(pool: &PgPool, tag: &str) -> AppResult<Vec<TaggedArticle>> {
    sqlx::query_as("SELECT id, slug, title, tags FROM kb_articles WHERE is_published AND $1 = ANY(tags) ORDER BY title LIMIT 100")
        .bind(tag)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// All tags in use on published articles, with counts.
pub async fn tag_cloud(pool: &PgPool) -> AppResult<Vec<(String, i64)>> {
    sqlx::query_as("SELECT t, count(*) FROM kb_articles, unnest(tags) t WHERE is_published GROUP BY t ORDER BY count(*) DESC, t LIMIT 100")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

// ---- Contact form ----

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ContactMessage {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub subject: String,
    pub body: String,
    pub ip: Option<String>,
    pub status: String,
    pub ticket_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

const CM_COLS: &str = "id, name, email, subject, body, ip, status, ticket_id, created_at";

pub async fn add_contact(
    pool: &PgPool,
    name: &str,
    email: &str,
    subject: &str,
    body: &str,
    ip: Option<&str>,
    spam: bool,
) -> AppResult<ContactMessage> {
    sqlx::query_as(&format!("INSERT INTO contact_messages (name, email, subject, body, ip, status) VALUES ($1,$2,$3,$4,$5,$6) RETURNING {CM_COLS}"))
        .bind(name)
        .bind(email)
        .bind(subject)
        .bind(body)
        .bind(ip)
        .bind(if spam { "spam" } else { "new" })
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn contacts(pool: &PgPool, status: Option<&str>) -> AppResult<Vec<ContactMessage>> {
    sqlx::query_as(&format!("SELECT {CM_COLS} FROM contact_messages WHERE ($1::text IS NULL OR status = $1) ORDER BY created_at DESC LIMIT 200"))
        .bind(status)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub async fn contact(pool: &PgPool, id: Uuid) -> AppResult<ContactMessage> {
    sqlx::query_as(&format!(
        "SELECT {CM_COLS} FROM contact_messages WHERE id = $1"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or(AppError::NotFound)
}

pub async fn set_contact_status(
    pool: &PgPool,
    id: Uuid,
    status: &str,
    ticket: Option<Uuid>,
) -> AppResult<ContactMessage> {
    sqlx::query_as(&format!("UPDATE contact_messages SET status = $2, ticket_id = COALESCE($3, ticket_id) WHERE id = $1 RETURNING {CM_COLS}"))
        .bind(id)
        .bind(status)
        .bind(ticket)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or(AppError::NotFound)
}

pub async fn delete_contact(pool: &PgPool, id: Uuid) -> AppResult<()> {
    sqlx::query("DELETE FROM contact_messages WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

// ---- Ticket watchers (multiple staff per ticket) ----

pub async fn watchers(pool: &PgPool, ticket: Uuid) -> AppResult<Vec<Uuid>> {
    sqlx::query_scalar("SELECT user_id FROM ticket_watchers WHERE ticket_id = $1 ORDER BY added_at")
        .bind(ticket)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Replace the watcher list; only staff accounts are kept.
pub async fn set_watchers(pool: &PgPool, ticket: Uuid, users: &[Uuid]) -> AppResult<Vec<Uuid>> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("DELETE FROM ticket_watchers WHERE ticket_id = $1")
        .bind(ticket)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    sqlx::query(
        "INSERT INTO ticket_watchers (ticket_id, user_id)
         SELECT DISTINCT $1::uuid, u.id FROM users u
         JOIN user_roles ur ON ur.user_id = u.id JOIN role_permissions rp ON rp.role_id = ur.role_id
         JOIN permissions p ON p.id = rp.permission_id
         WHERE u.id = ANY($2) AND p.name = 'tickets.manage' AND u.is_active ON CONFLICT DO NOTHING",
    )
    .bind(ticket)
    .bind(users)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    watchers(pool, ticket).await
}

pub const INBOUND_NOTIFY_JOB: &str = "support.inbound_notify";

/// Outgoing Message-IDs are routing hints, never authorization. Only accept the
/// exact format emitted by the SMTP delivery path; bind the parsed UUID in SQL.
fn outgoing_ticket_reference(reference: &str) -> Option<Uuid> {
    let reference = reference
        .strip_prefix("ticket.")?
        .strip_suffix("@pelty.invalid")?;
    let (ticket, delivery) = reference.split_once('.')?;
    let ticket = Uuid::parse_str(ticket).ok()?;
    let delivery = Uuid::parse_str(delivery).ok()?;
    (!ticket.is_nil() && !delivery.is_nil()).then_some(ticket)
}

/// Validated inbound files are inserted already linked, in the intake transaction.
pub struct InboundAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub struct InboundTicket<'a> {
    pub message_id: &'a str,
    pub sender: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub department: Option<Uuid>,
    pub customer: Option<(Uuid, Uuid)>,
    pub number: Option<i64>,
    pub references: &'a [String],
    pub reject: bool,
    pub spam: bool,
    pub attachments: &'a [InboundAttachment],
}

/// Deduplication, threading, message and attachments commit together. A failed
/// database write rolls back the claim, so a provider retry can safely recover.
pub async fn ingest_email(
    pool: &PgPool,
    m: InboundTicket<'_>,
) -> AppResult<(&'static str, Option<Uuid>)> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let claimed = sqlx::query("INSERT INTO ticket_pipe_log (message_id, sender, subject, outcome) VALUES ($1,$2,$3,'rejected') ON CONFLICT DO NOTHING")
        .bind(m.message_id).bind(m.sender).bind(m.subject).execute(&mut *tx).await.map_err(db_err)?.rows_affected();
    if claimed == 0 {
        return Ok(("duplicate", None));
    }
    let mut outcome = if m.spam { "spam" } else { "rejected" };
    let mut ticket = None;
    let department_policy: Option<String> = if let Some(department) = m.department {
        sqlx::query_scalar("SELECT pipe_unknown_sender FROM ticket_departments WHERE id=$1 AND is_active FOR SHARE")
            .bind(department).fetch_optional(&mut *tx).await.map_err(db_err)?
    } else {
        None
    };
    let department_active = m.department.is_none() || department_policy.is_some();
    if !m.reject && !m.spam && department_active {
        let customer = m.customer.map(|c| c.0);
        let user = m.customer.map(|c| c.1);
        // References are hints, never authorization. Always verify the sender
        // owns the destination; a known client cannot claim a guest's ticket.
        let mut target = None;
        for reference in m.references {
            target = sqlx::query_scalar::<_, Uuid>(
                "SELECT COALESCE(t.merged_into,t.id) FROM tickets t
                 WHERE (t.id=$5 OR EXISTS(SELECT 1 FROM ticket_pipe_log l WHERE l.ticket_id=t.id AND l.message_id=$1))
                   AND (($2::uuid IS NOT NULL AND t.customer_id=$2) OR ($2 IS NULL AND t.customer_id IS NULL AND lower(t.guest_email)=lower($3)))
                   AND ($4::uuid IS NULL OR t.department_id=$4) LIMIT 1")
                .bind(reference).bind(customer).bind(m.sender).bind(m.department)
                .bind(outgoing_ticket_reference(reference))
                .fetch_optional(&mut *tx).await.map_err(db_err)?;
            if target.is_some() {
                break;
            }
        }
        if target.is_none() {
            if let Some(number) = m.number {
                target = sqlx::query_scalar("SELECT COALESCE(merged_into,id) FROM tickets WHERE number=$1 AND (($2::uuid IS NOT NULL AND customer_id=$2) OR ($2 IS NULL AND customer_id IS NULL AND lower(guest_email)=lower($3))) AND ($4::uuid IS NULL OR department_id=$4)")
                    .bind(number).bind(customer).bind(m.sender).bind(m.department).fetch_optional(&mut *tx).await.map_err(db_err)?;
            }
        }
        let guest_allowed = department_policy.as_deref() == Some("guest");
        if let Some(id) = target {
            // Re-check the merged destination, not only the original ticket.
            let allowed: Option<Uuid> = sqlx::query_scalar("SELECT id FROM tickets WHERE id=$1 AND (($2::uuid IS NOT NULL AND customer_id=$2) OR ($2 IS NULL AND customer_id IS NULL AND lower(guest_email)=lower($3))) AND ($4::uuid IS NULL OR department_id=$4) AND merged_into IS NULL FOR UPDATE")
                .bind(id).bind(customer).bind(m.sender).bind(m.department).fetch_optional(&mut *tx).await.map_err(db_err)?;
            if allowed.is_some() && (customer.is_some() || guest_allowed) {
                ticket = Some(id);
                outcome = "replied";
                sqlx::query("UPDATE tickets SET status='customer_reply', last_reply_at=now(), closed_at=NULL WHERE id=$1")
                    .bind(id).execute(&mut *tx).await.map_err(db_err)?;
            }
        } else if m.number.is_none()
            && m.references.is_empty()
            && (customer.is_some() || guest_allowed)
        {
            if let Some(dep) = m.department {
                ticket = sqlx::query_scalar("INSERT INTO tickets (customer_id, opened_by, department_id, subject, guest_email) SELECT $1,$2,id,$4,$5 FROM ticket_departments WHERE id=$3 AND is_active RETURNING tickets.id")
                    .bind(customer).bind(user).bind(dep).bind(m.subject).bind(if customer.is_none() { m.sender } else { "" })
                    .fetch_optional(&mut *tx).await.map_err(db_err)?;
                if ticket.is_some() {
                    outcome = "opened";
                }
            }
        }
        if let Some(id) = ticket {
            let mid: Uuid = sqlx::query_scalar("INSERT INTO ticket_messages (ticket_id,author_id,is_staff,body) VALUES ($1,$2,false,$3) RETURNING id")
                .bind(id).bind(user).bind(m.body).fetch_one(&mut *tx).await.map_err(db_err)?;
            for a in m.attachments {
                sqlx::query("INSERT INTO ticket_attachments (uploaded_by,ticket_id,message_id,filename,content_type,size_bytes,data) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                    .bind(user).bind(id).bind(mid).bind(&a.filename).bind(&a.content_type).bind(a.data.len() as i32).bind(&a.data)
                    .execute(&mut *tx).await.map_err(db_err)?;
            }
            crate::jobs::enqueue_in(
                &mut tx,
                INBOUND_NOTIFY_JOB,
                serde_json::json!({"ticket_id": id, "ticket_message_id": mid, "outcome": outcome}),
                &format!("support.inbound_notify:{mid}"),
                5,
            )
            .await?;
        }
    }
    sqlx::query("UPDATE ticket_pipe_log SET outcome=$2,ticket_id=$3 WHERE message_id=$1")
        .bind(m.message_id)
        .bind(outcome)
        .bind(ticket)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    tx.commit().await.map_err(db_err)?;
    Ok((outcome, ticket))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_references_are_strict_routing_hints() {
        let ticket = Uuid::new_v4();
        let delivery = Uuid::new_v4();
        let valid = format!("ticket.{ticket}.{delivery}@pelty.invalid");
        assert_eq!(outgoing_ticket_reference(&valid), Some(ticket));
        for reference in [
            format!("ticket.{ticket}.delivery@pelty.invalid"),
            format!("ticket.{ticket}.{delivery}@pelty.invalid.evil"),
            format!("ticket.{ticket}.{delivery}.extra@pelty.invalid"),
            format!("ticket.{ticket}.{}@pelty.invalid", Uuid::nil()),
            format!("ticket.{}.{delivery}@pelty.invalid", Uuid::nil()),
            format!("<ticket.{ticket}.{delivery}@pelty.invalid>"),
            "ticket.invalid.invalid@pelty.invalid".into(),
            "anything@example.test".into(),
        ] {
            assert_eq!(outgoing_ticket_reference(&reference), None, "{reference}");
        }
    }

    fn r(kind: &str, v: &str) -> SpamRule {
        SpamRule {
            id: Uuid::nil(),
            kind: kind.into(),
            value: v.into(),
            hits: 0,
            created_at: OffsetDateTime::UNIX_EPOCH,
        }
    }
    #[test]
    fn spam_rules_match() {
        let rules = vec![
            r("sender", "bad@x.io"),
            r("domain", "spam.test"),
            r("subject", "viagra"),
            r("phrase", "crypto giveaway"),
        ];
        assert_eq!(
            spam_match(&rules, "BAD@x.io", "hi", "").unwrap().kind,
            "sender"
        );
        assert_eq!(
            spam_match(&rules, "a@mail.spam.test", "hi", "")
                .unwrap()
                .kind,
            "domain"
        );
        assert!(spam_match(&rules, "a@notspam.test", "hi", "").is_none());
        assert_eq!(
            spam_match(&rules, "a@ok.io", "Cheap VIAGRA", "")
                .unwrap()
                .kind,
            "subject"
        );
        assert_eq!(
            spam_match(&rules, "a@ok.io", "hello", "Join our Crypto Giveaway now")
                .unwrap()
                .kind,
            "phrase"
        );
        assert!(spam_match(&rules, "a@ok.io", "help", "server down").is_none());
    }
}
