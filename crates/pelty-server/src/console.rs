//! Interactive server console (Minecraft-server style): a line reader on
//! stdin next to the HTTP server. On a terminal it uses a line editor
//! (history, editing) whose external printer carries the log lines, so logs
//! never garble the prompt; with `--console` on a pipe it reads plain lines.
//! Commands reuse the same service functions as the admin API / CLI.
use pelty_api::AppState;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot, Notify};

pub const HELP: &str = "\
Console commands:
  help                                   this list
  stop                                   graceful shutdown (also Ctrl+C)
  status                                 version, uptime, database, addons, worker, Redis
  cluster                                instances sharing this Redis and the scheduler leader
  version                                server version
  addons list                            every addon with its status
  addons enable <id> | disable <id>      enable / disable an installed addon
  addons install <zip path | id> [--approve]
                                         install a package (or an available addon);
                                         --approve accepts its dangerous capabilities
  addons uninstall <id>                  uninstall an addon
  addons health                          call stats / circuit breaker of every addon
  admin create <email> [password] [--name <name>]
                                         create an administrator (password generated if omitted)
  admin reset-password <email> [password]
                                         set a new password, sign out everywhere
  users list [n]                         newest users (default 25)
  cron list                              scheduled tasks (built-in and `<addon>:<op>`), last / next run
  cron run <task>                        queue a task now
  cron enable <task> | disable <task>    turn automatic scheduling on / off
  cron history <task> [n]                last runs (default 10)
  cron schedule <task> <m h dom mon dow> set a 5-field cron schedule (company time zone)
  cron schedule <task> default           back to the built-in interval
  cron reset <task>                      clear a tripped circuit breaker and enable the task
  cron missed <task> skip|run_once       missed runs: skip (default) or run once on restart
  migrate                                apply pending database migrations
  reload                                 reload translation overrides from the database
  log level [filter]                     show / set the log filter (info, debug, info,sqlx=debug)
  update [--check] [--version x.y.z] [--yes] [--no-backup]
  import whmcs --file <whmcs.sql> [--cc-hash <hash>] [--dry-run]
                                         check for / install a release (installs only with --yes)";

#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    Help,
    Stop,
    Status,
    Cluster,
    CronList,
    CronRun(String),
    CronEnable(String, bool),
    CronHistory(String, i64),
    CronSchedule(String, Option<String>),
    CronReset(String),
    CronMissed(String, String),
    Version,
    AddonsList,
    AddonsEnable(String),
    AddonsDisable(String),
    AddonsInstall { target: String, approve: bool },
    AddonsUninstall(String),
    AddonsHealth,
    AdminCreate { email: String, password: Option<String>, name: Option<String> },
    AdminResetPassword { email: String, password: Option<String> },
    UsersList(i64),
    Migrate,
    Reload,
    LogLevel(Option<String>),
    Update(Vec<String>),
    ImportWhmcs(Vec<String>),
}

