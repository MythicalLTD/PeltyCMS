//! Tests for the interactive first-run setup with scripted answers.
use super::*;
use std::collections::VecDeque;

/// Scripted answers: `""` = Enter (default), `"y"` / `"n"` for confirms.
struct Script {
    answers: VecDeque<String>,
    out: Vec<String>,
}

impl Script {
    fn new(a: &[&str]) -> Self {
        Script {
            answers: a.iter().map(|s| s.to_string()).collect(),
            out: Vec::new(),
        }
    }
    fn next(&mut self, q: &str) -> io::Result<String> {
        self.out.push(format!("? {q}"));
        self.answers.pop_front().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("no scripted answer for: {q}"),
            )
        })
    }
    fn said(&self, needle: &str) -> bool {
        self.out.iter().any(|l| l.contains(needle))
    }
}

impl Prompt for Script {
    fn say(&mut self, text: &str) {
        self.out.push(text.to_string());
    }
    fn input(&mut self, q: &str, _default: &str) -> io::Result<String> {
        self.next(q)
    }
    fn secret(&mut self, q: &str, _confirm: bool) -> io::Result<String> {
        self.next(q)
    }
    fn confirm(&mut self, q: &str, default: bool) -> io::Result<bool> {
        Ok(match self.next(q)?.as_str() {
            "" => default,
            a => a.starts_with('y'),
        })
    }
    /// `""` = default, else the first item starting with the answer.
    fn select(&mut self, q: &str, items: &[&str], default: usize) -> io::Result<usize> {
        let a = self.next(q)?.to_lowercase();
        if a.is_empty() {
            return Ok(default);
        }
        items
            .iter()
            .position(|i| i.to_lowercase().starts_with(&a))
            .ok_or_else(|| io::Error::other(format!("no item {a:?} in {items:?}")))
    }
    /// "" = the defaults, "-" = none, else comma-separated item prefixes.
    fn multi_select(
        &mut self,
        q: &str,
        items: &[&str],
        defaults: &[bool],
    ) -> io::Result<Vec<usize>> {
        let a = self.next(q)?.to_lowercase();
        match a.as_str() {
            "" => Ok(defaults
                .iter()
                .enumerate()
                .filter(|(_, d)| **d)
                .map(|(i, _)| i)
                .collect()),
            "-" => Ok(vec![]),
            _ => a
                .split(',')
                .map(|w| {
                    items
                        .iter()
                        .position(|i| i.to_lowercase().starts_with(w.trim()))
                        .ok_or_else(|| io::Error::other(format!("no item {w:?}")))
                })
                .collect(),
        }
    }
}

#[derive(Default)]
struct Fake {
    db: VecDeque<DbCheck>,
    created: Vec<String>,
    mails: Vec<(String, String, String)>,
    currencies: Vec<String>,
}

impl Services for Fake {
    fn check_db(&mut self, _url: &str) -> DbCheck {
        self.db.pop_front().unwrap_or(DbCheck::Ok)
    }
    fn create_db(&mut self, url: &str) -> Result<(), String> {
        self.created.push(url.to_string());
        Ok(())
    }
    fn currencies(&mut self, _url: &str) -> Vec<String> {
        self.currencies.clone()
    }
    fn redis_ping(&mut self, url: &str) -> Result<u128, String> {
        if url.contains("good") {
            Ok(1)
        } else {
            Err("connection refused".into())
        }
    }
    fn send_test_mail(&mut self, u: &str, f: &str, t: &str) -> Result<(), String> {
        self.mails.push((u.into(), f.into(), t.into()));
        Ok(())
    }
    fn ensure_local_pg(&mut self, port: u16) -> Result<String, String> {
        Ok(format!(
            "postgres://pelty:local-test-secret@127.0.0.1:{port}/peltycms"
        ))
    }
}

fn none(_: &str) -> Option<String> {
    None
}

fn run(
    p: &mut Script,
    f: &mut Fake,
    cur: &dyn Fn(&str) -> Option<String>,
    eula: bool,
) -> Result<Answers, Error> {
    let mut ctx = Ctx {
        cur,
        eula_accepted: eula,
        languages: vec!["en".into(), "ro".into()],
        currencies: vec![],
        captcha: vec![],
        gateways: vec![],
    };
    wizard(p, f, &mut ctx)
}

