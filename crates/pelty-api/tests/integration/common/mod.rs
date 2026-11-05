#![allow(dead_code)]
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use pelty_api::AppState;
use pelty_core::config::Config;
use pelty_core::i18n::I18n;
use serde_json::Value;
use std::path::PathBuf;
use tower::ServiceExt;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Real Postgres required: TEST_DATABASE_URL (default: local dev instance).
pub async fn app_with(rate: u32) -> (Router, AppState) {
    let url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres@127.0.0.1:55432/peltycms_test".into());
    std::env::set_var("DATABASE_URL", &url);
    if std::env::var_os("PELTY_ADDONS_DIST").is_none() {
        std::env::set_var("PELTY_ADDONS_DIST", root().join("extensions/addons/dist"));
    }
    if std::env::var_os("PELTY_ADDONS_DIR").is_none() {
        std::env::set_var(
            "PELTY_ADDONS_DIR",
            std::env::temp_dir().join("pelty-test-addons"),
        );
    }
    let mut cfg = Config::from_env().expect("config");
    cfg.cookie_secure = false;
    cfg.auth_rate_per_minute = rate;
    cfg.themes_dir = root().join("extensions/themes");
    cfg.plugins_dir = root().join("extensions/plugins");
    // Test-only key (never used outside the test binary).
    cfg.encryption_key = Some(pelty_core::config::Redacted(
        "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=".into(),
    ));
    let suite = enter_suite().await;
    let pool = pelty_database::connect(&url, 5).await.expect("db");
    hold_for_test(pool.clone(), suite);
    pelty_database::migrate(&pool).await.expect("migrate");
    scrub_leftovers(&pool).await;
    let i18n = I18n::load(&root().join("locales"), "en").expect("locales");
    let mut state = AppState::new(pool, cfg, i18n);
    state.mailer = std::sync::Arc::new(mailbox().clone());
    // Offline transport: integration tests must never reach a real payment API.
    state.gateways = std::sync::Arc::new(gateways(pelty_payments::GatewayRegistry::default()));
    state.set_http(std::sync::Arc::new(pelty_payments::NoTransport));
    state.init().await.expect("init");
    registrars(&state.registrars);
    // Fresh installs start with no addons; integration tests use them all
    // (the bundled WASM addons from `cargo xtask addons`).
    {
        static SEED: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let _g = SEED.lock().await;
        pelty_api::addons::install_builtins(&state)
            .await
            .expect("addons");
    }
    (pelty_api::router(state.clone()), state)
}

/// Once per suite (before its first test gets a pool): reset shared global
/// state that a suite changes and only restores at its end, so a run that
/// was killed half-way, or a suite that panicked earlier in this run, cannot
/// make other suites fail (e.g. order auto-approval left on, or required
/// client fields left behind). Suites run one at a time (see below), so this
/// is safe as long as two test binaries do not share the database at once.
async fn scrub_leftovers(pool: &sqlx::PgPool) {
    type Cells = std::sync::Mutex<
        std::collections::HashMap<String, std::sync::Arc<tokio::sync::OnceCell<()>>>,
    >;
    static DONE: std::sync::OnceLock<Cells> = std::sync::OnceLock::new();
    let key = current_suite().unwrap_or_default();
    let cell = DONE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(key)
        .or_default()
        .clone();
    cell.get_or_init(|| async {
        for sql in [
            // Global settings with a non-default value in some suite.
            "DELETE FROM system_settings WHERE key IN ('orders.approval', 'billing.tax')",
            // addon_power_flow's external-login user: wrong-password checks
            // of earlier runs lock it out for a while.
            "UPDATE users SET locked_until = NULL, failed_logins = 0 WHERE email = 'alice@ext.test'",
            // Hook/filter fixtures left enabled by another suite rewrite core
            // data (e.g. integration-eventlog appends " (2)" to every new
            // invoice line). Suites that need one install it again.
            "UPDATE installed_addons SET enabled = FALSE WHERE id IN ('integration-eventlog', 'integration-extdemo', 'integration-hooks', 'integration-points', 'integration-powerdemo', 'integration-listener', 'integration-lifecycle')",
            // Tax rules (tax_flow leaves EU/CA rules on, which re-runs of
            // price-asserting suites would then pick up).
            "DELETE FROM tax_rules",
            // Client custom fields are global (a required one blocks every profile save).
            "DELETE FROM client_fields",
            // Jobs left queued by earlier suites or killed runs (hundreds of
            // retrying mails) would starve tests that pump the queue.
            "UPDATE jobs SET status = 'dead' WHERE status IN ('queued', 'running')",
        ] {
            if let Err(e) = sqlx::query(sql).execute(pool).await {
                eprintln!("scrub_leftovers: {sql}: {e}");
            }
        }
    })
    .await;
}

