//! `import whmcs` for the CLI and the console: read a WHMCS SQL export
//! (mysqldump / phpMyAdmin, no MySQL server needed) or a live MySQL DSN,
//! decrypt server passwords with the WHMCS `cc_encryption_hash`, seal them
//! with `PELTY_ENCRYPTION_KEY` and run the idempotent importer.

use pelty_core::config::Config;
use sqlx::PgPool;

#[derive(Debug, Default, PartialEq)]
pub struct Opts {
    pub file: Option<String>,
    pub dsn: Option<String>,
    pub cc_hash: Option<String>,
    pub dry_run: bool,
}

pub const USAGE: &str = "import whmcs (--file <whmcs.sql> | --dsn mysql://user:pass@host/db) [--cc-hash <hash>] [--dry-run]";

pub fn parse_args(args: &[&str]) -> Result<Opts, String> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = || {
            it.next()
                .map(|s| s.to_string())
                .ok_or(format!("{a} needs a value; usage: {USAGE}"))
        };
        match *a {
            "--dry-run" => o.dry_run = true,
            "--file" => o.file = Some(val()?),
            "--dsn" => o.dsn = Some(val()?),
            "--cc-hash" => o.cc_hash = Some(val()?),
            other => return Err(format!("unknown import option {other}; usage: {USAGE}")),
        }
    }
    if o.dsn.is_none() && o.file.is_none() {
        // WHMCS_DSN: MySQL URL of the WHMCS database for `import whmcs` (instead of --dsn).
        o.dsn = std::env::var("WHMCS_DSN").ok().filter(|s| !s.is_empty());
    }
    if o.cc_hash.is_none() {
        // WHMCS_CC_ENCRYPTION_HASH: `$cc_encryption_hash` from WHMCS configuration.php, used by `import whmcs` to decrypt server passwords (safer than --cc-hash, which shows up in the process list).
        o.cc_hash = std::env::var("WHMCS_CC_ENCRYPTION_HASH")
            .ok()
            .filter(|s| !s.is_empty());
    }
    match (&o.file, &o.dsn) {
        (None, None) => Err(format!(
            "missing --file or --dsn (or WHMCS_DSN); usage: {USAGE}"
        )),
        (Some(_), Some(_)) => Err("use either --file or --dsn, not both".into()),
        _ => Ok(o),
    }
}

async fn load(o: &Opts) -> Result<pelty_import::WhmcsData, String> {
    if let Some(f) = &o.file {
        let bytes = std::fs::read(f).map_err(|e| format!("{f}: {e}"))?;
        // Old dumps are often latin1; keep going with replacement chars.
        let sql = String::from_utf8_lossy(&bytes);
        let t = pelty_import::sqldump::parse(&sql).map_err(|e| format!("{f}: {e}"))?;
        if !t.has("tblclients") {
            return Err(format!(
                "{f}: no tblclients table found; is this a WHMCS export?"
            ));
        }
        return Ok(pelty_import::WhmcsData::from_tables(&t));
    }
    load_mysql(o.dsn.as_deref().unwrap_or_default()).await
}

#[cfg(feature = "whmcs-mysql")]
async fn load_mysql(dsn: &str) -> Result<pelty_import::WhmcsData, String> {
    pelty_import::whmcs::load_mysql(dsn)
        .await
        .map_err(|e| e.to_string())
}

#[cfg(not(feature = "whmcs-mysql"))]
async fn load_mysql(_dsn: &str) -> Result<pelty_import::WhmcsData, String> {
    Err("this binary was built without MySQL support; export the WHMCS database (mysqldump) and use --file, or rebuild with `--features whmcs-mysql`".into())
}

/// Load, decrypt, import; returns the rendered report.
pub async fn run(pool: &PgPool, cfg: &Config, o: &Opts) -> Result<String, String> {
    let mut data = load(o).await?;
    let mut warnings = vec![];
    match &o.cc_hash {
        Some(cc) => warnings.extend(data.decrypt_server_passwords(cc)),
        None if !data.servers.is_empty() => warnings.push(
            "no cc_encryption_hash given (--cc-hash / WHMCS_CC_ENCRYPTION_HASH): server passwords stay encrypted; servers without an API key are imported disabled".into(),
        ),
        None => {}
    }
    let sb = match &cfg.encryption_key {
        Some(k) => Some(pelty_auth::crypto::SecretBox::from_base64(&k.0)?),
        None => {
            if !data.servers.is_empty() {
                warnings.push("PELTY_ENCRYPTION_KEY is not set: servers are imported disabled without credentials".into());
            }
            None
        }
    };
    let sealer = sb.as_ref().map(|b| move |p: &[u8]| b.seal(p));
    let seal: Option<pelty_import::Sealer> = sealer.as_ref().map(|f| f as _);
    let mut report = pelty_import::run_with(pool, &data, o.dry_run, "whmcs", seal)
        .await
        .map_err(|e| e.to_string())?;
    warnings.append(&mut report.warnings);
    report.warnings = warnings;
    Ok(report.render())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn args() {
        let o = parse_args(&["--file", "a.sql", "--cc-hash", "x", "--dry-run"]).unwrap();
        assert_eq!(
            o,
            Opts {
                file: Some("a.sql".into()),
                dsn: None,
                cc_hash: Some("x".into()),
                dry_run: true
            }
        );
        assert!(parse_args(&["--file"]).is_err());
        assert!(parse_args(&["--file", "a", "--dsn", "b"]).is_err());
        assert!(parse_args(&["--bogus"]).is_err());
    }
}
