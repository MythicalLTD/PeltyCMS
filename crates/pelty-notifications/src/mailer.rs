//! SMTP mailer built on lettre: builds and sends multipart emails.
use lettre::message::{header::ContentType, Mailbox, MultiPart, SinglePart};
pub use lettre::Address;
use lettre::transport::smtp::AsyncSmtpTransport;
use lettre::{AsyncTransport, Message, Tokio1Executor};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
    /// RFC 2369 + RFC 8058 one-click unsubscribe URL (bulk mail only).
    /// When set, `List-Unsubscribe` and `List-Unsubscribe-Post` are added.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list_unsubscribe: Option<String>,
}

#[derive(Debug, Clone)]
struct ListUnsubscribe(String);
impl lettre::message::header::Header for ListUnsubscribe {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("List-Unsubscribe")
    }
    fn parse(s: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ListUnsubscribe(s.to_string()))
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), format!("<{}>", self.0))
    }
}
#[derive(Debug, Clone)]
struct ListUnsubscribePost;
impl lettre::message::header::Header for ListUnsubscribePost {
    fn name() -> lettre::message::header::HeaderName {
        lettre::message::header::HeaderName::new_from_ascii_str("List-Unsubscribe-Post")
    }
    fn parse(_: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Ok(ListUnsubscribePost)
    }
    fn display(&self) -> lettre::message::header::HeaderValue {
        lettre::message::header::HeaderValue::new(Self::name(), "List-Unsubscribe=One-Click".into())
    }
}

/// Transport metadata for ticket notifications; not part of template content.
pub struct TicketHeaders {
    pub reply_to: String,
    pub message_id: String,
}

pub trait Mailer: Send + Sync {
    fn send<'a>(&'a self, e: &'a Email) -> BoxFuture<'a, Result<(), String>>;
    fn send_ticket<'a>(&'a self, e: &'a Email, _headers: &'a TicketHeaders) -> BoxFuture<'a, Result<(), String>> {
        self.send(e)
    }
    fn name(&self) -> &'static str;
}

/// SMTP over rustls. URL forms (lettre): `smtps://user:pass@host:465`,
/// `smtp://user:pass@host:587?tls=required`, `smtp://host:25` (local relay).
pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn from_url(url: &str, from: &str) -> Result<Self, String> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .map_err(|e| format!("smtp url: {e}"))?
            .build();
        let from = from.parse().map_err(|e| format!("mail from: {e}"))?;
        Ok(SmtpMailer { transport, from })
    }
}

/// Transport security for [`SmtpMailer::from_parts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtpSecurity {
    /// Implicit TLS (usually port 465).
    Tls,
    /// STARTTLS required (usually port 587).
    StartTls,
    /// Plain text. Only for a trusted local relay.
    None,
}

impl SmtpMailer {
    pub fn from_parts(
        host: &str,
        port: u16,
        security: SmtpSecurity,
        credentials: Option<(String, String)>,
        from: &str,
        timeout_secs: u64,
    ) -> Result<Self, String> {
        use lettre::transport::smtp::authentication::Credentials;
        let b = match security {
            SmtpSecurity::Tls => AsyncSmtpTransport::<Tokio1Executor>::relay(host)
                .map_err(|e| format!("smtp: {e}"))?,
            SmtpSecurity::StartTls => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)
                .map_err(|e| format!("smtp: {e}"))?,
            SmtpSecurity::None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host),
        };
        let mut b = b
            .port(port)
            .timeout(Some(std::time::Duration::from_secs(timeout_secs)));
        if let Some((u, p)) = credentials {
            b = b.credentials(Credentials::new(u, p));
        }
        let from = from.parse().map_err(|e| format!("mail from: {e}"))?;
        Ok(SmtpMailer {
            transport: b.build(),
            from,
        })
    }
}

pub fn build_message(from: &Mailbox, e: &Email) -> Result<Message, String> {
    build_ticket_message(from, e, None)
}