#[test]
fn defaults_everywhere_with_generated_admin() {
    let mut p = Script::new(&[
        "y", // eula
        "",  // existing PostgreSQL (default)
        "",  // as url? (no)
        "",
        "",
        "",
        "",
        "", // host port user password db
        "",
        "",
        "",
        "", // bind, public, proxy (none), https
        "", // company now? yes
        "Acme Hosting",
        "billing@acme.test",
        "",
        "",
        "", // name email currency lang tz
        "",
        "",
        "",
        "", // admin? name email (default company email) generate? yes
        "", // smtp? no
        "", // advanced? no
        "", // save
    ]);
    let mut f = Fake::default();
    let a = run(&mut p, &mut f, &none, false).unwrap();
    assert_eq!(
        a.env["DATABASE_URL"],
        "postgres://postgres@127.0.0.1:5432/peltycms"
    );
    assert_eq!(a.env["PELTY_LOCAL_PG"], "false");
    assert_eq!(a.env["PELTY_BIND"], "127.0.0.1:8080");
    assert_eq!(a.env["PELTY_PUBLIC_URL"], "http://localhost:8080");
    assert_eq!(a.env["PELTY_COOKIE_SECURE"], "false");
    assert!(!a.env.contains_key("PELTY_TRUSTED_PROXIES"));
    assert_eq!(a.env[MARKER], "done");
    assert!(!a.env.contains_key("PELTY_SMTP_URL") && !a.env.contains_key("RUST_LOG"));
    let site = a.plan.site.unwrap();
    assert_eq!(
        (
            site.currency.as_str(),
            site.language.as_str(),
            site.timezone.as_str()
        ),
        ("EUR", "en", "UTC")
    );
    let admin = a.plan.admin.unwrap();
    assert_eq!(
        (admin.email.as_str(), admin.name.as_str()),
        ("billing@acme.test", "Administrator")
    );
    assert!(admin.generated && admin.password.len() >= MIN_PASSWORD);
    assert!(pelty_auth::password::check_policy(&admin.password, &admin.email).is_ok());
    assert!(
        p.said("[1/9]")
            && p.said("[9/9]")
            && p.said("No captcha providers")
            && p.said("No gateway addons available")
    );
}

#[test]
fn declining_the_eula_stops() {
    let mut p = Script::new(&[""]);
    assert!(matches!(
        run(&mut p, &mut Fake::default(), &none, false),
        Err(Error::Declined)
    ));
}

#[test]
fn eula_already_accepted_is_not_asked_and_cancel_writes_nothing() {
    let mut p = Script::new(&[
        "", "", "", "", "", "", "", "", "", "", "", "n", "n", "", "", "cancel",
    ]);
    let r = run(&mut p, &mut Fake::default(), &none, true);
    assert!(matches!(r, Err(Error::Cancelled)), "{r:?}");
    assert!(p.said("EULA already accepted"));
    assert!(p.said("(skipped, unchanged)") && p.said("(skipped, none created)"));
}

#[test]
fn database_parts_retry_create_and_validation() {
    let mut p = Script::new(&[
        "y",
        "", // existing PostgreSQL
        "n",
        "db.local",
        "notaport",
        "6543",
        "pelty",
        "s3cr:t@x",
        "my db",
        "pelty_prod", // invalid port + db name re-asked
        "y",          // create missing db
        "",           // (check after creation fails) try again? yes
        "y",
        "postgres://u:p@h:1/x", // as URL now
        "0.0.0.0:9000",
        "https://billing.example.com/",
        "",
        "", // proxy none, https default true from URL
        "n",
        "n",
        "n",
        "n",
        "",
    ]);
    let mut f = Fake {
        db: VecDeque::from([
            DbCheck::MissingDatabase,
            DbCheck::Failed("refused".into()),
            DbCheck::Ok,
        ]),
        ..Default::default()
    };
    let a = run(&mut p, &mut f, &none, false).unwrap();
    assert_eq!(
        f.created,
        vec!["postgres://pelty:s3cr%3At%40x@db.local:6543/pelty_prod".to_string()]
    );
    assert!(p.said("a port number") && p.said("letters, digits"));
    assert!(p.said("! refused"));
    assert_eq!(a.env["DATABASE_URL"], "postgres://u:p@h:1/x");
    assert_eq!(a.env["PELTY_PUBLIC_URL"], "https://billing.example.com");
    assert_eq!(a.env["PELTY_COOKIE_SECURE"], "true");
    assert_eq!(a.env["PELTY_BIND"], "0.0.0.0:9000");
    assert!(a.plan.site.is_none() && a.plan.admin.is_none());
    // the password never reaches the screen
    assert!(!p.out.iter().any(|l| l.contains("s3cr")));
}

