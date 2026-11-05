//! Platform reference pages built from structured sources: system settings
//! keys, general settings, email templates and scheduled tasks.
use crate::doc::Page;
use crate::extract::{self, read};
use crate::pages::fields_table;

fn c(s: &str) -> String {
    format!("`{s}`")
}

pub const GENERAL_STRUCTS: &[&str] = &[
    "General",
    "SystemUrls",
    "Maintenance",
    "Localisation",
    "Ordering",
    "Support",
    "CustomCode",
];
const GS: &str = "crates/pelty-database/src/config/general_settings.rs";

pub fn settings_keys() -> Page {
    let mut p = Page::new("settings-keys", "Settings keys", "Platform reference");
    let (keys, unresolved) = extract::settings_keys();
    p.p(format!(
        "Runtime settings live in the `system_settings` table as JSON values, one row per key, and are edited in the admin area (not through environment variables, see [Configuration](configuration.md)). {} keys are read or written by the code. Found by scanning `settings::get` / `settings::set` calls (consts resolved to their value), raw SQL on `system_settings`, and `*_KEY` consts of files that query the table directly.",
        keys.len()
    ));
    p.table(
        &["Key", "Const", "Description", "Used in"],
        keys.iter()
            .map(|k| {
                vec![
                    c(&k.key),
                    k.consts.iter().map(|x| c(x)).collect::<Vec<_>>().join(" "),
                    k.doc.clone(),
                    k.files.iter().map(|x| c(x)).collect::<Vec<_>>().join(" "),
                ]
            })
            .collect(),
    );
    if !unresolved.is_empty() {
        p.h(2, "Dynamic keys");
        p.p("These calls build the key at run time, so the scanner cannot list a fixed value:");
        p.list(unresolved.iter().map(|u| c(u)).collect());
    }
    p.h(2, "General settings");
    p.p("Admin → System & settings → General settings stores one JSON document under `general_settings` (custom head/body code under `custom_code`). Its sections:");
    for s in GENERAL_STRUCTS {
        p.h(3, *s);
        fields_table(&mut p, GS, s);
    }
    p
}

pub fn email_templates() -> Page {
    let en = extract::en();
    let mut p = Page::new("email-templates", "Email templates", "Platform reference");
    let t = pelty_notifications::template::TEMPLATES;
    p.p(format!(
        "{} templates, from `pelty_notifications::template::TEMPLATES`. Defaults (subject and body) come from the locale files (`email.<name>.subject`, `email.<name>.body`); admins override them per locale in Admin → System & settings → Email templates, and deleting an override restores the default. Variables are written `{{{{name}}}}` in subject and body.",
        t.len()
    ));
    p.table(
        &["Template", "Name", "Default subject (en)", "Variables"],
        t.iter()
            .map(|d| {
                vec![
                    c(d.name),
                    en.get(&format!("email.name.{}", d.name))
                        .cloned()
                        .unwrap_or_default(),
                    en.get(&format!("email.{}.subject", d.name))
                        .cloned()
                        .unwrap_or_default()
                        .replace('|', "\\|"),
                    d.vars.iter().map(|v| c(v)).collect::<Vec<_>>().join(" "),
                ]
            })
            .collect(),
    );
    p
}

fn every(secs: i64) -> String {
    match secs {
        s if s % 86_400 == 0 => format!("every {} day(s)", s / 86_400),
        s if s % 3_600 == 0 => format!("every {} hour(s)", s / 3_600),
        s => format!("every {} min", s / 60),
    }
}

/// Background job kinds declared as `pub const …: &str = "kind"` (job consts).
pub fn job_kinds() -> Vec<(String, String)> {
    let re = regex::Regex::new(
        r#"pub const (?:JOB_KIND|[A-Z_]*_JOB|RECURRING|ORPHAN_UPLOADS): &str = "([a-z_.]+)";"#,
    )
    .unwrap();
    let mut out = vec![];
    for f in [
        "platform/worker.rs",
        "money/overdue.rs",
        "money/autopay.rs",
        "hosting/domains.rs",
        "platform/backup.rs",
        "money/exchange_rates.rs",
        "comms/notify.rs",
        "platform/updates.rs",
        "addon_host/plugin_hooks.rs",
        "addon_host/addon_power.rs",
        "comms/mail.rs",
        "comms/support_ext.rs",
        "routes/finance/usage.rs",
        "routes/messaging/campaigns.rs",
        "routes/helpdesk/support.rs",
    ] {
        let rel = format!("crates/pelty-api/src/{f}");
        for cap in re.captures_iter(&read(&rel)) {
            out.push((cap[1].to_string(), rel.clone()));
        }
    }
    out
}

pub fn scheduled_tasks() -> Page {
    let en = extract::en();
    let mut p = Page::new("scheduled-tasks", "Scheduled tasks", "Platform reference");
    let tasks = pelty_api::routes::automation::TASKS;
    p.p(format!(
        "The scheduler inside `pelty-server` queues {} built-in tasks (`pelty_api::routes::automation::TASKS`). No system cron is needed. Admin → System & settings → Jobs shows the last run and next run of each, can disable a task, pin a daily task to a start hour (UTC) and run one now (`jobs.manage`).",
        tasks.len()
    ));
    p.table(
        &["Task", "Shown as", "Interval"],
        tasks
            .iter()
            .map(|(k, s)| {
                vec![
                    c(k),
                    en.get(&format!("automation.task.{k}"))
                        .cloned()
                        .unwrap_or_default(),
                    every(*s),
                ]
            })
            .collect(),
    );
    p.h(2, "Other job kinds");
    p.p("Queued on demand (by an action or another task) rather than on a timer:");
    let sched: Vec<&str> = tasks.iter().map(|t| t.0).collect();
    p.table(
        &["Job kind", "Shown as", "Declared in"],
        job_kinds()
            .into_iter()
            .filter(|(k, _)| !sched.contains(&k.as_str()))
            .map(|(k, f)| {
                vec![
                    c(&k),
                    en.get(&format!("automation.task.{k}"))
                        .cloned()
                        .unwrap_or_default(),
                    c(&f),
                ]
            })
            .collect(),
    );
    p
}