/// Split a line into words; `"..."` / `'...'` keep spaces (Windows paths).
pub fn split(line: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for c in line.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => cur.push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                any = true;
            }
            None if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            None => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err("unterminated quote".into());
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// Parse one console line; `Ok(None)` for a blank line. A leading `/` is
/// accepted (Minecraft habit).
pub fn parse(line: &str) -> Result<Option<Cmd>, String> {
    let words = split(line.trim().trim_start_matches('/'))?;
    let w: Vec<&str> = words.iter().map(String::as_str).collect();
    let lower: Vec<String> = w.iter().take(2).map(|s| s.to_ascii_lowercase()).collect();
    let head: Vec<&str> = lower.iter().map(String::as_str).collect();
    let usage = |u: &str| Err(format!("usage: {u}"));
    Ok(Some(match (head.as_slice(), w.len()) {
        ([], _) => return Ok(None),
        (["help" | "?", ..], _) => Cmd::Help,
        (["stop" | "exit" | "quit"], 1) => Cmd::Stop,
        (["status"], 1) => Cmd::Status,
        (["cluster"], 1) => Cmd::Cluster,
        (["cron", "list"], 2) | (["cron"], 1) => Cmd::CronList,
        (["cron", "run"], 3) => Cmd::CronRun(w[2].into()),
        (["cron", "run"], _) => return usage("cron run <task>"),
        (["cron", "enable"], 3) => Cmd::CronEnable(w[2].into(), true),
        (["cron", "disable"], 3) => Cmd::CronEnable(w[2].into(), false),
        (["cron", "enable" | "disable"], _) => return usage("cron enable|disable <task>"),
        (["cron", "history"], 3) => Cmd::CronHistory(w[2].into(), 10),
        (["cron", "history"], 4) => Cmd::CronHistory(w[2].into(), w[3].parse().map_err(|_| "usage: cron history <task> [n]".to_string())?),
        (["cron", "history"], _) => return usage("cron history <task> [n]"),
        (["cron", "schedule"], 4) if w[3].eq_ignore_ascii_case("default") => Cmd::CronSchedule(w[2].into(), None),
        // Unquoted expressions are accepted too: `cron schedule x 0 3 * * *`.
        (["cron", "schedule"], n) if n >= 4 => Cmd::CronSchedule(w[2].into(), Some(w[3..].join(" "))),
        (["cron", "schedule"], _) => return usage("cron schedule <task> \"<cron>\" | default"),
        (["cron", "reset"], 3) => Cmd::CronReset(w[2].into()),
        (["cron", "missed"], 4) if matches!(w[3], "skip" | "run_once") => Cmd::CronMissed(w[2].into(), w[3].into()),
        (["cron", "missed"], _) => return usage("cron missed <task> skip|run_once"),
        (["cron", "reset"], _) => return usage("cron reset <task>"),
        (["version"], 1) => Cmd::Version,
        (["import", "whmcs", ..], _) => Cmd::ImportWhmcs(w[2..].iter().map(|x| x.to_string()).collect()),
        (["import", ..], _) => return usage(crate::whmcs_import::USAGE),
        (["update", ..], _) => Cmd::Update(w[1..].iter().map(|x| x.to_string()).collect()),
        (["addons", "list"], 2) | (["addons"], 1) => Cmd::AddonsList,
        (["addons", "enable"], 3) => Cmd::AddonsEnable(w[2].into()),
        (["addons", "enable"], _) => return usage("addons enable <id>"),
        (["addons", "disable"], 3) => Cmd::AddonsDisable(w[2].into()),
        (["addons", "disable"], _) => return usage("addons disable <id>"),
        (["addons", "install"], 3 | 4) => {
            let mut approve = false;
            let mut target = None;
            for a in &w[2..] {
                match *a {
                    "--approve" => approve = true,
                    t if target.is_none() => target = Some(t.to_string()),
                    _ => return usage("addons install <zip path | id> [--approve]"),
                }
            }
            match target {
                Some(target) => Cmd::AddonsInstall { target, approve },
                None => return usage("addons install <zip path | id> [--approve]"),
            }
        }
        (["addons", "install"], _) => return usage("addons install <zip path | id> [--approve]"),
        (["addons", "uninstall"], 3) => Cmd::AddonsUninstall(w[2].into()),
        (["addons", "uninstall"], _) => return usage("addons uninstall <id>"),
        (["addons", "health"], 2) => Cmd::AddonsHealth,
        (["addons", ..], _) => return usage("addons list | enable <id> | disable <id> | install <zip|id> [--approve] | uninstall <id> | health"),
        (["admin", "create"], n) if n >= 3 => {
            // `admin create <email> [password] [--name First Last]`
            let flag = w.iter().position(|x| *x == "--name");
            let head = &w[..flag.unwrap_or(w.len())];
            let name = flag.map(|i| w[i + 1..].join(" "));
            if head.len() > 4 || head.len() < 3 || name.as_deref() == Some("") {
                return usage("admin create <email> [password] [--name <display name>]");
            }
            Cmd::AdminCreate { email: head[2].into(), password: head.get(3).map(|p| p.to_string()), name }
        }
        (["admin", "reset-password"], 3 | 4) => Cmd::AdminResetPassword { email: w[2].into(), password: w.get(3).map(|p| p.to_string()) },
        (["admin", ..], _) => return usage("admin create <email> [password] | admin reset-password <email> [password]"),
        (["users", "list"], 2) | (["users"], 1) => Cmd::UsersList(25),
        (["users", "list"], 3) => Cmd::UsersList(w[2].parse().ok().filter(|n| (1..=500).contains(n)).ok_or("users list [1-500]")?),
        (["users", ..], _) => return usage("users list [n]"),
        (["migrate"], 1) => Cmd::Migrate,
        (["reload"], 1) => Cmd::Reload,
        (["log", "level"], 2) => Cmd::LogLevel(None),
        (["log", "level"], 3) => Cmd::LogLevel(Some(w[2].into())),
        (["log", ..], _) => return usage("log level [filter]"),
        ([c, ..], _) if ["stop", "status", "cluster", "version", "migrate", "reload"].contains(c) => return Err(format!("`{c}` takes no arguments")),
        ([c, ..], _) => return Err(format!("unknown command `{c}`; type `help`")),
    }))
}

