//! Shared inbound email intake for department mailboxes, signed webhooks and
//! local MTA pipes. All transports use the same sender and upload policy.
use crate::AppState;
use pelty_database::{support, support_ext as db, ticket_attachments};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use uuid::Uuid;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, Default)]
pub struct InboundMail {
    pub message_id: String,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub body: String,
    pub references: Vec<String>,
    pub automatic: bool,
    pub attachments: Vec<MailAttachment>,
    pub invalid_mime: bool,
    pub remote_uid: Option<u32>,
    pub remote_uid_validity: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct MailAttachment {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>,
}

pub trait MailSource: Send + Sync {
    /// Fetch unseen messages. IMAP acknowledgment is deferred until the
    /// ticket and its notification outbox job have committed.
    fn fetch(&self) -> BoxFuture<'_, Result<Vec<InboundMail>, String>>;
    fn acknowledge<'a>(&'a self, _mail: &'a InboundMail) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async { Ok(()) })
    }
}

#[derive(Default)]
pub struct MockSource(pub Mutex<Vec<InboundMail>>);

impl MailSource for MockSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Vec<InboundMail>, String>> {
        let v = self
            .0
            .lock()
            .map(|mut g| std::mem::take(&mut *g))
            .map_err(|_| "poisoned".to_string());
        Box::pin(async move { v })
    }
}

/// Ticket number from a subject: `[1234]` (our notification format) or
/// `Ticket #1234`.
pub fn ticket_number(subject: &str) -> Option<i64> {
    let mut rest = subject;
    while let Some(i) = rest.find('[') {
        let tail = &rest[i + 1..];
        if let Some(j) = tail.find(']') {
            let inner = tail[..j].trim().trim_start_matches('#');
            if !inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()) {
                return inner.parse().ok();
            }
        }
        rest = tail;
    }
    let lower = subject.to_lowercase();
    let i = lower.find("ticket #")?;
    let digits: String = subject[i + 8..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

/// Drop quoted history (`>` lines and everything after an "On … wrote:").
pub fn strip_quoted(body: &str) -> String {
    let mut out = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if t == "-- "
            || t == "--"
            || t.eq_ignore_ascii_case("-----Original Message-----")
            || t.starts_with("________________________________")
            || (t.starts_with("On ") && t.trim_end().ends_with("wrote:"))
        {
            break;
        }
        if t.starts_with('>') {
            continue;
        }
        out.push(line);
    }
    out.join("\n").trim().chars().take(20000).collect()
}

