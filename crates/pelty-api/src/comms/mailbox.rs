//! Mailbox fetchers for ticket piping: minimal POP3 (RFC 1939) and IMAP4rev1
//! (RFC 3501) clients plus a small RFC 5322/MIME reader. The clients run over
//! any async stream, so tests drive them with an in-memory scripted server;
//! production connects with [`TcpConnector`] (TLS via rustls + webpki roots).
use crate::pipe::{BoxFuture, InboundMail, MailSource};
use base64::Engine;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

pub trait MailStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> MailStream for T {}

/// Opens the byte stream to a mail server.
pub trait MailboxConnector: Send + Sync {
    fn connect<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        tls: bool,
    ) -> BoxFuture<'a, Result<Box<dyn MailStream>, String>>;
}

/// Real network connector (plain TCP or implicit TLS).
pub struct TcpConnector;

impl MailboxConnector for TcpConnector {
    fn connect<'a>(
        &'a self,
        host: &'a str,
        port: u16,
        tls: bool,
    ) -> BoxFuture<'a, Result<Box<dyn MailStream>, String>> {
        Box::pin(async move {
            let tcp = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                tokio::net::TcpStream::connect((host, port)),
            )
            .await
            .map_err(|_| "connect timeout".to_string())?
            .map_err(|e| e.to_string())?;
            if !tls {
                return Ok(Box::new(tcp) as Box<dyn MailStream>);
            }
            let roots = tokio_rustls::rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            let cfg = tokio_rustls::rustls::ClientConfig::builder_with_provider(Arc::new(
                tokio_rustls::rustls::crypto::ring::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
            let name = tokio_rustls::rustls::pki_types::ServerName::try_from(host.to_string())
                .map_err(|e| e.to_string())?;
            let s = tokio_rustls::TlsConnector::from(Arc::new(cfg))
                .connect(name, tcp)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Box::new(s) as Box<dyn MailStream>)
        })
    }
}

#[derive(Debug, Clone)]
pub struct MailboxConfig {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub tls: bool,
    pub username: String,
    pub password: String,
    /// Department piping address, used as `to` when a message lacks one.
    pub address: String,
}

/// Most messages pulled per run (the rest wait for the next run).
pub const MAX_PER_RUN: usize = 50;
const MAX_LINE: usize = 64 * 1024;
pub const MAX_MESSAGE: usize = 32 * 1024 * 1024;

struct Conn {
    r: BufReader<Box<dyn MailStream>>,
}

impl Conn {
    async fn line(&mut self) -> Result<String, String> {
        let mut buf = Vec::new();
        let n = (&mut self.r)
            .take(MAX_LINE as u64)
            .read_until(b'\n', &mut buf)
            .await
            .map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed".into());
        }
        Ok(String::from_utf8_lossy(&buf)
            .trim_end_matches(['\r', '\n'])
            .to_string())
    }
    async fn send(&mut self, s: &str) -> Result<(), String> {
        let w = self.r.get_mut();
        w.write_all(s.as_bytes()).await.map_err(|e| e.to_string())?;
        w.write_all(b"\r\n").await.map_err(|e| e.to_string())?;
        w.flush().await.map_err(|e| e.to_string())
    }
}

use tokio::io::AsyncReadExt;

fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// POP3: USER/PASS, LIST, RETR + DELE each message, QUIT (deletes commit).
pub async fn pop3_fetch(
    stream: Box<dyn MailStream>,
    user: &str,
    pass: &str,
) -> Result<Vec<String>, String> {
    let mut c = Conn {
        r: BufReader::new(stream),
    };
    let ok = |l: &str| -> Result<(), String> {
        if l.starts_with("+OK") {
            Ok(())
        } else {
            Err(format!("pop3: {}", l.chars().take(200).collect::<String>()))
        }
    };
    ok(&c.line().await?)?;
    c.send(&format!("USER {user}")).await?;
    ok(&c.line().await?)?;
    c.send(&format!("PASS {pass}")).await?;
    ok(&c.line().await?).map_err(|_| "pop3: login failed".to_string())?;
    c.send("LIST").await?;
    ok(&c.line().await?)?;
    let mut ids = Vec::new();
    loop {
        let l = c.line().await?;
        if l == "." {
            break;
        }
        if let Some(n) = l
            .split_whitespace()
            .next()
            .and_then(|x| x.parse::<u32>().ok())
        {
            ids.push(n);
        }
    }
    let mut out = Vec::new();
    for id in ids.into_iter().take(MAX_PER_RUN) {
        c.send(&format!("RETR {id}")).await?;
        ok(&c.line().await?)?;
        let mut msg = String::new();
        loop {
            let l = c.line().await?;
            if l == "." {
                break;
            }
            let l = l
                .strip_prefix('.')
                .map(|x| if l.starts_with("..") { x } else { &l[..] })
                .unwrap_or(&l);
            if msg.len() + l.len() + 2 > MAX_MESSAGE {
                return Err("pop3: message too large".into());
            }
            msg.push_str(l);
            msg.push_str("\r\n");
        }
        out.push(msg);
        c.send(&format!("DELE {id}")).await?;
        ok(&c.line().await?)?;
    }
    c.send("QUIT").await?;
    let _ = c.line().await;
    Ok(out)
}

fn uid_validity(line: &str) -> Option<u64> {
    line.to_ascii_uppercase()
        .split_once("[UIDVALIDITY ")?
        .1
        .split(']')
        .next()?
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .map(u64::from)
}

/// IMAP: LOGIN, SELECT INBOX, SEARCH UNSEEN, FETCH BODY[] (marks \Seen), LOGOUT.
pub async fn imap_fetch(
    stream: Box<dyn MailStream>,
    user: &str,
    pass: &str,
) -> Result<Vec<String>, String> {
    Ok(imap_fetch_inner(stream, user, pass, false)
        .await?
        .into_iter()
        .map(|(_, raw, _)| raw)
        .collect())
}