/// All suites (the former `tests/*.rs` files) share one test binary. Two things
/// keep that equivalent to one binary per suite:
///
/// * Suites mutate DB-global state (security policy, captcha, mail templates,
///   overdue sweeps over every invoice, ...), and cargo used to run the old
///   binaries one after another. [`SuiteGate`] lets tests of the same suite run
///   in parallel but never two suites at once.
/// * An `AppState` keeps its pool alive after its test ends; per process that
///   piled up connections until Postgres hit `max_connections`. The pool is
///   closed when the test's runtime shuts down.
///
/// Both are released by a guard owned by a task of the test's runtime, which is
/// dropped when the `#[tokio::test]` returns or panics.
fn hold_for_test(pool: sqlx::PgPool, suite: Option<SuiteTicket>) {
    struct Guard(Option<sqlx::PgPool>, Option<SuiteTicket>);
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(p) = self.0.take() {
                reaper().spawn(async move { p.close().await });
            }
        }
    }
    let g = Guard(Some(pool), suite);
    tokio::spawn(async move {
        let _g = g;
        std::future::pending::<()>().await
    });
}

/// Suite of the running test: libtest names the test thread `suite::test`.
fn current_suite() -> Option<String> {
    let t = std::thread::current();
    let name = t.name()?;
    name.split_once("::").map(|(m, _)| m.to_string())
}

/// One suite at a time, any number of its tests in parallel. libtest starts
/// tests in name order, so a waiting test only ever waits on earlier suites
/// that are already running: no deadlock.
struct SuiteGate {
    state: std::sync::Mutex<(Option<String>, usize)>,
    changed: tokio::sync::Notify,
}

struct SuiteTicket;

impl Drop for SuiteTicket {
    fn drop(&mut self) {
        let mut s = gate().state.lock().unwrap_or_else(|e| e.into_inner());
        s.1 -= 1;
        if s.1 == 0 {
            s.0 = None;
            gate().changed.notify_waiters();
        }
    }
}

fn gate() -> &'static SuiteGate {
    static G: std::sync::OnceLock<SuiteGate> = std::sync::OnceLock::new();
    G.get_or_init(|| SuiteGate {
        state: std::sync::Mutex::new((None, 0)),
        changed: tokio::sync::Notify::new(),
    })
}

async fn enter_suite() -> Option<SuiteTicket> {
    let suite = current_suite()?;
    loop {
        let notified = gate().changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        {
            let mut s = gate().state.lock().unwrap_or_else(|e| e.into_inner());
            if s.0.is_none() || s.0.as_deref() == Some(suite.as_str()) {
                s.0 = Some(suite);
                s.1 += 1;
                return Some(SuiteTicket);
            }
        }
        notified.await;
    }
}

/// Long-lived runtime that closes pools whose test runtime is gone.
fn reaper() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .thread_name("test-pool-reaper")
            .enable_all()
            .build()
            .expect("reaper runtime")
    })
}

/// Shared in-memory mailbox for the test binary; filter by recipient.
pub fn mailbox() -> &'static pelty_notifications::MemoryMailer {
    static M: std::sync::OnceLock<pelty_notifications::MemoryMailer> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Run the job worker until the queue is drained, then return mail sent to `to`.
pub async fn mails_to(state: &AppState, to: &str) -> Vec<pelty_notifications::Email> {
    // Deterministic: wait only for *this recipient's* email jobs (other tests
    // share the queue), pulling any retry backoff forward so a transient
    // failure can't make us return early. Bounded at ~30s.
    for _ in 0..1500 {
        sqlx::query("UPDATE jobs SET run_at = now() WHERE kind LIKE 'email.send%' AND status = 'queued' AND payload->>'to' = $1 AND run_at > now()")
            .bind(to).execute(&state.pool).await.expect("bump");
        pelty_api::worker::run_once(state, "test-mail", 50)
            .await
            .expect("worker");
        let pending: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind LIKE 'email.send%' AND status IN ('queued','running') AND payload->>'to' = $1")
            .bind(to).fetch_one(&state.pool).await.expect("count");
        if pending == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    mailbox()
        .sent
        .lock()
        .unwrap()
        .iter()
        .filter(|m| m.to == to)
        .cloned()
        .collect()
}

/// Mail delivery order isn't guaranteed (parallel workers), so split two
/// tokens into (superseded, live) by asking the DB which one is still valid.
#[allow(dead_code)]
pub async fn stale_and_live(state: &AppState, a: String, b: String) -> (String, String) {
    let live: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM user_tokens WHERE token_hash = $1 AND used_at IS NULL)",
    )
    .bind(&pelty_auth::token::hash(&b)[..])
    .fetch_one(&state.pool)
    .await
    .expect("token lookup");
    if live {
        (a, b)
    } else {
        (b, a)
    }
}