#[test]
fn typed_admin_password_policy_and_smtp_with_test_mail() {
    let mut p = Script::new(&[
        "y",
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "n",
        "y",
        "Jane Doe",
        "admin@acme.test",
        "n",
        "short",
        "aaaaaaaaaaaaaaaa",
        "Correct-Horse-Battery-9",
        "y",
        "smtp.acme.test",
        "",
        "",
        "mailer",
        "pw",
        "",
        "y",
        "",
        "y",
        "debug",
        "",
        "60",
        "",
        "30",
        "",
        "/srv/backups",
        "y",
        "y",
        "10",
        "",
        "",
        "",
        "",
        "100",
        "64",
        "n",
        "25",
        "3",
        "y",
        "redis://bad:6379",
        "y",
        "redis://good:6379/0",
        "n", // optional AI drafts
        "",
    ]);
    let mut f = Fake::default();
    let a = run(
        &mut p,
        &mut f,
        &|k| (k == "PELTY_PUBLIC_URL").then(|| "https://acme.test".to_string()),
        false,
    )
    .unwrap();
    let admin = a.plan.admin.unwrap();
    assert_eq!(
        (admin.password.as_str(), admin.name.as_str()),
        ("Correct-Horse-Battery-9", "Jane Doe")
    );
    assert!(!admin.generated);
    assert!(p.said("rejected by the policy") && p.said("Password strength: strong"));
    assert_eq!(
        a.env["PELTY_SMTP_URL"],
        "smtp://mailer:pw@smtp.acme.test:587?tls=required"
    );
    assert_eq!(a.env["PELTY_MAIL_FROM"], "PeltyCMS <no-reply@acme.test>");
    assert_eq!(f.mails[0].2, "admin@acme.test");
    // advanced: uploads, background jobs, redis
    assert_eq!(
        (
            a.env["PELTY_UPLOAD_ATTACHMENT_MB"].as_str(),
            a.env["PELTY_UPLOAD_AVATAR_MB"].as_str(),
            a.env["PELTY_UPLOAD_ADDON_MB"].as_str()
        ),
        ("10", "2", "64")
    );
    assert!(p.said("between 1 and 64") || p.said("64"));
    assert_eq!(
        (
            a.env["PELTY_SCHEDULER"].as_str(),
            a.env["PELTY_WORKER_CONCURRENCY"].as_str(),
            a.plan.billing_hour
        ),
        ("false", "25", Some(3))
    );
    assert!(p.said("pelty-server cron run"));
    assert!(p.said("Redis test failed: connection refused") && p.said("Redis OK"));
    assert_eq!(a.env["PELTY_REDIS_URL"], "redis://good:6379/0");
    assert_eq!(a.env["RUST_LOG"], "debug,sqlx=warn");
    assert_eq!(a.env["PELTY_SESSION_IDLE_MINUTES"], "60");
    assert_eq!(a.env["PELTY_AUTH_RATE_PER_MINUTE"], "30");
    assert_eq!(a.env["PELTY_BACKUP_DIR"], "/srv/backups");
    assert_eq!(a.env["PELTY_DOCS_PUBLIC"], "true");
    assert_eq!(a.env["PELTY_LOG_JSON"], "false");
    let s = summary(&Answers {
        env: a.env.clone(),
        ..Default::default()
    });
    assert!(s.contains("smtp://mailer:****@") && !s.contains(":pw@"));
}