/// Shared by the running server and the console.
pub struct Ctx {
    pub state: AppState,
    pub started: Instant,
    pub worker: Arc<tokio::task::JoinHandle<()>>,
    pub stop: Arc<Notify>,
}

fn err(e: pelty_core::AppError) -> String {
    let m = e.to_string();
    if m.contains(e.code()) {
        m
    } else {
        format!("{m} ({})", e.code())
    }
}

/// A random password that passes every password-policy option.
pub fn generate_password() -> String {
    use rand::Rng;
    const SETS: [&[u8]; 4] = [b"ABCDEFGHJKLMNPQRSTUVWXYZ", b"abcdefghijkmnopqrstuvwxyz", b"23456789", b"-_.!@#%+="];
    let mut rng = rand::rngs::OsRng;
    loop {
        let all: Vec<u8> = SETS.concat();
        let p: String = (0..20).map(|_| all[rng.gen_range(0..all.len())] as char).collect();
        if SETS.iter().all(|s| p.bytes().any(|b| s.contains(&b))) {
            return p;
        }
    }
}

fn uptime(d: std::time::Duration) -> String {
    let s = d.as_secs();
    match (s / 86400, s / 3600 % 24, s / 60 % 60, s % 60) {
        (0, 0, m, s) => format!("{m}m {s:02}s"),
        (0, h, m, s) => format!("{h}h {m:02}m {s:02}s"),
        (d, h, m, _) => format!("{d}d {h}h {m:02}m"),
    }
}

fn fmt_ts(t: i64) -> String {
    time::OffsetDateTime::from_unix_timestamp(t).map(|d| format!("{} {:02}:{:02}", d.date(), d.hour(), d.minute())).unwrap_or_default()
}

fn task_err(t: &str, e: pelty_core::AppError) -> String {
    match e {
        pelty_core::AppError::NotFound => format!("no task {t:?} (see `cron list`)"),
        e => e.to_string(),
    }
}