/// Bare address from `Name <a@b>`.
pub fn address(v: &str) -> String {
    match (v.find('<'), v.rfind('>')) {
        (Some(a), Some(b)) if b > a => v[a + 1..b].trim().to_string(),
        _ => v.trim().to_string(),
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PipeStats {
    pub opened: usize,
    pub replied: usize,
    pub rejected: usize,
    pub duplicate: usize,
    pub spam: usize,
}

/// Fetch a batch; the database commits each message atomically.
pub async fn process(s: &AppState, src: &dyn MailSource) -> Result<PipeStats, String> {
    process_source(s, src, None).await
}

/// The configured mailbox department is authoritative, not the To header or
/// another department that happens to share its address.
pub async fn process_in_department(
    s: &AppState,
    src: &dyn MailSource,
    department: Uuid,
) -> Result<PipeStats, String> {
    process_source(s, src, Some(department)).await
}

async fn process_source(
    s: &AppState,
    src: &dyn MailSource,
    department: Option<Uuid>,
) -> Result<PipeStats, String> {
    let mut total = PipeStats::default();
    for m in src.fetch().await? {
        let st = process_messages_bound(s, vec![m.clone()], department).await?;
        src.acknowledge(&m).await?;
        total.opened += st.opened;
        total.replied += st.replied;
        total.rejected += st.rejected;
        total.duplicate += st.duplicate;
        total.spam += st.spam;
    }
    Ok(total)
}

pub async fn process_messages(s: &AppState, mails: Vec<InboundMail>) -> Result<PipeStats, String> {
    process_messages_bound(s, mails, None).await
}

/// Use this for authenticated webhooks and local MTA delivery. The UUID is
/// carried unchanged into the transaction, where active status is rechecked.
pub async fn process_messages_in_department(
    s: &AppState,
    mails: Vec<InboundMail>,
    department: Uuid,
) -> Result<PipeStats, String> {
    process_messages_bound(s, mails, Some(department)).await
}

async fn process_messages_bound(
    s: &AppState,
    mails: Vec<InboundMail>,
    department: Option<Uuid>,
) -> Result<PipeStats, String> {
    let mut st = PipeStats::default();
    let rules = db::spam_rules(&s.pool).await.map_err(|e| e.to_string())?;
    let limit = pelty_database::general_settings::load(&s.pool)
        .await
        .map_err(|e| e.to_string())?
        .support
        .max_bytes();
    for m in mails {
        let from = address(&m.from).to_ascii_lowercase();
        let subject: String = m.subject.trim().chars().take(200).collect();
        let subject = if subject.is_empty() {
            "(no subject)"
        } else {
            &subject
        };
        let message_id = m.message_id.trim().trim_matches(['<', '>']);
        if message_id.is_empty() || message_id.len() > 900 {
            st.rejected += 1;
            continue;
        }
        let body = strip_quoted(&m.body);
        let dep = match department {
            Some(id) => Some(id),
            None => support::department_by_email(&s.pool, &address(&m.to))
                .await
                .map_err(|e| e.to_string())?,
        };
        let customer = support::customer_by_email(&s.pool, &from)
            .await
            .map_err(|e| e.to_string())?;
        let spam = db::spam_match(&rules, &from, subject, &m.body);
        let self_sender = support::department_by_email(&s.pool, &from)
            .await
            .map_err(|e| e.to_string())?
            .is_some();
        let mut reject = m.automatic
            || self_sender
            || m.invalid_mime
            || body.is_empty()
            || !crate::routes::support_ext::valid_email(&from)
            || m.attachments.len() > ticket_attachments::MAX_PER_MESSAGE;
        let mut files = Vec::new();
        for a in m.attachments {
            match crate::routes::ticket_attachments::sniff(Some(&a.content_type), &a.data) {
                Ok(kind) if a.data.len() <= limit => files.push(db::InboundAttachment {
                    filename: crate::routes::ticket_attachments::clean_name(&a.filename),
                    content_type: kind.to_string(),
                    data: a.data,
                }),
                _ => {
                    reject = true;
                    break;
                }
            }
        }
        let (outcome, _) = db::ingest_email(
            &s.pool,
            db::InboundTicket {
                message_id,
                sender: &from,
                subject,
                body: &body,
                department: dep,
                customer,
                number: ticket_number(subject),
                references: &m.references,
                reject,
                spam: spam.is_some(),
                attachments: &files,
            },
        )
        .await
        .map_err(|e| e.to_string())?;
        match outcome {
            "duplicate" => st.duplicate += 1,
            "spam" => {
                st.spam += 1;
                if let Some(r) = spam {
                    db::spam_hit(&s.pool, r.id)
                        .await
                        .map_err(|e| e.to_string())?;
                }
            }
            "opened" => st.opened += 1,
            "replied" => st.replied += 1,
            _ => st.rejected += 1,
        }
    }
    Ok(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_numbers_and_quotes() {
        assert_eq!(ticket_number("Re: Ticket #1234 Help"), Some(1234));
        assert_eq!(ticket_number("Re: [1001] Server down"), Some(1001));
        assert_eq!(ticket_number("[#77] x"), Some(77));
        assert_eq!(ticket_number("[urgent] x"), None);
        assert_eq!(ticket_number("Order #55"), None);
        assert_eq!(ticket_number("hello"), None);
        assert_eq!(
            strip_quoted("Thanks!\n\nOn Mon, X wrote:\n> old"),
            "Thanks!"
        );
        assert_eq!(strip_quoted("a\n> b\nc"), "a\nc");
        assert_eq!(address("Jane <jane@x.io>"), "jane@x.io");
        assert_eq!(address(" j@x.io "), "j@x.io");
    }
}
