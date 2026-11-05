//! Terminal prompts (dialoguer: hidden input works on Windows and Unix) and
//! the live database / SMTP checks used by the wizard.
use super::{DbCheck, Prompt, Services};
use dialoguer::{theme::ColorfulTheme, Confirm, FuzzySelect, Input, Password, Select};
use std::io;

#[derive(Default)]
pub struct Term {
    theme: ColorfulTheme,
}

fn io_err(e: dialoguer::Error) -> io::Error {
    match e {
        dialoguer::Error::IO(e) => e,
    }
}

impl Prompt for Term {
    fn say(&mut self, text: &str) {
        println!("{text}");
    }
    fn input(&mut self, q: &str, default: &str) -> io::Result<String> {
        let mut i = Input::<String>::with_theme(&self.theme)
            .with_prompt(q)
            .allow_empty(true);
        if !default.is_empty() {
            i = i.default(default.to_string());
        }
        i.interact_text().map_err(io_err)
    }
    fn secret(&mut self, q: &str, confirm: bool) -> io::Result<String> {
        let mut p = Password::with_theme(&self.theme)
            .with_prompt(q)
            .allow_empty_password(!confirm)
            .report(false);
        if confirm {
            p = p.with_confirmation("Repeat it", "The two entries differ, try again");
        }
        let v = p.interact().map_err(io_err)?;
        // Own report line: dialoguer would print ******** even for an empty entry.
        let shown = if v.is_empty() { "(none)" } else { "(hidden)" };
        println!(
            "{} {} {} {}",
            console::style("✔").green(),
            console::style(q).bold(),
            console::style("·").dim(),
            console::style(shown).dim()
        );
        Ok(v)
    }
    fn confirm(&mut self, q: &str, default: bool) -> io::Result<bool> {
        Confirm::with_theme(&self.theme)
            .with_prompt(q)
            .default(default)
            .interact()
            .map_err(io_err)
    }
    fn select(&mut self, q: &str, items: &[&str], default: usize) -> io::Result<usize> {
        if items.len() > 12 {
            FuzzySelect::with_theme(&self.theme)
                .with_prompt(q)
                .items(items)
                .default(default)
                .max_length(12)
                .interact()
                .map_err(io_err)
        } else {
            Select::with_theme(&self.theme)
                .with_prompt(q)
                .items(items)
                .default(default)
                .interact()
                .map_err(io_err)
        }
    }
    fn multi_select(
        &mut self,
        q: &str,
        items: &[&str],
        defaults: &[bool],
    ) -> io::Result<Vec<usize>> {
        dialoguer::MultiSelect::with_theme(&self.theme)
            .with_prompt(q)
            .items(items)
            .defaults(defaults)
            .interact()
            .map_err(io_err)
    }
}

/// Real checks on a small single-threaded runtime (the server's runtime does
/// not exist yet while the wizard runs).
pub struct Live {
    rt: tokio::runtime::Runtime,
}

impl Live {
    pub fn new() -> io::Result<Self> {
        Ok(Live {
            rt: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?,
        })
    }
}

const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

async fn connect(url: &str) -> Result<sqlx::PgConnection, sqlx::Error> {
    use sqlx::Connection;
    match tokio::time::timeout(TIMEOUT, sqlx::PgConnection::connect(url)).await {
        Ok(r) => r,
        Err(_) => Err(sqlx::Error::PoolTimedOut),
    }
}

impl Services for Live {
    fn check_db(&mut self, url: &str) -> DbCheck {
        self.rt.block_on(async {
            match connect(url).await {
                Ok(c) => {
                    use sqlx::Connection;
                    let _ = c.close().await;
                    DbCheck::Ok
                }
                Err(sqlx::Error::Database(e)) if e.code().as_deref() == Some("3D000") => {
                    DbCheck::MissingDatabase
                }
                Err(sqlx::Error::PoolTimedOut) => DbCheck::Failed("timed out connecting".into()),
                Err(e) => DbCheck::Failed(e.to_string()),
            }
        })
    }

    fn create_db(&mut self, url: &str) -> Result<(), String> {
        let name = super::pg_parts(url).db;
        super::valid_db_name(&name)?;
        let mut admin = url::Url::parse(url).map_err(|e| e.to_string())?;
        admin.set_path("postgres");
        self.rt.block_on(async {
            use sqlx::Executor;
            let mut c = connect(admin.as_str()).await.map_err(|e| e.to_string())?;
            let sql = format!("CREATE DATABASE \"{}\"", name.replace('"', "\"\""));
            c.execute(sqlx::AssertSqlSafe(sql))
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    fn currencies(&mut self, url: &str) -> Vec<String> {
        self.rt.block_on(async {
            let Ok(mut c) = connect(url).await else {
                return vec![];
            };
            // Before the first start the table does not exist yet: fall back to typing.
            let rows: Vec<(String,)> =
                sqlx::query_as("SELECT code FROM currencies ORDER BY enabled DESC, code")
                    .fetch_all(&mut c)
                    .await
                    .unwrap_or_default();
            rows.into_iter().map(|r| r.0).collect()
        })
    }

    fn redis_ping(&mut self, url: &str) -> Result<u128, String> {
        let sh = pelty_api::shared::Shared::new(Some(url.to_string()));
        self.rt.block_on(sh.ping()).map(|d| d.as_millis())
    }
    fn send_test_mail(&mut self, smtp_url: &str, from: &str, to: &str) -> Result<(), String> {
        use pelty_notifications::Mailer;
        let m = pelty_notifications::SmtpMailer::from_url(smtp_url, from)?;
        let e = pelty_notifications::Email {
            to: to.to_string(),
            subject: "PeltyCMS test email".into(),
            text: "This is a test email from the PeltyCMS setup wizard. SMTP works.".into(),
            html: "<p>This is a test email from the PeltyCMS setup wizard. SMTP works.</p>".into(),
            list_unsubscribe: None,
        };
        self.rt.block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(30), m.send(&e))
                .await
                .map_err(|_| "timed out".to_string())?
        })
    }
}