#[test]
fn proxy_presets_and_validation() {
    // nginx preset; https defaults to yes behind a proxy.
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "0.0.0.0:8080",
        "",
        "nginx",
        "",
        "n",
        "n",
        "",
        "",
        "",
    ]);
    let a = run(&mut p, &mut Fake::default(), &none, true).unwrap();
    assert_eq!(a.env["PELTY_TRUSTED_PROXIES"], "127.0.0.1,::1");
    assert_eq!(a.env["PELTY_COOKIE_SECURE"], "true");
    assert!(p.said("listen on 127.0.0.1"));
    // Cloudflare preset, then "other" with validation.
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "https://x.test",
        "cloudflare",
        "",
        "n",
        "n",
        "",
        "",
        "",
    ]);
    let a = run(&mut p, &mut Fake::default(), &none, true).unwrap();
    assert_eq!(a.env["PELTY_TRUSTED_PROXIES"], CLOUDFLARE);
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "other",
        "0.0.0.0/0",
        "10.0.0.0/24 nope",
        "10.0.0.0/24, 192.0.2.1",
        "n",
        "n",
        "n",
        "",
        "",
        "",
    ]);
    let a = run(&mut p, &mut Fake::default(), &none, true).unwrap();
    assert_eq!(a.env["PELTY_TRUSTED_PROXIES"], "10.0.0.0/24,192.0.2.1");
    assert_eq!(a.env["PELTY_COOKIE_SECURE"], "false");
    assert!(p.said("would trust everyone") && p.said("nope is not an IP"));
    // Previously set, now "no proxy": cleared.
    let had = |k: &str| (k == "PELTY_TRUSTED_PROXIES").then(|| "127.0.0.1,::1".to_string());
    let mut p = Script::new(&[
        "", "", "", "", "", "", "", "", "", "no", "", "n", "n", "", "", "",
    ]);
    let a = run(&mut p, &mut Fake::default(), &had, true).unwrap();
    assert_eq!(a.env["PELTY_TRUSTED_PROXIES"], "");
}

#[test]
fn selects_currency_language_timezone_and_reedit_from_summary() {
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "Acme",
        "a@acme.test",
        "RON",
        "ro",
        "Other",
        "bad zone",
        "Europe/Bucharest", // company with typed zone
        "n",
        "",
        "",
        "edit: company",
        "",
        "Acme SRL",
        "",
        "",
        "",
        "", // re-edit the company: new name, rest kept
        "edit: admin",
        "",
        "Ops",
        "ops@acme.test",
        "", // now add an admin
        "", // save
    ]);
    let mut f = Fake {
        currencies: vec!["EUR".into(), "RON".into(), "USD".into()],
        ..Default::default()
    };
    let mut ctx = Ctx {
        cur: &none,
        eula_accepted: true,
        languages: vec!["en".into(), "ro".into()],
        currencies: vec![],
        captcha: vec![],
        gateways: vec![],
    };
    let a = wizard(&mut p, &mut f, &mut ctx).unwrap();
    let site = a.plan.site.unwrap();
    assert_eq!(
        (
            site.company_name.as_str(),
            site.currency.as_str(),
            site.language.as_str(),
            site.timezone.as_str()
        ),
        ("Acme SRL", "RON", "ro", "Europe/Bucharest")
    );
    assert_eq!(a.env["PELTY_DEFAULT_LOCALE"], "ro");
    assert!(p.said("an IANA time zone"));
    assert_eq!(a.plan.admin.unwrap().name, "Ops");
}

#[test]
fn captcha_choice_with_keys_and_none() {
    let dir = std::env::temp_dir().join(format!("pelty-dist-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    for f in [
        "captcha-turnstile.zip",
        "gateway-stripe.zip",
        "captcha-hcaptcha.zip",
    ] {
        std::fs::write(dir.join(f), b"").unwrap();
    }
    let provs = captcha_providers(&dir);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        provs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        ["captcha-internal", "captcha-hcaptcha", "captcha-turnstile"]
    );
    assert_eq!(captcha_providers(Path::new("/nonexistent")).len(), 1);
    let mut ctx = Ctx {
        cur: &none,
        eula_accepted: true,
        languages: vec!["en".into()],
        currencies: vec![],
        captcha: provs,
        gateways: vec![],
    };
    // Turnstile with keys (empty secret re-asked), then re-edited to none from the summary.
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "n",
        "n",
        "",
        "cloudflare",
        "0x4AAA",
        "",
        "0xSECRET",
        "",
        "edit: captcha",
        "none",
        "",
    ]);
    let a = wizard(&mut p, &mut Fake::default(), &mut ctx).unwrap();
    assert!(a.plan.captcha.is_none());
    assert!(
        p.said("Cloudflare Turnstile")
            && p.said("0x4AAA")
            && !p.out.iter().any(|l| l.contains("0xSECRET"))
    );
    let mut p = Script::new(&[
        "", "", "", "", "", "", "", "", "", "", "", "n", "n", "", "built-in", "", "",
    ]);
    let a = wizard(&mut p, &mut Fake::default(), &mut ctx).unwrap();
    let c = a.plan.captcha.unwrap();
    assert_eq!(
        (c.provider.as_str(), c.secret_key.as_str()),
        ("captcha-internal", "")
    );
}

