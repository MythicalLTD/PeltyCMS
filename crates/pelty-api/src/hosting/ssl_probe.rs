//! TLS certificate probe for client domains (WHMCS "SSL Sync").
//! Connects to port 443, verifies the chain against webpki roots and reads
//! the leaf certificate's validity window and issuer. Private and loopback
//! addresses are refused so a client domain cannot point the probe inward.
use pelty_database::ssl_checks::Probe;
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;
use tokio_rustls::rustls;

/// Days before expiry at which a valid certificate is reported as "expiring".
pub const EXPIRING_DAYS: i64 = 14;
const TIMEOUT: Duration = Duration::from_secs(8);

fn fail(status: &'static str, error: impl Into<String>) -> Probe {
    Probe {
        status,
        issuer: String::new(),
        valid_from: None,
        expires_at: None,
        error: error.into(),
    }
}

/// Validity window and issuer of a DER certificate.
pub fn parse_leaf(der_bytes: &[u8]) -> Result<(OffsetDateTime, OffsetDateTime, String), String> {
    use der::Decode;
    let c = x509_cert::Certificate::from_der(der_bytes).map_err(|e| e.to_string())?;
    let v = &c.tbs_certificate.validity;
    let ts = |t: x509_cert::time::Time| {
        OffsetDateTime::from_unix_timestamp(t.to_unix_duration().as_secs() as i64)
            .map_err(|e| e.to_string())
    };
    Ok((
        ts(v.not_before)?,
        ts(v.not_after)?,
        c.tbs_certificate.issuer.to_string(),
    ))
}

/// Status for a verified certificate's window at `now`.
pub fn classify(from: OffsetDateTime, to: OffsetDateTime, now: OffsetDateTime) -> &'static str {
    if now >= to || now < from {
        "expired"
    } else if to - now <= time::Duration::days(EXPIRING_DAYS) {
        "expiring"
    } else {
        "valid"
    }
}

pub async fn probe(host: &str, port: u16, allow_private: bool) -> Probe {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || !host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return fail("invalid", "bad host name");
    }
    let addrs =
        match tokio::time::timeout(TIMEOUT, tokio::net::lookup_host((host.as_str(), port))).await {
            Ok(Ok(a)) => a.collect::<Vec<_>>(),
            Ok(Err(e)) => return fail("unreachable", e.to_string()),
            Err(_) => return fail("unreachable", "dns timeout"),
        };
    let Some(addr) = addrs
        .into_iter()
        .find(|a| allow_private || !pelty_wasm::is_private_ip(a.ip()))
    else {
        return fail("unreachable", "no public address");
    };
    let tcp = match tokio::time::timeout(TIMEOUT, tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => return fail("unreachable", e.to_string()),
        Err(_) => return fail("unreachable", "connect timeout"),
    };
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let cfg = match rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    {
        Ok(b) => b.with_root_certificates(roots).with_no_client_auth(),
        Err(e) => return fail("invalid", e.to_string()),
    };
    let Ok(name) = rustls::pki_types::ServerName::try_from(host.clone()) else {
        return fail("invalid", "bad host name");
    };
    let tls = match tokio::time::timeout(
        TIMEOUT,
        tokio_rustls::TlsConnector::from(Arc::new(cfg)).connect(name, tcp),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            // A handshake that fails on the certificate is "invalid"; anything else is unreachable.
            let msg = e.to_string();
            let status = if msg.contains("certificate") || msg.contains("Certificate") {
                "invalid"
            } else {
                "unreachable"
            };
            return fail(status, msg);
        }
        Err(_) => return fail("unreachable", "handshake timeout"),
    };
    let Some(leaf) = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|c| c.first().cloned())
    else {
        return fail("invalid", "no certificate");
    };
    match parse_leaf(leaf.as_ref()) {
        Ok((from, to, issuer)) => Probe {
            status: classify(from, to, OffsetDateTime::now_utc()),
            issuer,
            valid_from: Some(from),
            expires_at: Some(to),
            error: String::new(),
        },
        Err(e) => fail("invalid", e),
    }
}

/// Probe up to `limit` active domains not checked in the last day.
pub async fn run(s: &crate::AppState, limit: i64) -> usize {
    let Ok(due) = pelty_database::ssl_checks::due(&s.pool, 24, limit).await else {
        return 0;
    };
    let mut n = 0;
    for (id, name) in due {
        let p = probe(&name, 443, false).await;
        if pelty_database::ssl_checks::save(&s.pool, id, &p)
            .await
            .is_ok()
        {
            n += 1;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_classifies_a_certificate() {
        let (from, to, issuer) =
            parse_leaf(include_bytes!("../../tests/fixtures/ssl_test.der")).unwrap();
        assert!(issuer.contains("pelty-test"), "{issuer}");
        assert!(to > from);
        assert_eq!(classify(from, to, from + time::Duration::days(1)), "valid");
        assert_eq!(classify(from, to, to - time::Duration::days(3)), "expiring");
        assert_eq!(classify(from, to, to + time::Duration::days(1)), "expired");
        assert!(parse_leaf(b"nope").is_err());
    }
}