async fn imap_fetch_inner(
    stream: Box<dyn MailStream>,
    user: &str,
    pass: &str,
    pending: bool,
) -> Result<Vec<(u32, String, Option<u64>)>, String> {
    let mut c = Conn {
        r: BufReader::new(stream),
    };
    let greet = c.line().await?;
    if !greet.starts_with("* OK") {
        return Err(format!("imap: {greet}"));
    }
    let mut tag = 0u32;
    async fn cmd(c: &mut Conn, tag: &mut u32, s: &str) -> Result<Vec<String>, String> {
        *tag += 1;
        let t = format!("a{tag}");
        c.send(&format!("{t} {s}")).await?;
        let mut untagged = Vec::new();
        loop {
            let l = c.line().await?;
            if let Some(rest) = l.strip_prefix(&format!("{t} ")) {
                return if rest.starts_with("OK") {
                    Ok(untagged)
                } else {
                    Err(format!(
                        "imap: {}",
                        rest.chars().take(200).collect::<String>()
                    ))
                };
            }
            untagged.push(l);
        }
    }
    cmd(
        &mut c,
        &mut tag,
        &format!("LOGIN {} {}", quote(user), quote(pass)),
    )
    .await
    .map_err(|_| "imap: login failed".to_string())?;
    let selected = cmd(&mut c, &mut tag, "SELECT INBOX").await?;
    let validity = selected.iter().find_map(|line| uid_validity(line));
    if pending && validity.is_none() {
        return Err("imap: missing UIDVALIDITY".into());
    }
    let search = cmd(
        &mut c,
        &mut tag,
        if pending {
            "UID SEARCH UNSEEN"
        } else {
            "SEARCH UNSEEN"
        },
    )
    .await?;
    let ids: Vec<u32> = search
        .iter()
        .filter_map(|l| l.strip_prefix("* SEARCH"))
        .flat_map(|x| {
            x.split_whitespace()
                .filter_map(|n| n.parse().ok())
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = Vec::new();
    for id in ids.into_iter().take(MAX_PER_RUN) {
        tag += 1;
        let t = format!("a{tag}");
        c.send(&if pending {
            format!("{t} UID FETCH {id} BODY.PEEK[]")
        } else {
            format!("{t} FETCH {id} BODY[]")
        })
        .await?;
        let mut msg = String::new();
        loop {
            let l = c.line().await?;
            if let Some(rest) = l.strip_prefix(&format!("{t} ")) {
                if !rest.starts_with("OK") {
                    return Err(format!("imap: {rest}"));
                }
                break;
            }
            // `* n FETCH (BODY[] {size}` then exactly `size` bytes.
            if let Some(size) = l
                .strip_suffix('}')
                .and_then(|x| x.rsplit_once('{'))
                .and_then(|(_, n)| n.parse::<usize>().ok())
            {
                if size > MAX_MESSAGE {
                    return Err("imap: message too large".into());
                }
                let mut buf = vec![0u8; size];
                c.r.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
                msg = String::from_utf8_lossy(&buf).to_string();
            }
        }
        out.push((id, msg, validity));
    }
    let _ = cmd(&mut c, &mut tag, "LOGOUT").await;
    Ok(out)
}

// ---- RFC 5322 / MIME ----

fn unfold_headers(head: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in head.split("\r\n").flat_map(|x| x.split('\n')) {
        if let (true, Some(last)) = (l.starts_with(' ') || l.starts_with('\t'), out.last_mut()) {
            last.1.push(' ');
            last.1.push_str(l.trim());
        } else if let Some((k, v)) = l.split_once(':') {
            out.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    out
}

fn split_head(raw: &str) -> (&str, &str) {
    if let Some(i) = raw.find("\r\n\r\n") {
        (&raw[..i], &raw[i + 4..])
    } else if let Some(i) = raw.find("\n\n") {
        (&raw[..i], &raw[i + 2..])
    } else {
        (raw, "")
    }
}

fn header<'a>(h: &'a [(String, String)], k: &str) -> &'a str {
    h.iter()
        .find(|x| x.0 == k)
        .map(|x| x.1.as_str())
        .unwrap_or("")
}

fn param(v: &str, name: &str) -> Option<String> {
    v.split(';').skip(1).find_map(|p| {
        let (k, val) = p.split_once('=')?;
        (k.trim().eq_ignore_ascii_case(name)).then(|| val.trim().trim_matches('"').to_string())
    })
}

pub fn decode_qp(s: &str) -> String {
    String::from_utf8_lossy(&decode_qp_bytes(s)).to_string()
}

fn decode_qp_bytes(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if b.get(i + 1) == Some(&b'\r') && b.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if b.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if let (Some(h), Some(l)) = (b.get(i + 1), b.get(i + 2)) {
                if let Ok(v) = u8::from_str_radix(&format!("{}{}", *h as char, *l as char), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn decode_body(body: &str, enc: &str) -> String {
    match enc.to_ascii_lowercase().as_str() {
        "base64" => {
            let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
            base64::engine::general_purpose::STANDARD
                .decode(clean)
                .map(|b| String::from_utf8_lossy(&b).to_string())
                .unwrap_or_default()
        }
        "quoted-printable" => decode_qp(body),
        _ => body.to_string(),
    }
}

/// RFC 2047 encoded-words (`=?utf-8?B?...?=` / `?Q?`) in headers.
pub fn decode_words(v: &str) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("=?") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        let parts: Vec<&str> = tail.splitn(3, '?').collect();
        if parts.len() == 3 {
            if let Some(end) = parts[2].find("?=") {
                let text = &parts[2][..end];
                let dec = match parts[1].to_ascii_uppercase().as_str() {
                    "B" => base64::engine::general_purpose::STANDARD
                        .decode(text)
                        .map(|b| String::from_utf8_lossy(&b).to_string())
                        .unwrap_or_default(),
                    _ => decode_qp(&text.replace('_', " ")),
                };
                out.push_str(&dec);
                let consumed = 2 + parts[0].len() + 1 + parts[1].len() + 1 + end + 2;
                rest = &rest[i + consumed..];
                continue;
            }
        }
        out.push_str("=?");
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn text_part(headers: &[(String, String)], body: &str, depth: u8) -> Option<String> {
    let ct = header(headers, "content-type");
    let ctl = ct.to_ascii_lowercase();
    let disposition = header(headers, "content-disposition");
    if disposition.to_ascii_lowercase().starts_with("attachment")
        || param(disposition, "filename").is_some()
        || param(ct, "name").is_some()
    {
        return None;
    }
    if !ctl.is_empty() && !ctl.starts_with("text/") && !ctl.starts_with("multipart/") {
        return None;
    }
    if ctl.starts_with("multipart/") && depth < 5 {
        let boundary = param(ct, "boundary")?;
        let delim = format!("--{boundary}");
        let mut html = None;
        for part in body.split(&delim).skip(1) {
            if part.starts_with("--") {
                break;
            }
            let part = part.trim_start_matches(['\r', '\n']);
            let (h, b) = split_head(part);
            let ph = unfold_headers(h);
            let pct = header(&ph, "content-type").to_ascii_lowercase();
            if header(&ph, "content-disposition")
                .to_ascii_lowercase()
                .starts_with("attachment")
            {
                continue;
            }
            if pct.is_empty() || pct.starts_with("text/plain") || pct.starts_with("multipart/") {
                if let Some(t) = text_part(&ph, b, depth + 1) {
                    return Some(t);
                }
            } else if pct.starts_with("text/html") && html.is_none() {
                html = Some(strip_html(&decode_body(
                    b,
                    header(&ph, "content-transfer-encoding"),
                )));
            }
        }
        return html;
    }
    let dec = decode_body(body, header(headers, "content-transfer-encoding"));
    if ctl.starts_with("text/html") {
        return Some(strip_html(&dec));
    }
    Some(dec)
}

fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

pub fn message_references(value: &str) -> Vec<String> {
    value
        .split_whitespace()
        .filter_map(|v| {
            let id = v.trim_matches(['<', '>']);
            (!id.is_empty() && id.len() <= 900).then(|| id.to_string())
        })
        .take(100)
        .collect()
}

fn collect_attachments(
    headers: &[(String, String)],
    body: &str,
    depth: u8,
    out: &mut Vec<crate::pipe::MailAttachment>,
) -> Result<(), ()> {
    if depth > 8 {
        return Err(());
    }
    let ct = header(headers, "content-type");
    if ct.to_ascii_lowercase().starts_with("multipart/") {
        let boundary = param(ct, "boundary").filter(|b| !b.is_empty()).ok_or(())?;
        let delimiter = format!("--{boundary}");
        let mut closed = false;
        for part in body.split(&delimiter).skip(1) {
            if part.starts_with("--") {
                closed = true;
                break;
            }
            let (h, b) = split_head(part.trim_start_matches(['\r', '\n']));
            collect_attachments(
                &unfold_headers(h),
                b.trim_end_matches(['\r', '\n']),
                depth + 1,
                out,
            )?;
        }
        return if closed { Ok(()) } else { Err(()) };
    }
    let disposition = header(headers, "content-disposition");
    let filename = param(disposition, "filename").or_else(|| param(ct, "name"));
    if filename.is_some() || disposition.to_ascii_lowercase().starts_with("attachment") {
        if out.len() >= pelty_database::ticket_attachments::MAX_PER_MESSAGE {
            return Err(());
        }
        let data = match header(headers, "content-transfer-encoding")
            .to_ascii_lowercase()
            .as_str()
        {
            "base64" => base64::engine::general_purpose::STANDARD
                .decode(
                    body.chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<String>(),
                )
                .map_err(|_| ())?,
            "quoted-printable" => decode_qp_bytes(body),
            "" | "7bit" | "8bit" => body.as_bytes().to_vec(),
            _ => return Err(()),
        };
        out.push(crate::pipe::MailAttachment {
            filename: decode_words(&filename.unwrap_or_else(|| "file".into())),
            content_type: ct
                .split(';')
                .next()
                .unwrap_or("application/octet-stream")
                .trim()
                .to_ascii_lowercase(),
            data,
        });
    }
    Ok(())
}

/// Parse a raw message into the fields piping needs.
pub fn parse_message(raw: &str, fallback_to: &str) -> InboundMail {
    let (h, b) = split_head(raw);
    let headers = unfold_headers(h);
    let mut message_id = header(&headers, "message-id")
        .trim_matches(['<', '>'])
        .to_string();
    if message_id.is_empty() {
        use sha2::Digest;
        message_id = format!(
            "sha256-{}",
            sha2::Sha256::digest(raw.as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
    }
    let to = header(&headers, "to");
    let mut attachments = Vec::new();
    let invalid_mime = collect_attachments(&headers, b, 0, &mut attachments).is_err();
    let references = message_references(header(&headers, "in-reply-to"))
        .into_iter()
        .chain(
            message_references(header(&headers, "references"))
                .into_iter()
                .rev(),
        )
        .take(100)
        .collect();
    let auto = header(&headers, "auto-submitted");
    let precedence = header(&headers, "precedence").to_ascii_lowercase();
    InboundMail {
        message_id: message_id.chars().take(900).collect(),
        from: decode_words(header(&headers, "from")),
        to: if !fallback_to.is_empty() {
            fallback_to.to_string()
        } else {
            decode_words(to)
        },
        subject: decode_words(header(&headers, "subject")),
        body: text_part(&headers, b, 0)
            .unwrap_or_default()
            .replace("\r\n", "\n")
            .trim()
            .to_string(),
        references,
        automatic: (!auto.is_empty() && !auto.eq_ignore_ascii_case("no"))
            || matches!(precedence.trim(), "bulk" | "junk" | "list")
            || !header(&headers, "x-auto-response-suppress").is_empty()
            || header(&headers, "content-type")
                .to_ascii_lowercase()
                .starts_with("multipart/report"),
        attachments,
        invalid_mime,
        remote_uid: None,
        remote_uid_validity: None,
    }
}

/// A department mailbox as a [`MailSource`].
pub struct MailboxSource {
    pub connector: Arc<dyn MailboxConnector>,
    pub cfg: MailboxConfig,
}

impl MailSource for MailboxSource {
    fn fetch(&self) -> BoxFuture<'_, Result<Vec<InboundMail>, String>> {
        Box::pin(async move {
            let s = self
                .connector
                .connect(&self.cfg.host, self.cfg.port, self.cfg.tls)
                .await?;
            if self.cfg.protocol == "imap" {
                let raws =
                    imap_fetch_inner(s, &self.cfg.username, &self.cfg.password, true).await?;
                return Ok(raws
                    .into_iter()
                    .map(|(uid, raw, validity)| {
                        let mut m = parse_message(&raw, &self.cfg.address);
                        m.remote_uid = Some(uid);
                        m.remote_uid_validity = validity;
                        m
                    })
                    .collect());
            }
            let raws = match self.cfg.protocol.as_str() {
                "pop3" => pop3_fetch(s, &self.cfg.username, &self.cfg.password).await?,
                p => return Err(format!("unsupported protocol {p}")),
            };
            Ok(raws
                .iter()
                .map(|r| parse_message(r, &self.cfg.address))
                .collect())
        })
    }
    fn acknowledge<'a>(&'a self, mail: &'a InboundMail) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(uid) = mail.remote_uid else {
                return Ok(());
            };
            let expected_validity = mail
                .remote_uid_validity
                .filter(|v| (1..=u64::from(u32::MAX)).contains(v))
                .ok_or("imap: missing original UIDVALIDITY")?;
            let mut selected_validity = None;
            let stream = self
                .connector
                .connect(&self.cfg.host, self.cfg.port, self.cfg.tls)
                .await?;
            let mut c = Conn {
                r: BufReader::new(stream),
            };
            if !c.line().await?.starts_with("* OK") {
                return Err("imap: invalid greeting".into());
            }
            let commands = [
                format!(
                    "LOGIN {} {}",
                    quote(&self.cfg.username),
                    quote(&self.cfg.password)
                ),
                "SELECT INBOX".into(),
                format!("UID STORE {uid} +FLAGS.SILENT (\\Seen)"),
                "LOGOUT".into(),
            ];
            for (i, command) in commands.iter().enumerate() {
                let tag = format!("a{}", i + 1);
                c.send(&format!("{tag} {command}")).await?;
                loop {
                    let line = c.line().await?;
                    if i == 1 && line.to_ascii_uppercase().contains("[UIDVALIDITY") {
                        let validity = uid_validity(&line)
                            .ok_or("imap: malformed SELECT UIDVALIDITY; retry polling")?;
                        if validity != expected_validity {
                            return Err("imap: UIDVALIDITY changed; retry polling".into());
                        }
                        selected_validity = Some(validity);
                    }
                    if let Some(rest) = line.strip_prefix(&format!("{tag} ")) {
                        if !rest.starts_with("OK") {
                            return Err("imap: acknowledgment failed".into());
                        }
                        if i == 1 && selected_validity != Some(expected_validity) {
                            return Err("imap: missing SELECT UIDVALIDITY; retry polling".into());
                        }
                        break;
                    }
                }
            }
            Ok(())
        })
    }
}

/// Greeting plus `(expected command prefix, reply)` steps.
pub type Script = (String, Vec<(String, String)>);

/// Test connector: each `connect` gets an in-memory stream answered by a
/// scripted server (`(expected command prefix, reply)` pairs after a greeting).
#[derive(Clone, Default)]
pub struct ScriptedConnector {
    pub script: Arc<std::sync::Mutex<Option<Script>>>,
    pub seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ScriptedConnector {
    pub fn set(&self, greeting: &str, steps: Vec<(&str, String)>) {
        if let Ok(mut g) = self.script.lock() {
            *g = Some((
                greeting.to_string(),
                steps.into_iter().map(|(a, b)| (a.to_string(), b)).collect(),
            ));
        }
    }
}

impl MailboxConnector for ScriptedConnector {
    fn connect<'a>(
        &'a self,
        _host: &'a str,
        _port: u16,
        _tls: bool,
    ) -> BoxFuture<'a, Result<Box<dyn MailStream>, String>> {
        Box::pin(async move {
            let (greeting, steps) = self
                .script
                .lock()
                .ok()
                .and_then(|mut g| g.take())
                .ok_or("no script")?;
            let (client, server) = tokio::io::duplex(1 << 20);
            let seen = self.seen.clone();
            tokio::spawn(async move {
                let (rd, mut wr) = tokio::io::split(server);
                let mut rd = BufReader::new(rd);
                let _ = wr.write_all(greeting.as_bytes()).await;
                for (expect, reply) in steps {
                    let mut l = String::new();
                    if rd.read_line(&mut l).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let l = l.trim_end().to_string();
                    if let Ok(mut s) = seen.lock() {
                        s.push(l.clone());
                    }
                    let reply = if l.starts_with(&expect) {
                        reply
                    } else {
                        format!("-ERR unexpected {l}\r\n")
                    };
                    let _ = wr.write_all(reply.as_bytes()).await;
                }
            });
            Ok(Box::new(client) as Box<dyn MailStream>)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_fixtures() {
        let mixed = parse_message(
            include_str!("../../tests/fixtures/inbound-mixed.eml"),
            "desk@example.test",
        );
        assert_eq!(mixed.to, "desk@example.test");
        assert_eq!(mixed.subject, "Server help");
        assert_eq!(crate::pipe::strip_quoted(&mixed.body), "Please help!");
        assert_eq!(mixed.attachments.len(), 1);
        assert_eq!(
            mixed.attachments[0].data,
            b"log line
"
        );
        assert!(!mixed.invalid_mime);
        let reply = parse_message(include_str!("../../tests/fixtures/inbound-reply.eml"), "");
        assert_eq!(reply.references[0], "fixture-mixed@example.test");
        assert_eq!(crate::pipe::strip_quoted(&reply.body), "Still broken.");
        assert!(parse_message(include_str!("../../tests/fixtures/inbound-auto.eml"), "").automatic);
        assert!(
            parse_message(
                "Precedence: list

Hello",
                ""
            )
            .automatic
        );
        assert!(
            parse_message(
                "Content-Type: multipart/mixed; boundary=x

--x

truncated",
                ""
            )
            .invalid_mime
        );
    }

    const MSG: &str = "Message-ID: <abc@x>\r\nFrom: =?utf-8?B?QW7EgyBQb3A=?= <ana@example.test>\r\nTo: support@host.test\r\nSubject: Re: [1001]\r\n Server down\r\nContent-Type: multipart/alternative; boundary=\"b1\"\r\n\r\n--b1\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nHello =C8=99i\r\nline2=\r\n continued\r\n--b1\r\nContent-Type: text/html\r\n\r\n<p>x</p>\r\n--b1--\r\n";

    #[test]
    fn parses_mime() {
        let m = parse_message(MSG, "fallback@x");
        assert_eq!(m.message_id, "abc@x");
        assert_eq!(m.from, "Ană Pop <ana@example.test>");
        assert_eq!(m.subject, "Re: [1001] Server down");
        assert_eq!(m.body, "Hello și\nline2 continued");
        let m = parse_message(
            "From: a@b\r\nContent-Type: text/html\r\n\r\n<b>Hi</b> &amp; bye",
            "dep@x",
        );
        assert_eq!(m.body, "Hi & bye");
        assert_eq!(m.to, "dep@x");
        assert!(m.message_id.starts_with("sha256-"));
        let b64 = "Subject: x\r\nContent-Transfer-Encoding: base64\r\n\r\nSGVsbG8gd29ybGQ=\r\n";
        assert_eq!(parse_message(b64, "").body, "Hello world");
    }

    #[tokio::test]
    async fn pop3_session() {
        let c = ScriptedConnector::default();
        let body = MSG.replace("\r\n.", "\r\n..");
        c.set(
            "+OK ready\r\n",
            vec![
                ("USER u", "+OK\r\n".into()),
                ("PASS p", "+OK logged in\r\n".into()),
                ("LIST", "+OK 1 messages\r\n1 300\r\n.\r\n".into()),
                ("RETR 1", format!("+OK\r\n{body}.\r\n")),
                ("DELE 1", "+OK\r\n".into()),
                ("QUIT", "+OK bye\r\n".into()),
            ],
        );
        let src = MailboxSource {
            connector: Arc::new(c.clone()),
            cfg: MailboxConfig {
                protocol: "pop3".into(),
                host: "h".into(),
                port: 995,
                tls: true,
                username: "u".into(),
                password: "p".into(),
                address: "support@host.test".into(),
            },
        };
        let got = src.fetch().await.unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].subject, "Re: [1001] Server down");
        assert_eq!(c.seen.lock().unwrap().last().unwrap(), "QUIT");
        // Bad password.
        c.set(
            "+OK\r\n",
            vec![("USER", "+OK\r\n".into()), ("PASS", "-ERR auth\r\n".into())],
        );
        assert_eq!(src.fetch().await.unwrap_err(), "pop3: login failed");
    }

    #[tokio::test]
    async fn imap_session() {
        let c = ScriptedConnector::default();
        c.set(
            "* OK IMAP ready\r\n",
            vec![
                ("a1 LOGIN \"u\" \"p\\\"q\"", "a1 OK\r\n".into()),
                (
                    "a2 SELECT INBOX",
                    "* 3 EXISTS\r\n* OK [UIDVALIDITY 9] valid\r\na2 OK\r\n".into(),
                ),
                ("a3 UID SEARCH UNSEEN", "* SEARCH 2\r\na3 OK\r\n".into()),
                (
                    "a4 UID FETCH 2 BODY.PEEK[]",
                    format!("* 2 FETCH (BODY[] {{{}}}\r\n{MSG})\r\na4 OK\r\n", MSG.len()),
                ),
                ("a5 LOGOUT", "* BYE\r\na5 OK\r\n".into()),
            ],
        );
        let src = MailboxSource {
            connector: Arc::new(c.clone()),
            cfg: MailboxConfig {
                protocol: "imap".into(),
                host: "h".into(),
                port: 993,
                tls: true,
                username: "u".into(),
                password: "p\"q".into(),
                address: "".into(),
            },
        };
        let got = src.fetch().await.unwrap();
        assert_eq!(got.len(), 1, "{:?}", c.seen.lock().unwrap());
        assert_eq!(got[0].from, "Ană Pop <ana@example.test>");
        assert_eq!(got[0].body, "Hello și\nline2 continued");
        assert_eq!(got[0].remote_uid, Some(2));
        assert_eq!(got[0].remote_uid_validity, Some(9));
        assert!(!c
            .seen
            .lock()
            .unwrap()
            .iter()
            .any(|cmd| cmd.contains("STORE")));
        c.set(
            "* OK IMAP ready\r\n",
            vec![
                ("a1 LOGIN", "a1 OK\r\n".into()),
                (
                    "a2 SELECT INBOX",
                    "* OK [UIDVALIDITY 9] valid\r\na2 OK\r\n".into(),
                ),
                ("a3 UID STORE 2 +FLAGS.SILENT (\\Seen)", "a3 OK\r\n".into()),
                ("a4 LOGOUT", "* BYE\r\na4 OK\r\n".into()),
            ],
        );
        src.acknowledge(&got[0]).await.unwrap();
        c.set(
            "* OK IMAP ready\r\n",
            vec![
                ("a1 LOGIN", "a1 OK\r\n".into()),
                (
                    "a2 SELECT INBOX",
                    "* OK [UIDVALIDITY 10] changed\r\na2 OK\r\n".into(),
                ),
            ],
        );
        assert!(src
            .acknowledge(&got[0])
            .await
            .unwrap_err()
            .contains("UIDVALIDITY changed"));
        for select in [
            "a2 OK\r\n",
            "* OK [UIDVALIDITY 0] invalid\r\na2 OK\r\n",
            "* OK [UIDVALIDITY broken] invalid\r\na2 OK\r\n",
        ] {
            let before = c.seen.lock().unwrap().len();
            c.set(
                "* OK IMAP ready\r\n",
                vec![
                    ("a1 LOGIN", "a1 OK\r\n".into()),
                    ("a2 SELECT INBOX", select.into()),
                ],
            );
            assert!(src
                .acknowledge(&got[0])
                .await
                .unwrap_err()
                .contains("SELECT UIDVALIDITY"));
            assert!(!c.seen.lock().unwrap()[before..]
                .iter()
                .any(|cmd| cmd.contains("STORE")));
        }
        let mut missing_original = got[0].clone();
        missing_original.remote_uid_validity = None;
        assert!(src
            .acknowledge(&missing_original)
            .await
            .unwrap_err()
            .contains("missing original UIDVALIDITY"));
    }
}