/// Pull `token=...` out of the first link in a mail body.
pub fn token_from(mail: &pelty_notifications::Email) -> String {
    let i = mail.text.find("token=").expect("token link") + 6;
    mail.text[i..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

pub async fn app() -> (Router, AppState) {
    app_with(1000).await
}

pub fn email() -> String {
    format!("u{}@pelty.test", uuid::Uuid::new_v4().simple())
}

pub struct Resp {
    pub status: StatusCode,
    pub body: Value,
    pub cookies: Vec<String>,
}

pub async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Resp {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .expect("request");
    let res = app.clone().oneshot(req).await.expect("response");
    let status = res.status();
    let cookies = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Resp {
        status,
        body,
        cookies,
    }
}

/// Register + login; returns (cookie header, csrf token).
pub async fn login_new(app: &Router, state: &AppState, role: &str) -> (String, String, String) {
    let e = email();
    state
        .auth
        .register(
            pelty_auth::RegisterInput {
                email: &e,
                password: "Very-Long-Pass-123",
                display_name: "T",
            },
            role,
            None,
        )
        .await
        .expect("register");
    let r = call(
        app,
        "POST",
        "/api/v1/auth/login",
        Some(serde_json::json!({"email": e, "password": "Very-Long-Pass-123"})),
        &[],
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let c = r
        .cookies
        .iter()
        .find(|c| c.starts_with("pelty_session="))
        .expect("cookie");
    let cookie = c.split(';').next().expect("kv").to_string();
    (
        cookie,
        r.body["csrf_token"].as_str().expect("csrf").to_string(),
        e,
    )
}

/// Logged-in session as (cookie, csrf) with header helper.
pub struct Sess {
    pub cookie: String,
    pub csrf: String,
    pub email: String,
}
impl Sess {
    pub fn h(&self) -> [(&str, &str); 2] {
        [
            ("cookie", self.cookie.as_str()),
            ("x-csrf-token", self.csrf.as_str()),
        ]
    }
}
pub async fn sess(app: &Router, state: &AppState, role: &str) -> Sess {
    let (cookie, csrf, email) = login_new(app, state, role).await;
    Sess {
        cookie,
        csrf,
        email,
    }
}

/// Create a group + product priced 10.00 EUR monthly (+5 setup) / 100 annual,
/// with a required "Location" option (EU free, US +2.50 monthly).
/// Returns (product_id, option_id, [eu_choice, us_choice]).
pub async fn seed_product(app: &Router, admin: &Sess) -> (String, String, [String; 2]) {
    let slug = format!("g{}", &uuid::Uuid::new_v4().simple().to_string()[..10]);
    let g = call(
        app,
        "POST",
        "/api/v1/admin/catalog/groups",
        Some(serde_json::json!({"name": "VPS", "slug": slug})),
        &admin.h(),
    )
    .await;
    assert_eq!(g.status, StatusCode::OK, "{:?}", g.body);
    let p = call(app, "POST", "/api/v1/admin/catalog/products", Some(serde_json::json!({
        "group_id": g.body["id"], "name": "VPS S", "slug": format!("{slug}-s"), "module": "example_vps", "module_config": {"node": "n1"}
    })), &admin.h()).await;
    assert_eq!(p.status, StatusCode::OK, "{:?}", p.body);
    let pid = p.body["id"].as_str().expect("pid").to_string();
    let r = call(
        app,
        "PUT",
        &format!("/api/v1/admin/catalog/products/{pid}/prices"),
        Some(serde_json::json!({"prices": [
            {"currency": "EUR", "cycle": "monthly", "amount": "10.00", "setup_fee": "5.00"},
            {"currency": "EUR", "cycle": "annual", "amount": "100.00"},
            {"currency": "JPY", "cycle": "monthly", "amount": "1500"}
        ]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let r = call(
        app,
        "POST",
        &format!("/api/v1/admin/catalog/products/{pid}/options"),
        Some(serde_json::json!({
        "name": "Location", "choices": [
            {"label": "EU"},
            {"label": "US", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "2.50"}]}
        ]})),
        &admin.h(),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let oid = r.body["id"].as_str().expect("oid").to_string();
    let full = call(
        app,
        "GET",
        "/api/v1/admin/catalog/products",
        None,
        &admin.h(),
    )
    .await;
    let prod = full.body["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|x| x["id"] == pid.as_str())
        .expect("prod")
        .clone();
    let ch = &prod["options"][0]["choices"];
    (
        pid,
        oid,
        [
            ch[0]["id"].as_str().expect("c").into(),
            ch[1]["id"].as_str().expect("c").into(),
        ],
    )
}

/// Build a fixture addon `extensions/addons/<dir>` (crate `crate_name`) to wasm and pack
/// it with its addon.json. Cached per process.
pub fn fixture_zip(dir: &str, crate_name: &str) -> Vec<u8> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(z) = cache.lock().unwrap().get(dir) {
        return z.clone();
    }
    let target = std::env::var("PELTY_ADDONS_TARGET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("pelty-addons-target"));
    let st = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-wasip2",
            "-p",
            crate_name,
            "--target-dir",
        ])
        .arg(&target)
        .current_dir(root().join("extensions/addons"))
        .status()
        .expect("run cargo");
    assert!(st.success(), "fixture build failed for {crate_name}");
    let wasm = std::fs::read(target.join(format!(
        "wasm32-wasip2/release/{}.wasm",
        crate_name.replace('-', "_")
    )))
    .unwrap();
    let pkg = std::env::temp_dir().join(format!(
        "pelty-api-fixture-{}-{}",
        dir.replace(['/', '.'], "_"),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&pkg);
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join("addon.wasm"), wasm).unwrap();
    std::fs::copy(
        root()
            .join("extensions/addons")
            .join(dir)
            .join("addon.json"),
        pkg.join("addon.json"),
    )
    .unwrap();
    let assets = root().join("extensions/addons").join(dir).join("assets");
    if assets.is_dir() {
        std::fs::create_dir_all(pkg.join("assets")).unwrap();
        for e in std::fs::read_dir(&assets).unwrap().flatten() {
            std::fs::copy(e.path(), pkg.join("assets").join(e.file_name())).unwrap();
        }
    }
    pelty_extensions::addon_pkg::rehash(&pkg).unwrap();
    let z = pelty_extensions::addon_pkg::pack(&pkg).unwrap();
    let _ = std::fs::remove_dir_all(&pkg);
    cache.lock().unwrap().insert(dir.into(), z.clone());
    z
}

/// Copy a source-tree addon directory (`extensions/addons/<id>`) into the test addons dir.
pub fn copy_disk_addon(id: &str) {
    fn cp(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                cp(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }
    cp(
        &root().join("extensions/addons").join(id),
        &pelty_api::addons::addons_dir().join(id),
    );
}

/// Registers every HTTP registrar addon (natively linked, sharing the registry transport).
#[allow(dead_code)]
pub fn registrars(r: &pelty_provisioning::registrar::RegistrarRegistry) {
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_namecheap::namecheap::Namecheap::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_resellerclub::resellerclub::ResellerClub::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_enom::enom::Enom::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_opensrs::opensrs::OpenSrs::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_godaddy::godaddy::GoDaddy::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_centralnic::centralnic::CentralNic::new(r.transport()),
    ));
    r.register_dyn(std::sync::Arc::new(
        pelty_addon_registrar_generic::generic::Generic::new(r.transport()),
    ));
}

/// Registers every gateway addon (natively linked) like the host does after loading them.
#[allow(dead_code)]
pub fn gateways(mut r: pelty_payments::GatewayRegistry) -> pelty_payments::GatewayRegistry {
    r.register(
        pelty_addon_gateway_stripe::stripe::descriptor(),
        pelty_addon_gateway_stripe::stripe::factory,
    );
    r.register(
        pelty_addon_gateway_paypal::paypal::descriptor(),
        pelty_addon_gateway_paypal::paypal::factory,
    );
    r.register(
        pelty_addon_gateway_mollie::mollie::descriptor(),
        pelty_addon_gateway_mollie::mollie::factory,
    );
    r.register(
        pelty_addon_gateway_razorpay::razorpay::descriptor(),
        pelty_addon_gateway_razorpay::razorpay::factory,
    );
    r.register(
        pelty_addon_gateway_gocardless::gocardless::descriptor(),
        pelty_addon_gateway_gocardless::gocardless::factory,
    );
    r.register(
        pelty_addon_gateway_crypto::crypto::descriptor(),
        pelty_addon_gateway_crypto::crypto::factory,
    );
    r.register(
        pelty_addon_gateway_paddle::paddle::descriptor(),
        pelty_addon_gateway_paddle::paddle::factory,
    );
    r.register(
        pelty_addon_gateway_adyen::adyen::descriptor(),
        pelty_addon_gateway_adyen::adyen::factory,
    );
    r.register(
        pelty_addon_gateway_manual::manual::descriptor(),
        pelty_addon_gateway_manual::manual::factory,
    );
    r.register(
        pelty_addon_gateway_custom::custom::descriptor(),
        pelty_addon_gateway_custom::custom::factory,
    );
    r
}

/// A 3-letter currency code `<prefix>XY` that does not exist yet. Earlier
/// runs on the same database leave their codes behind (676 per prefix).
#[allow(dead_code)]
pub async fn fresh_currency_code(app: &axum::Router, admin: &Sess, prefix: char) -> String {
    let existing = call(app, "GET", "/api/v1/admin/currencies", None, &admin.h())
        .await
        .body
        .to_string();
    for _ in 0..2000 {
        let b = *uuid::Uuid::new_v4().as_bytes();
        let c = format!(
            "{prefix}{}{}",
            (b'A' + b[0] % 26) as char,
            (b'A' + b[1] % 26) as char
        );
        if !existing.contains(&format!("\"{c}\"")) {
            return c;
        }
    }
    panic!("no free currency code with prefix {prefix}: clean the test database");
}

/// Runs only the queued `kind` jobs whose payload mentions `needle` (a test's
/// unique tag), ignoring `run_at` backoff, until none is queued or running.
/// Unlike `worker::run_once` it never picks up (or waits behind) jobs other
/// suites left in the shared queue. Returns how many ran.
pub async fn drain_jobs(s: &AppState, kind: &str, needle: &str) -> usize {
    let mut ran = 0;
    for _ in 0..200 {
        let claimed: Vec<pelty_database::jobs::Job> = sqlx::query_as(
            "UPDATE jobs SET status = 'running', locked_at = now(), locked_by = 'drain-test', attempts = attempts + 1
             WHERE id IN (SELECT id FROM jobs WHERE kind = $1 AND status = 'queued' AND strpos(payload::text, $2) > 0 ORDER BY id FOR UPDATE SKIP LOCKED)
             RETURNING id, kind, payload, status, attempts, max_attempts, run_at, locked_by, last_error, idempotency_key, result, created_at, finished_at",
        )
        .bind(kind)
        .bind(needle)
        .fetch_all(&s.pool)
        .await
        .unwrap();
        ran += claimed.len();
        pelty_api::worker::run_claimed(s, &claimed).await.unwrap();
        let left: i64 = sqlx::query_scalar("SELECT count(*) FROM jobs WHERE kind = $1 AND status IN ('queued', 'running') AND strpos(payload::text, $2) > 0")
            .bind(kind)
            .bind(needle)
            .fetch_one(&s.pool)
            .await
            .unwrap();
        if left == 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    ran
}

/// Like [`call`] but from a given client IP (sets `ConnectInfo`).
pub async fn call_from(
    app: &Router,
    ip: &str,
    method: &str,
    uri: &str,
    body: Option<Value>,
    headers: &[(&str, &str)],
) -> Resp {
    let mut b = Request::builder().method(method).uri(uri);
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    let mut req = match body {
        Some(v) => b
            .header("content-type", "application/json")
            .body(Body::from(v.to_string())),
        None => b.body(Body::empty()),
    }
    .expect("request");
    let addr: std::net::SocketAddr = if ip.contains(':') {
        format!("[{ip}]:5000")
    } else {
        format!("{ip}:5000")
    }
    .parse()
    .expect("ip");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(addr));
    let res = app.clone().oneshot(req).await.expect("response");
    let status = res.status();
    let cookies = res
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect();
    let bytes = res.into_body().collect().await.expect("body").to_bytes();
    Resp {
        status,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        cookies,
    }
}