/// Run one command; the text is printed by the console.
pub async fn exec(c: &Ctx, cmd: Cmd) -> Result<String, String> {
    use pelty_api::addons::{self, Actor};
    use pelty_api::routes::automation;
    let s = &c.state;
    match cmd {
        Cmd::Help => Ok(HELP.into()),
        Cmd::Version => Ok(format!("PeltyCMS {}", env!("CARGO_PKG_VERSION"))),
        Cmd::Update(args) => {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            let mut o = crate::update::parse_args(&a)?;
            // The console owns stdin, so it never prompts: no --yes = check only.
            let ask = !o.yes && !o.check;
            o.check |= ask;
            let mut out = vec![];
            crate::update::run(Some(&s.pool), &s.cfg.database_url, &o, &mut |m| out.push(m)).await?;
            if ask {
                out.push("run `update --yes` (optionally with --version) to install".into());
            }
            Ok(out.join("\n"))
        }
        Cmd::ImportWhmcs(args) => {
            let a: Vec<&str> = args.iter().map(String::as_str).collect();
            let o = crate::whmcs_import::parse_args(&a)?;
            crate::whmcs_import::run(&s.pool, &s.cfg, &o).await
        }
        Cmd::Stop => {
            c.stop.notify_one();
            Ok("Stopping the server...".into())
        }
        Cmd::Status => {
            let db = pelty_database::ping(&s.pool).await;
            let mut out = vec![
                format!("PeltyCMS {}, up {}", env!("CARGO_PKG_VERSION"), uptime(c.started.elapsed())),
                format!("listening on {} ({})", s.cfg.bind, s.cfg.public_url),
                format!("database: {} (pool {} / {} idle)", if db { "ok" } else { "UNREACHABLE" }, s.pool.size(), s.pool.num_idle()),
            ];
            if db {
                if let Ok(rows) = pelty_database::addon_registry::list(&s.pool).await {
                    let loaded = s.wasm_addons.read().map(|m| m.len()).unwrap_or(0);
                    out.push(format!("addons: {} installed, {} enabled, {} wasm loaded", rows.len(), rows.iter().filter(|r| r.enabled).count(), loaded));
                }
            }
            let mut w = format!("worker: {}", if c.worker.is_finished() { "STOPPED" } else { "running" });
            if db {
                if let Ok(t) = pelty_database::job_stats::totals(&s.pool).await {
                    w.push_str(&format!("; jobs {}", if t.is_empty() { "none".into() } else { t.iter().map(|(k, n)| format!("{k}={n}")).collect::<Vec<_>>().join(" ") }));
                }
            }
            if !s.cfg.scheduler {
                w.push_str("; scheduler off (PELTY_SCHEDULER=false: use `pelty-server cron run`)");
            }
            out.push(w);
            out.push(s.shared.status_line().await);
            out.push(format!("instance: {}", s.shared.instance));
            if s.shared.enabled() {
                let leader = pelty_api::cluster::instances(s).await.map(|(i, l)| (i.len(), l));
                out.push(match leader {
                    Some((n, l)) => format!("cluster: {n} live instance(s); scheduler leader {}; pubsub {}", l.as_deref().map(|l| if l == s.shared.instance { "this instance".to_string() } else { l.to_string() }).unwrap_or_else(|| "none yet".into()), pelty_api::cluster::bus_state()),
                    None => format!("cluster: unknown (Redis down); pubsub {}", pelty_api::cluster::bus_state()),
                });
            }
            out.push(format!("log filter: {}", crate::logging::current_filter()));
            Ok(out.join("\n"))
        }
        Cmd::Cluster => {
            if !s.shared.enabled() {
                return Ok(format!("single instance (no PELTY_REDIS_URL): {}", s.shared.instance));
            }
            let (list, leader) = pelty_api::cluster::instances(s).await.ok_or("Redis unreachable")?;
            let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let mut out = vec![format!("{:<40} {:<10} {:<22} {:<9} last seen", "instance", "version", "listen", "scheduler")];
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            for v in &list {
                let id = str_of(v, "id");
                let mark = if leader.as_deref() == Some(id.as_str()) { " (leader)" } else if id == s.shared.instance { " (this)" } else { "" };
                out.push(format!("{:<40} {:<10} {:<22} {:<9} {}s ago{mark}", id, str_of(v, "version"), str_of(v, "bind"), if v["scheduler"].as_bool() == Some(true) { "on" } else { "off" }, now - v["seen"].as_i64().unwrap_or(now)));
            }
            out.push(format!("{} live instance(s); pubsub {}", list.len(), pelty_api::cluster::bus_state()));
            Ok(out.join("\n"))
        }
        Cmd::CronList => {
            let list = automation::tasks(s).await.map_err(err)?;
            let ago = |v: &Value, k: &str| v[k].as_i64().map(fmt_ts).unwrap_or_else(|| "-".into());
            let mut out = vec![format!("{:<36} {:<16} {:<8} {:<8} {:<10} {:<17} next", "task", "schedule", "enabled", "health", "last", "last run")];
            for v in &list {
                let every = v["interval_secs"].as_i64().unwrap_or(0);
                let every = match v["run_hour"].as_i64() {
                    _ if v["schedule"].is_string() => v["schedule"].as_str().unwrap_or("").to_string(),
                    Some(h) => format!("daily {h:02}h"),
                    None if every % 86_400 == 0 => format!("{}d", every / 86_400),
                    None if every % 3600 == 0 => format!("{}h", every / 3600),
                    None => format!("{}m", every / 60),
                };
                out.push(format!("{:<36} {:<16} {:<8} {:<8} {:<10} {:<17} {}", v["name"].as_str().unwrap_or(""), every, if v["tripped_at"].is_i64() { "TRIPPED" } else if v["enabled"].as_bool() == Some(true) { "yes" } else { "NO" }, v["health"].as_str().unwrap_or(""), v["last_status"].as_str().unwrap_or("-"), ago(v, "last_finished_at"), ago(v, "next_run_at")));
            }
            for v in list.iter().filter(|v| v["tripped_at"].is_i64()) {
                out.push(format!("! {} tripped after {} failed runs ({}): `cron reset {}` after fixing it", v["name"].as_str().unwrap_or(""), v["failures"], v["tripped_error"].as_str().unwrap_or("-"), v["name"].as_str().unwrap_or("")));
            }
            out.push(format!("{} task(s); times in UTC; cron schedules and daily run hours in the company time zone", list.len()));
            Ok(out.join("\n"))
        }
        Cmd::CronRun(t) => automation::run_task(s, &t).await.map(|id| match id { Some(id) => format!("queued {t} (job {id})"), None => format!("{t} is already queued this minute") }).map_err(|e| task_err(&t, e)),
        Cmd::CronEnable(t, on) => automation::set_task_enabled(s, &t, on).await.map(|_| format!("{t} {}", if on { "enabled" } else { "disabled (not scheduled automatically; `cron run` still works)" })).map_err(|e| task_err(&t, e)),
        Cmd::CronSchedule(t, expr) => automation::set_task_schedule(s, &t, expr.as_deref())
            .await
            .map(|next| match expr {
                Some(e) => format!("{t} runs on \"{e}\"; next: {}", next.iter().take(3).map(|&n| fmt_ts(n)).collect::<Vec<_>>().join(", ")),
                None => format!("{t} back to its built-in interval"),
            })
            .map_err(|e| match e {
                pelty_core::AppError::Validation(_) => "invalid cron expression: 5 fields (minute hour day-of-month month day-of-week), e.g. \"0 3 * * *\"".to_string(),
                e => task_err(&t, e),
            }),
        Cmd::CronReset(t) => automation::reset_breaker(s, &t).await.map(|_| format!("{t} reset and enabled")).map_err(|e| task_err(&t, e)),
        Cmd::CronMissed(t, m) => automation::set_task_missed(s, &t, &m).await.map(|_| format!("{t}: missed runs {}", if m == "skip" { "are skipped" } else { "run once on restart" })).map_err(|e| task_err(&t, e)),
        Cmd::CronHistory(t, n) => {
            let rows = automation::history(s, &t, n.clamp(1, 200)).await.map_err(|e| task_err(&t, e))?;
            if rows.is_empty() {
                return Ok(format!("{t}: no runs yet"));
            }
            let mut out = vec![format!("{:<8} {:<10} {:<17} {:>9}  result / error", "job", "status", "queued", "took")];
            for r in rows {
                let took = r["duration_ms"].as_i64().map(|d| format!("{d} ms")).unwrap_or_else(|| "-".into());
                let detail = r["error"].as_str().map(str::to_string).unwrap_or_else(|| if r["result"].is_null() { String::new() } else { r["result"].to_string() });
                out.push(format!("{:<8} {:<10} {:<17} {:>9}  {}", r["id"], r["status"].as_str().unwrap_or(""), fmt_ts(r["created_at"].as_i64().unwrap_or(0)), took, detail.chars().take(80).collect::<String>()));
            }
            Ok(out.join("\n"))
        }
        Cmd::AddonsList => {
            let items = addons::list_entries(s).await.map_err(err)?;
            if items.is_empty() {
                return Ok("no addons".into());
            }
            let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
            let mut out = vec![format!("{:<32} {:<10} {:<12} {:<9} name", "id", "version", "kind", "status")];
            for v in &items {
                out.push(format!("{:<32} {:<10} {:<12} {:<9} {}", str_of(v, "id"), str_of(v, "version"), str_of(v, "kind"), str_of(v, "status"), str_of(v, "name")));
            }
            out.push(format!("{} addons", items.len()));
            Ok(out.join("\n"))
        }
        Cmd::AddonsEnable(id) => addons::set_addon_enabled(s, &id, true, Actor::CONSOLE).await.map(|_| format!("enabled {id}")).map_err(err),
        Cmd::AddonsDisable(id) => addons::set_addon_enabled(s, &id, false, Actor::CONSOLE).await.map(|_| format!("disabled {id}")).map_err(err),
        Cmd::AddonsUninstall(id) => addons::uninstall_addon(s, &id, Actor::CONSOLE).await.map(|_| format!("uninstalled {id}")).map_err(err),
        Cmd::AddonsInstall { target, approve } => {
            let path = std::path::Path::new(&target);
            let mut notes = Vec::new();
            let (id, dangerous) = if path.is_file() {
                let bytes = tokio::fs::read(path).await.map_err(|e| format!("{target}: {e}"))?;
                let pkg = pelty_extensions::addon_pkg::inspect(&bytes, addons::PKG_LIMITS).map_err(|e| format!("not a valid addon package: {e}"))?;
                let m: addons::Manifest = serde_json::from_slice(&pkg.manifest_raw).map_err(|_| "addon.json is invalid".to_string())?;
                let dangerous = addons::dangerous_caps(&m);
                if !dangerous.is_empty() && !approve {
                    return Err(format!("{} asks for dangerous capabilities: {}\nre-run with --approve to accept them", m.id, dangerous.join(", ")));
                }
                let e = addons::upload_package(s, &bytes, false, Actor::CONSOLE).await.map_err(err)?;
                notes.push(format!("unpacked {} {}", e.manifest.id, e.manifest.version));
                (e.manifest.id.clone(), dangerous)
            } else if target.ends_with(".zip") || target.contains(['/', '\\']) {
                return Err(format!("{target}: no such file"));
            } else {
                let m = addons::catalog(s).into_iter().find(|e| e.manifest.id == target).ok_or(format!("no addon `{target}` in the catalog (addons list)"))?;
                let dangerous = addons::dangerous_caps(&m.manifest);
                if !dangerous.is_empty() && !approve {
                    return Err(format!("{target} asks for dangerous capabilities: {}\nre-run with --approve to accept them", dangerous.join(", ")));
                }
                (target.clone(), dangerous)
            };
            addons::install_addon(s, &id, &json!({ "approve": dangerous }), Actor::CONSOLE).await.map_err(err)?;
            notes.push(format!("installed {id} (disabled; `addons enable {id}` to turn it on)"));
            Ok(notes.join("\n"))
        }
        Cmd::AddonsHealth => {
            let all = s.addon_health.all();
            if all.is_empty() {
                return Ok("no addon calls since start".into());
            }
            let mut out = vec![format!("{:<32} {:>7} {:>8} {:>8} {:>7} {:>7}  breaker", "addon", "calls", "failures", "timeouts", "skipped", "max ms")];
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
            for (id, st) in all {
                let breaker = match st.tripped_until {
                    Some(t) if t > now => format!("OPEN ({}s)", t - now),
                    _ => "closed".into(),
                };
                out.push(format!("{id:<32} {:>7} {:>8} {:>8} {:>7} {:>7}  {breaker}", st.calls, st.failures, st.timeouts, st.skipped, st.max_ms));
                if let Some(e) = st.last_error {
                    out.push(format!("    last error: {e}"));
                }
            }
            Ok(out.join("\n"))
        }
        Cmd::AdminCreate { email, password, name } => {
            let generated = password.is_none();
            let password = password.unwrap_or_else(generate_password);
            let u = crate::create_admin(&s.auth, &email, &password, name.as_deref().unwrap_or("Administrator")).await?;
            Ok(if generated { format!("admin created: {} ({})\npassword: {password}\n(shown once; change it after signing in)", u.email, u.id) } else { format!("admin created: {} ({})", u.email, u.id) })
        }
        Cmd::AdminResetPassword { email, password } => {
            let generated = password.is_none();
            let password = password.unwrap_or_else(generate_password);
            let revoked = reset_password(s, &email, &password).await.map_err(err)?;
            let mut msg = format!("password of {email} changed; {revoked} session(s) signed out");
            if generated {
                msg.push_str(&format!("\npassword: {password}\n(shown once)"));
            }
            Ok(msg)
        }
        Cmd::UsersList(n) => {
            let (users, total) = pelty_database::users::list(&s.pool, n, 0).await.map_err(err)?;
            let mut out = vec![format!("{:<36} {:<36} {:<20} {:<7} created", "id", "email", "roles", "active")];
            for u in &users {
                let roles = pelty_database::users::roles(&s.pool, u.id).await.map_err(err)?.join(",");
                let created = u.created_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
                out.push(format!("{:<36} {:<36} {:<20} {:<7} {created}", u.id, u.email, roles, u.is_active));
            }
            out.push(format!("{} of {total} users", users.len()));
            Ok(out.join("\n"))
        }
        Cmd::Migrate => pelty_database::migrate(&s.pool).await.map(|_| "migrations applied (database is up to date)".into()).map_err(|e| e.to_string()),
        Cmd::Reload => pelty_api::translations::reload(s).await.map(|n| format!("translation overrides reloaded ({n})")).map_err(err),
        Cmd::LogLevel(None) => Ok(format!("log filter: {}", crate::logging::current_filter())),
        Cmd::LogLevel(Some(f)) => crate::logging::set_filter(&f).map(|_| format!("log filter set to {f}")),
    }
}