#[test]
fn headless_defaults_flags_and_errors() {
    let h = Headless::default();
    let e = headless(&mut Fake::default(), &none, false, &h).unwrap_err();
    assert!(e.contains("--accept-eula"));
    let mut f = Fake {
        db: VecDeque::from([DbCheck::MissingDatabase, DbCheck::Ok]),
        ..Default::default()
    };
    let mut args: Vec<String> = [
        "setup",
        "--db-url=postgres://u@db/pelty",
        "--bind",
        "0.0.0.0:9000",
        "--admin-email",
        "root@acme.test",
        "--admin-name",
        "Root",
        "--accept-eula",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let h = Headless::take_flags(&mut args).unwrap();
    assert_eq!(args, vec!["setup"]);
    assert!(!h.local_pg);
    let a = headless(&mut f, &none, false, &h).unwrap();
    assert_eq!(f.created, vec!["postgres://u@db/pelty".to_string()]);
    assert_eq!(a.env["PELTY_PUBLIC_URL"], "http://localhost:9000");
    assert_eq!(a.env["PELTY_LOCAL_PG"], "false");
    assert_eq!(a.env[MARKER], "done");
    let admin = a.plan.admin.unwrap();
    assert!(admin.generated && admin.name == "Root");
    // Environment overrides and policy errors.
    let cur = |k: &str| match k {
        "PELTY_ADMIN_EMAIL" => Some("a@acme.test".to_string()),
        "PELTY_ADMIN_PASSWORD" => Some("short".to_string()),
        _ => None,
    };
    assert!(
        headless(&mut Fake::default(), &cur, true, &Headless::default())
            .unwrap_err()
            .contains("--admin-password")
    );
    let mut f = Fake {
        db: VecDeque::from([DbCheck::Failed("refused".into())]),
        ..Default::default()
    };
    assert!(headless(&mut f, &none, true, &Headless::default())
        .unwrap_err()
        .contains("refused"));
    assert!(Headless::take_flags(&mut vec!["--db-url".to_string()]).is_err());
    let mut args = vec!["--local-pg".into(), "--accept-eula".into()];
    let h = Headless::take_flags(&mut args).unwrap();
    assert!(h.local_pg && args.is_empty());
    let a = headless(&mut Fake::default(), &none, false, &h).unwrap();
    assert_eq!(a.env["PELTY_LOCAL_PG"], "true");
    assert!(a.env["DATABASE_URL"].contains("@127.0.0.1:5432/peltycms"));
}

#[test]
fn strength_and_next_steps() {
    assert_eq!(strength("aaaaaaaaaaaa"), "weak");
    assert_eq!(strength("abcdefgh1234"), "fair");
    assert_eq!(strength("Abcdefgh1234-x"), "good");
    assert_eq!(strength("Correct-Horse-Battery-Staple-9"), "strong");
    let s = next_steps(
        "https://x.test",
        Some("a@x.test"),
        Some("tok_abc"),
        Path::new("/srv/pelty"),
    );
    assert!(
        s.contains("https://x.test")
            && s.contains("a@x.test")
            && s.contains("/srv/pelty")
            && s.contains("/api/v1/auth/setup-login?token=tok_abc")
            && s.contains("one-time")
    );
    let plain = next_steps(
        "https://x.test",
        Some("a@x.test"),
        None,
        Path::new("/srv/pelty"),
    );
    assert!(plain.contains("a@x.test") && !plain.contains("setup-login"));
    let widths: Vec<usize> = console::strip_ansi_codes(&s)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.chars().count())
        .collect();
    assert!(widths.windows(2).all(|w| w[0] == w[1]), "{s}");
}