pub fn build_ticket_message(from: &Mailbox, e: &Email, ticket: Option<&TicketHeaders>) -> Result<Message, String> {
    let to: Mailbox = e.to.parse().map_err(|err| format!("recipient: {err}"))?;
    // Header injection is impossible: lettre encodes header values, and we
    // also refuse control characters in the subject.
    if e.subject.chars().any(char::is_control) {
        return Err("subject contains control characters".into());
    }
    let mut b = Message::builder().from(from.clone()).to(to).subject(&e.subject)
        .raw_header(lettre::message::header::HeaderValue::new(
            lettre::message::header::HeaderName::new_from_ascii_str("Auto-Submitted"), "auto-generated".into()));
    if let Some(t) = ticket {
        b = b.reply_to(t.reply_to.parse().map_err(|e| format!("reply-to: {e}"))?)
            .message_id(Some(t.message_id.clone()));
    }
    if let Some(u) = &e.list_unsubscribe {
        if !u.starts_with("https://") && !u.starts_with("http://") || u.chars().any(|c| c.is_control() || c == '>' || c == ' ') {
            return Err("invalid unsubscribe url".into());
        }
        b = b.header(ListUnsubscribe(u.clone())).header(ListUnsubscribePost);
    }
    b.multipart(
            MultiPart::alternative()
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(e.text.clone()),
                )
                .singlepart(
                    SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(e.html.clone()),
                ),
        )
        .map_err(|err| err.to_string())
}

impl Mailer for SmtpMailer {
    fn send<'a>(&'a self, e: &'a Email) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let msg = build_message(&self.from, e)?;
            self.transport
                .send(msg)
                .await
                .map(|_| ())
                .map_err(|err| err.to_string())
        })
    }
    fn send_ticket<'a>(&'a self, e: &'a Email, headers: &'a TicketHeaders) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let msg = build_ticket_message(&self.from, e, Some(headers))?;
            self.transport.send(msg).await.map(|_| ()).map_err(|err| err.to_string())
        })
    }
    fn name(&self) -> &'static str {
        "smtp"
    }
}

/// Development fallback when SMTP isn't configured: records that a mail
/// would be sent. Bodies are never logged (they contain one-time links).
pub struct LogMailer;
impl Mailer for LogMailer {
    fn send<'a>(&'a self, e: &'a Email) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            tracing::warn!(to = %e.to, subject = %e.subject, "SMTP not configured (PELTY_SMTP_URL); email not delivered");
            Ok(())
        })
    }
    fn name(&self) -> &'static str {
        "log"
    }
}

/// Test mailer: keeps sent mail in memory.
#[derive(Default, Clone)]
pub struct MemoryMailer {
    pub sent: Arc<Mutex<Vec<Email>>>,
}
impl MemoryMailer {
    pub fn take(&self) -> Vec<Email> {
        std::mem::take(&mut *self.sent.lock().unwrap_or_else(|p| p.into_inner()))
    }
}
impl Mailer for MemoryMailer {
    fn send<'a>(&'a self, e: &'a Email) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.sent
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(e.clone());
            Ok(())
        })
    }
    fn name(&self) -> &'static str {
        "memory"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builds_multipart_and_rejects_injection() {
        let from: Mailbox = "Pelty <billing@pelty.test>".parse().unwrap();
        let e = Email {
            to: "ana@example.com".into(),
            subject: "Reset".into(),
            text: "t".into(),
            html: "<p>t</p>".into(),
            list_unsubscribe: None,
        };
        let raw = String::from_utf8(build_message(&from, &e).unwrap().formatted()).unwrap();
        assert!(raw.contains("Auto-Submitted: auto-generated"));
        let headers = TicketHeaders { reply_to: "desk@example.test".into(), message_id: "<ticket.test.delivery@pelty.invalid>".into() };
        let ticket_raw = String::from_utf8(build_ticket_message(&from, &e, Some(&headers)).unwrap().formatted()).unwrap();
        assert!(ticket_raw.contains("Reply-To: desk@example.test"));
        assert!(ticket_raw.contains("Message-ID: <ticket.test.delivery@pelty.invalid>"));
        assert!(raw.contains("multipart/alternative") && raw.contains("To: ana@example.com"));
        let bad = Email {
            subject: "x\r\nBcc: evil@x".into(),
            ..e.clone()
        };
        assert!(build_message(&from, &bad).is_err());
        let bulk = Email { list_unsubscribe: Some("https://x.test/api/v1/unsubscribe/abc".into()), ..e.clone() };
        let raw = String::from_utf8(build_message(&from, &bulk).unwrap().formatted()).unwrap();
        assert!(raw.contains("List-Unsubscribe: <https://x.test/api/v1/unsubscribe/abc>"), "{raw}");
        assert!(raw.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
        assert!(build_message(&from, &Email { list_unsubscribe: Some("javascript:x".into()), ..e.clone() }).is_err());
        assert!(build_message(
            &from,
            &Email {
                to: "not an email".into(),
                ..e
            }
        )
        .is_err());
    }
    #[test]
    fn smtp_url_parses() {
        assert!(SmtpMailer::from_url("smtp://127.0.0.1:2525", "a@b.c").is_ok());
        assert!(SmtpMailer::from_url("nope://", "a@b.c").is_err());
    }
}