/// Same steps as the emailed password reset (policy, hash, sign out
/// everywhere, revoke API keys, audit, event), started from the console.
async fn reset_password(s: &AppState, email: &str, password: &str) -> pelty_core::AppResult<u64> {
    use pelty_database::{audit, sessions, users};
    let email = pelty_auth::service::normalize_email(email)?;
    let u = users::find_by_email(&s.pool, &email).await?.ok_or(pelty_core::AppError::NotFound)?;
    pelty_auth::policy::load(&s.pool).await?.check_password(password, &email)?;
    users::set_password(&s.pool, u.id, &pelty_auth::password::hash(password)?).await?;
    let revoked = sessions::revoke_all(&s.pool, u.id).await?;
    pelty_database::api_keys::revoke_all(&s.pool, u.id).await?;
    audit::record(&s.pool, None, "auth.password_reset", Some(&u.id.to_string()), None, json!({ "sessions_revoked": revoked, "via": "console" })).await?;
    s.emit(pelty_extensions::events::DomainEvent::new("user.password_reset", json!({ "user_id": u.id, "sessions_revoked": revoked })));
    Ok(revoked)
}

type Request = (String, oneshot::Sender<(String, bool)>);

/// Start the console. `tty`: line editor with external printer; else plain
/// line reading. Commands run on the async runtime, one at a time.
pub fn start(ctx: Arc<Ctx>, tty: bool) {
    let (tx, mut rx) = mpsc::channel::<Request>(1);
    tokio::spawn(async move {
        while let Some((line, reply)) = rx.recv().await {
            let (text, stop) = match parse(&line) {
                Ok(None) => (String::new(), false),
                Ok(Some(cmd)) => {
                    let stop = cmd == Cmd::Stop;
                    let changes = matches!(cmd, Cmd::AddonsEnable(_) | Cmd::AddonsDisable(_) | Cmd::AddonsInstall { .. } | Cmd::AddonsUninstall(_));
                    match exec(&ctx, cmd).await {
                        Ok(t) => {
                            if changes {
                                pelty_api::cluster::publish(&ctx.state, pelty_api::cluster::Topic::Addons).await;
                            }
                            (t, stop)
                        }
                        Err(e) => (format!("error: {e}"), false),
                    }
                }
                Err(e) => (e, false),
            };
            let _ = reply.send((text, stop));
        }
    });
    let spawned = std::thread::Builder::new().name("console".into()).spawn(move || {
        if tty {
            tty_loop(tx)
        } else {
            plain_loop(tx)
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "console not started");
    }
}

fn ask(tx: &mpsc::Sender<Request>, line: String) -> Option<(String, bool)> {
    let (rtx, rrx) = oneshot::channel();
    tx.blocking_send((line, rtx)).ok()?;
    rrx.blocking_recv().ok()
}

fn plain_loop(tx: mpsc::Sender<Request>) {
    use std::io::BufRead;
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        match ask(&tx, line) {
            Some((text, stop)) => {
                if !text.is_empty() {
                    println!("{text}");
                }
                if stop {
                    return;
                }
            }
            None => return,
        }
    }
}