#[test]
fn merge_keeps_comments_unknown_keys_and_order() {
    let text = "# head\nDATABASE_URL=postgres://old\n# PELTY_SMTP_URL=smtp://example\nCUSTOM=1\nexport PELTY_BIND=1.2.3.4:5\n";
    let mut u = BTreeMap::new();
    u.insert("DATABASE_URL".to_string(), "postgres://new".to_string());
    u.insert("PELTY_SMTP_URL".to_string(), "smtp://real".to_string());
    u.insert("PELTY_BIND".to_string(), "0.0.0.0:8080".to_string());
    u.insert("PELTY_MAIL_FROM".to_string(), "Acme <a@b.c>".to_string());
    let out = merge_env(text, &u);
    assert_eq!(
        out,
        "# head\nDATABASE_URL=postgres://new\nPELTY_SMTP_URL=smtp://real\nCUSTOM=1\nPELTY_BIND=0.0.0.0:8080\n\n# --- Set by pelty-server setup ----------------------------------------------\nPELTY_MAIL_FROM=\"Acme <a@b.c>\"\n"
    );
    // round trip through the .env parser
    let parsed: BTreeMap<_, _> = bootstrap::parse_env(&out).into_iter().collect();
    assert_eq!(parsed["PELTY_MAIL_FROM"], "Acme <a@b.c>");
    assert_eq!(parsed["CUSTOM"], "1");
    // idempotent
    assert_eq!(merge_env(&out, &u), out);
}

#[test]
fn merge_prefers_active_line_over_commented_example() {
    let text = "# PELTY_BIND=example\nPELTY_BIND=1.1.1.1:1\n";
    let u = BTreeMap::from([("PELTY_BIND".to_string(), "2.2.2.2:2".to_string())]);
    assert_eq!(
        merge_env(text, &u),
        "# PELTY_BIND=example\nPELTY_BIND=2.2.2.2:2\n"
    );
}

#[test]
fn write_env_template_marker_and_mode() {
    let d = std::env::temp_dir().join(format!("pelty-setup-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&d).unwrap();
    let mut r = bootstrap::Report::default();
    bootstrap::ensure_env_file(&d, &mut r);
    assert!(pending(&d));
    let key_before: BTreeMap<_, _> = bootstrap::read_env_file(&d).into_iter().collect();
    write_env(
        &d,
        &BTreeMap::from([
            (MARKER.to_string(), "done".to_string()),
            ("PELTY_BIND".to_string(), "0.0.0.0:1".to_string()),
        ]),
    )
    .unwrap();
    assert!(!pending(&d));
    let after: BTreeMap<_, _> = bootstrap::read_env_file(&d).into_iter().collect();
    assert_eq!(
        after["PELTY_ENCRYPTION_KEY"],
        key_before["PELTY_ENCRYPTION_KEY"]
    );
    assert_eq!(after["PELTY_BIND"], "0.0.0.0:1");
    assert!(std::fs::read_to_string(d.join(".env"))
        .unwrap()
        .contains("# --- Database"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(d.join(".env"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    accept_eula(&d).unwrap();
    assert!(crate::eula::accepted_in(
        &std::fs::read_to_string(d.join("eula.txt")).unwrap()
    ));
    std::fs::remove_dir_all(&d).ok();
}

#[test]
fn validators() {
    assert!(
        valid_email("a@b.co").is_ok()
            && valid_email("a@b").is_err()
            && valid_email("@b.co").is_err()
    );
    assert!(valid_bind("0.0.0.0:80").is_ok() && valid_bind("localhost").is_err());
    assert_eq!(
        valid_public_url("https://x.test/").unwrap(),
        "https://x.test"
    );
    assert!(valid_public_url("x.test").is_err());
    assert_eq!(valid_currency("ron").unwrap(), "RON");
    assert!(valid_currency("EURO").is_err());
    assert!(
        valid_proxies("10.0.0.1, ::1").is_ok()
            && valid_proxies("").is_err()
            && valid_proxies("::/0").is_err()
    );
    assert!(valid_timezone("Europe/Bucharest").is_ok() && valid_timezone("bad zone").is_err());
    assert!(valid_pg_url("mysql://x").is_err());
    assert_eq!(
        mask_url("postgres://u:secret@h/db"),
        "postgres://u:****@h/db"
    );
}

#[test]
fn public_url_default_follows_the_port_until_customised() {
    let script = || {
        Script::new(&[
            "", // existing PostgreSQL
            "",
            "",
            "",
            "",
            "",
            "",
            "127.0.0.1:9100",
            "",
            "",
            "",
            "n",
            "n",
            "",
            "",
            "",
        ])
    };
    let tmpl = |k: &str| match k {
        "PELTY_BIND" => Some("127.0.0.1:8080".to_string()),
        "PELTY_PUBLIC_URL" => Some("http://localhost:8080".to_string()),
        _ => None,
    };
    let a = run(&mut script(), &mut Fake::default(), &tmpl, true).unwrap();
    assert_eq!(a.env["PELTY_PUBLIC_URL"], "http://localhost:9100");
    let custom =
        |k: &str| (k == "PELTY_PUBLIC_URL").then(|| "https://billing.acme.test".to_string());
    let a = run(&mut script(), &mut Fake::default(), &custom, true).unwrap();
    assert_eq!(a.env["PELTY_PUBLIC_URL"], "https://billing.acme.test");
}

#[test]
fn gateways_from_bundled_packages_with_hidden_keys() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/addons/dist");
    let gws = gateway_choices(&dist);
    let stripe = gws
        .iter()
        .find(|g| g.addon == "gateway-stripe")
        .expect("bundled stripe package");
    assert_eq!(
        (stripe.provides.as_str(), stripe.secrets.clone()),
        (
            "stripe",
            vec!["secret_key".to_string(), "webhook_secret".to_string()]
        )
    );
    assert!(
        gws.iter().all(|g| g.addon.starts_with("gateway-"))
            && !gws.iter().any(|g| g.addon.starts_with("captcha"))
    );
    assert!(gateway_choices(Path::new("/nonexistent")).is_empty());
    assert_eq!(
        gateway_env("stripe", "secret_key"),
        "PELTY_GW_STRIPE_SECRET_KEY"
    );
    let mut ctx = Ctx {
        cur: &none,
        eula_accepted: true,
        languages: vec!["en".into()],
        currencies: vec![],
        captcha: vec![],
        gateways: gws,
    };
    // Stripe (live, one key now, webhook later) + Manual, then re-edited to none.
    let manual = ctx
        .gateways
        .iter()
        .find(|g| g.addon == "gateway-manual")
        .unwrap()
        .label
        .to_lowercase();
    let mut p = Script::new(&[
        "", // existing PostgreSQL
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "",
        "n",
        "n",
        "n",
        "y",
        &format!("stripe,{manual}"),
        "n",
        "sk_live_SECRET",
        "",
        "",
        "",
        "",
    ]);
    let r = wizard(&mut p, &mut Fake::default(), &mut ctx);
    let a = r.unwrap_or_else(|e| panic!("{e:?} {:#?}", p.out));
    assert_eq!(a.plan.gateways.len(), 2, "{:?}", p.out);
    let s = a
        .plan
        .gateways
        .iter()
        .find(|g| g.addon == "gateway-stripe")
        .unwrap();
    assert!(!s.test_mode);
    assert_eq!(
        s.secret_refs,
        vec![
            ("secret_key".into(), "PELTY_GW_STRIPE_SECRET_KEY".into()),
            (
                "webhook_secret".into(),
                "PELTY_GW_STRIPE_WEBHOOK_SECRET".into()
            )
        ]
    );
    assert_eq!(
        a.env.get("PELTY_GW_STRIPE_SECRET_KEY").map(String::as_str),
        Some("sk_live_SECRET")
    );
    assert!(!a.env.contains_key("PELTY_GW_STRIPE_WEBHOOK_SECRET"));
    assert!(
        p.said("live, 1/2 keys (hidden)") && !p.out.iter().any(|l| l.contains("sk_live_SECRET"))
    );
    let mut p = Script::new(&[
        "", "", "", "", "", "", "", "", "", "", "", "n", "n", "n", "n", "", "",
    ]);
    let a = wizard(&mut p, &mut Fake::default(), &mut ctx).unwrap();
    assert!(a.plan.gateways.is_empty() && p.said("(none now)"));
}

#[test]
fn local_postgres_mode_sets_env() {
    let mut p = Script::new(&[
        "y",     // eula
        "local", // Local PostgreSQL
        "",      // port default 5432
        "", "", "", "", // web defaults
        "n", "n", "n", "n", // company / admin / smtp / advanced
        "",  // save
    ]);
    let a = run(&mut p, &mut Fake::default(), &none, false).unwrap();
    assert_eq!(a.env["PELTY_LOCAL_PG"], "true");
    assert!(a.env["DATABASE_URL"].starts_with("postgres://pelty:"));
    assert!(a.env["DATABASE_URL"].contains("@127.0.0.1:5432/peltycms"));
    assert!(p.said("Ready at") && p.said("Connection OK"));
}