fn tty_loop(tx: mpsc::Sender<Request>) {
    use rustyline::error::ReadlineError;
    use rustyline::ExternalPrinter;
    let cfg = rustyline::Config::builder().auto_add_history(true).max_history_size(500).map(|b| b.build());
    let mut rl = match cfg.and_then(rustyline::DefaultEditor::with_config) {
        Ok(rl) => rl,
        Err(e) => {
            tracing::warn!(error = %e, "console: no line editor, reading plain lines");
            return plain_loop(tx);
        }
    };
    match rl.create_external_printer() {
        Ok(mut p) => crate::logging::set_printer(Some(Box::new(move |msg| {
            if p.print(msg.clone()).is_err() {
                print!("{msg}");
            }
        }))),
        Err(e) => tracing::warn!(error = %e, "console: log lines may overwrite the prompt (is TERM set to a real terminal type?)"),
    }
    println!("Console ready. Type `help` for commands, `stop` (or Ctrl+C) to shut down.");
    loop {
        match rl.readline("> ") {
            Ok(line) => match ask(&tx, line) {
                Some((text, stop)) => {
                    if !text.is_empty() {
                        println!("{text}");
                    }
                    if stop {
                        break;
                    }
                }
                None => break,
            },
            Err(ReadlineError::Interrupted) => {
                if let Some((text, _)) = ask(&tx, "stop".into()) {
                    println!("{text}");
                }
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("Console closed (Ctrl+D); the server keeps running. Ctrl+C stops it.");
                break;
            }
            Err(e) => {
                tracing::warn!(error = %e, "console stopped");
                break;
            }
        }
    }
    crate::logging::set_printer(None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse("").unwrap(), None);
        assert_eq!(parse("   ").unwrap(), None);
        assert_eq!(parse("help").unwrap(), Some(Cmd::Help));
        assert_eq!(parse("/stop").unwrap(), Some(Cmd::Stop));
        assert_eq!(parse("STATUS").unwrap(), Some(Cmd::Status));
        assert_eq!(parse("version").unwrap(), Some(Cmd::Version));
        assert_eq!(parse("update --check").unwrap(), Some(Cmd::Update(vec!["--check".into()])));
        assert_eq!(parse("import whmcs --file a.sql --dry-run").unwrap(), Some(Cmd::ImportWhmcs(vec!["--file".into(), "a.sql".into(), "--dry-run".into()])));
        assert!(parse("import blesta").is_err());
        assert_eq!(parse("addons").unwrap(), Some(Cmd::AddonsList));
        assert_eq!(parse("addons list").unwrap(), Some(Cmd::AddonsList));
        assert_eq!(parse("addons enable gateway-stripe").unwrap(), Some(Cmd::AddonsEnable("gateway-stripe".into())));
        assert_eq!(parse("addons disable x").unwrap(), Some(Cmd::AddonsDisable("x".into())));
        assert_eq!(parse("addons uninstall x").unwrap(), Some(Cmd::AddonsUninstall("x".into())));
        assert_eq!(parse("addons health").unwrap(), Some(Cmd::AddonsHealth));
        assert_eq!(parse(r#"addons install "C:\My Addons\a.zip""#).unwrap(), Some(Cmd::AddonsInstall { target: r"C:\My Addons\a.zip".into(), approve: false }));
        assert_eq!(parse("addons install --approve ./a.zip").unwrap(), Some(Cmd::AddonsInstall { target: "./a.zip".into(), approve: true }));
        assert_eq!(parse("admin create a@b.co").unwrap(), Some(Cmd::AdminCreate { email: "a@b.co".into(), password: None, name: None }));
        assert_eq!(parse("admin create a@b.co --name Jane  Doe").unwrap(), Some(Cmd::AdminCreate { email: "a@b.co".into(), password: None, name: Some("Jane Doe".into()) }));
        assert_eq!(parse("admin create a@b.co Secret-Pass-123 --name Ops").unwrap(), Some(Cmd::AdminCreate { email: "a@b.co".into(), password: Some("Secret-Pass-123".into()), name: Some("Ops".into()) }));
        assert!(parse("admin create a@b.co --name").is_err());
        assert_eq!(parse("admin reset-password a@b.co 'p w'").unwrap(), Some(Cmd::AdminResetPassword { email: "a@b.co".into(), password: Some("p w".into()) }));
        assert_eq!(parse("users list").unwrap(), Some(Cmd::UsersList(25)));
        assert_eq!(parse("users list 5").unwrap(), Some(Cmd::UsersList(5)));
        assert_eq!(parse("migrate").unwrap(), Some(Cmd::Migrate));
        assert_eq!(parse("cron").unwrap(), Some(Cmd::CronList));
        assert_eq!(parse("cron run billing.recurring").unwrap(), Some(Cmd::CronRun("billing.recurring".into())));
        assert_eq!(parse("cron disable acme:sync").unwrap(), Some(Cmd::CronEnable("acme:sync".into(), false)));
        assert_eq!(parse("cron history x 5").unwrap(), Some(Cmd::CronHistory("x".into(), 5)));
        assert_eq!(parse("cron schedule x \"0 3 * * 1\"").unwrap(), Some(Cmd::CronSchedule("x".into(), Some("0 3 * * 1".into()))));
        assert_eq!(parse("cron schedule x 0 3 * * 1").unwrap(), Some(Cmd::CronSchedule("x".into(), Some("0 3 * * 1".into()))));
        assert_eq!(parse("cron schedule x default").unwrap(), Some(Cmd::CronSchedule("x".into(), None)));
        assert_eq!(parse("cron reset acme:sync").unwrap(), Some(Cmd::CronReset("acme:sync".into())));
        assert_eq!(parse("cron missed x run_once").unwrap(), Some(Cmd::CronMissed("x".into(), "run_once".into())));
        assert!(parse("cron missed x always").is_err());
        assert!(parse("cron history x many").is_err() && parse("cron run").is_err());
        assert_eq!(parse("cluster").unwrap(), Some(Cmd::Cluster));
        assert_eq!(parse("reload").unwrap(), Some(Cmd::Reload));
        assert_eq!(parse("log level").unwrap(), Some(Cmd::LogLevel(None)));
        assert_eq!(parse("log level info,sqlx=debug").unwrap(), Some(Cmd::LogLevel(Some("info,sqlx=debug".into()))));
    }

    #[test]
    fn rejects_bad_input() {
        for bad in ["frobnicate", "addons enable", "addons install", "addons install a b", "addons nope", "admin create", "admin delete x", "users list 0", "users list x", "stop now", "log", "addons install \"unterminated"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert!(parse("frobnicate").unwrap_err().contains("help"));
    }

    #[test]
    fn splits_words() {
        assert_eq!(split(r#"a  "b c" '' d"#).unwrap(), vec!["a", "b c", "", "d"]);
    }

    #[test]
    fn generated_passwords_pass_policy() {
        for _ in 0..50 {
            let p = generate_password();
            assert_eq!(p.len(), 20);
            pelty_auth::password::check_policy(&p, "admin@example.com").unwrap();
        }
    }

    #[test]
    fn formats_uptime() {
        assert_eq!(uptime(std::time::Duration::from_secs(65)), "1m 05s");
        assert_eq!(uptime(std::time::Duration::from_secs(3 * 3600 + 61)), "3h 01m 01s");
        assert_eq!(uptime(std::time::Duration::from_secs(2 * 86400 + 3600)), "2d 1h 00m");
    }
}
