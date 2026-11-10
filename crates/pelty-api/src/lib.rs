//! HTTP layer: routing, extractors, error mapping. Business logic lives in
//! service crates; SQL lives in `pelty-database`.
pub mod addon_host;
pub use addon_host::addon_events;
pub use addon_host::addon_ext;
pub use addon_host::addon_filters;
pub use addon_host::addon_functions;
pub use addon_host::addon_health;
pub use addon_host::addon_power;
pub use addon_host::addon_secrets;
pub use addon_host::addon_trust;
pub use addon_host::addon_upgrade;
pub use addon_host::addons;
pub use addon_host::plugin_hooks;
pub use addon_host::wasm_addons;
pub mod money;
pub use money::autopay;
pub use money::einvoice;
pub use money::exchange_rates;
pub use money::overdue;
pub use money::pdf;
pub use money::vies;
pub mod comms;
pub use comms::chat_bus;
pub use comms::event_mail;
pub use comms::mail;
pub use comms::mailbox;
pub use comms::notify;
pub use comms::pipe;
pub use comms::support_ext;
pub use comms::webhooks_out;
pub mod access;
pub use access::captcha;
pub use access::duo;
pub use access::fraud;
pub use access::geoip;
pub use access::oauth;
pub use access::oidc_provider;
pub use access::saml;
pub use access::security;
pub use access::security_headers;
pub use access::trusted_proxy;
pub mod hosting;
pub use hosting::cluster;
pub use hosting::domains;
pub use hosting::provision;
pub use hosting::registrant_verify;
pub use hosting::ssl_probe;
pub mod platform;
pub use platform::approval;
pub use platform::backup;
pub use platform::changes;
pub use platform::cron;
pub use platform::csv;
pub use platform::dev_docs;
pub use platform::extract;
pub use platform::static_files;
pub use platform::translations;
pub use platform::updates;
pub use platform::whmcs_api;
pub use platform::worker;
pub mod error;
pub mod routes;
pub mod shared;

use axum::http::HeaderName;
use axum::{middleware, Router};
use pelty_auth::rate_limit::RateLimiter;
use pelty_auth::AuthService;
use pelty_core::config::Config;
use pelty_core::i18n::I18n;
use pelty_database::Pool;
use pelty_extensions::plugins::{EventLogger, PluginRegistry};
use pelty_extensions::themes::ThemeStore;
use pelty_payments::GatewayRegistry;
use std::sync::{Arc, RwLock};
use tower_http::compression::CompressionLayer;
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: Pool,
    pub chat_bus: Arc<chat_bus::ChatBus>,
    pub cfg: Arc<Config>,
    pub auth: AuthService,
    pub i18n: Arc<I18n>,
    pub auth_limiter: Arc<RateLimiter>,
    /// Optional Redis shared by all instances (`PELTY_REDIS_URL`).
    pub shared: Arc<shared::Shared>,
    pub themes: Arc<ThemeStore>,
    pub plugins: Arc<PluginRegistry>,
    pub event_logger: Arc<EventLogger>,
    pub gateways: Arc<GatewayRegistry>,
    /// Cached active theme id; use `active_theme()` / `set_active_theme()`.
    pub active_theme_cache: Arc<RwLock<String>>,
    /// Outbound mail transport (SMTP, or a logger when unconfigured).
    pub mailer: Arc<dyn pelty_notifications::Mailer>,
    /// Mailer built from the admin SMTP settings; overrides `mailer` when set.
    pub smtp_runtime: Arc<RwLock<Option<Arc<dyn pelty_notifications::Mailer>>>>,
    /// Sealing key for secrets at rest; `None` when PELTY_ENCRYPTION_KEY is unset.
    pub secret_box: Option<pelty_auth::crypto::SecretBox>,
    /// Installed provisioning modules (built-ins; extend via `register`).
    pub provisioning: Arc<pelty_provisioning::Registry>,
    /// Extra fraud checks run on every client order (see `fraud`).
    pub fraud_hooks: Arc<RwLock<Vec<Arc<dyn fraud::FraudHook>>>>,
    /// Domain registrar modules (built-in: `mock`).
    pub registrars: Arc<pelty_provisioning::registrar::RegistrarRegistry>,
    /// Outbound HTTP for non-gateway integrations (exchange rates, VIES).
    /// Swappable so tests can inject a mock; use [`AppState::http`].
    pub http_transport: Arc<RwLock<Arc<dyn pelty_payments::http::HttpTransport>>>,
    /// Loaded captcha / SSO WASM addons, keyed `kind:engine`.
    /// Addons whose mixins the admin switched off.
    pub mixins_off: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Per-addon call statistics and circuit breaker (`addon_health`).
    pub addon_health: Arc<addon_health::Health>,
    pub wasm_addons: Arc<RwLock<std::collections::BTreeMap<String, Arc<pelty_wasm::WasmAddon>>>>,
    /// Addon-private key/value store (DB-backed).
    pub addon_kv: Arc<dyn pelty_wasm::KvStore>,
    /// Staff notification transport (Slack/Discord/Teams/webhook); mockable.
    pub notify_transport: Arc<dyn notify::NotifyTransport>,
    /// Opens department mailboxes for ticket piping (IMAP/POP3); mockable.
    pub mailbox: Arc<dyn mailbox::MailboxConnector>,
    /// Optional MMDB for IP → city/country (login alerts, etc.).
    pub geoip: Arc<geoip::GeoIp>,
}

impl AppState {
    /// `OrderService` over this state's pool.
    pub fn orders(&self) -> pelty_database::OrderService {
        pelty_database::OrderService::new(self.pool.clone())
    }

    /// `HostingService` over this state's pool.
    pub fn hosting(&self) -> pelty_database::HostingService {
        pelty_database::HostingService::new(self.pool.clone())
    }

    /// `DomainService` over this state's pool.
    pub fn domains(&self) -> pelty_database::DomainService {
        pelty_database::DomainService::new(self.pool.clone())
    }

    /// `InvoiceService` over this state's pool.
    pub fn invoices(&self) -> pelty_database::InvoiceService {
        pelty_database::InvoiceService::new(self.pool.clone())
    }

    /// `ClientService` over this state's pool.
    pub fn clients(&self) -> pelty_database::ClientService {
        pelty_database::ClientService::new(self.pool.clone())
    }

    /// `TicketService` over this state's pool.
    pub fn tickets(&self) -> pelty_database::TicketService {
        pelty_database::TicketService::new(self.pool.clone())
    }

    pub fn new(pool: Pool, cfg: Config, mut i18n: I18n) -> Self {
        let auth = AuthService {
            pool: pool.clone(),
            idle: time::Duration::minutes(cfg.session_idle_minutes),
            absolute: time::Duration::hours(cfg.session_absolute_hours),
        };
        let auth_limiter = Arc::new(RateLimiter::new(
            cfg.auth_rate_per_minute,
            std::time::Duration::from_secs(60),
        ));
        let shared = Arc::new(shared::Shared::new(
            cfg.redis_url.as_ref().map(|r| r.0.clone()),
        ));
        if shared.enabled() {
            auth_limiter.set_shared(Arc::new(shared::RedisCounter(shared.clone())));
        }
        let themes = Arc::new(ThemeStore::load(&cfg.themes_dir));
        let event_logger = Arc::new(EventLogger::default());
        let mut plugins = PluginRegistry::new();
        plugins.register(event_logger.clone());
        plugins.load_external(&cfg.plugins_dir);
        // Extension strings live in their own namespace: plugin.<id>.* / theme.<id>.*
        for p in plugins.list() {
            let id = &p.manifest.id;
            if let Err(e) = i18n.merge_namespace(
                &format!("plugin.{id}."),
                &cfg.plugins_dir.join(id).join("locales"),
            ) {
                tracing::warn!(plugin = %id, error = %e, "plugin locales skipped");
            }
        }
        for t in themes.list() {
            if let Err(e) = i18n.merge_namespace(
                &format!("theme.{}.", t.id),
                &cfg.themes_dir.join(&t.id).join("locales"),
            ) {
                tracing::warn!(theme = %t.id, error = %e, "theme locales skipped");
            }
        }
        let mailer: Arc<dyn pelty_notifications::Mailer> = match &cfg.smtp_url {
            Some(url) => match pelty_notifications::SmtpMailer::from_url(&url.0, &cfg.mail_from) {
                Ok(m) => Arc::new(m),
                Err(e) => {
                    tracing::error!(error = %e, "invalid SMTP configuration; email will only be logged");
                    Arc::new(pelty_notifications::LogMailer)
                }
            },
            None => Arc::new(pelty_notifications::LogMailer),
        };
        let secret_box = cfg.encryption_key.as_ref().map(|k| {
            pelty_auth::crypto::SecretBox::from_base64(&k.0)
                .unwrap_or_else(|e| panic!("invalid PELTY_ENCRYPTION_KEY: {e}"))
        });
        let addon_kv: Arc<dyn pelty_wasm::KvStore> = Arc::new(wasm_addons::PgKv {
            pool: pool.clone(),
            secret_box: secret_box.clone(),
        });
        let geoip = geoip::GeoIp::load(cfg.geoip_db.as_deref());
        AppState {
            provisioning: Arc::new(pelty_provisioning::Registry::default()),
            fraud_hooks: Arc::new(RwLock::new(Vec::new())),
            registrars: Arc::new(pelty_provisioning::registrar::RegistrarRegistry::default()),
            notify_transport: notify::default_transport(),
            mailbox: Arc::new(mailbox::TcpConnector),
            secret_box,
            mailer,
            smtp_runtime: Arc::new(RwLock::new(None)),
            pool,
            chat_bus: Arc::new(chat_bus::ChatBus::default()),
            cfg: Arc::new(cfg),
            auth,
            i18n: Arc::new(i18n),
            auth_limiter,
            shared,
            themes,
            plugins: Arc::new(plugins),
            event_logger,
            gateways: Arc::new(GatewayRegistry::empty(Arc::new(pelty_wasm::GuardedHttp))),
            active_theme_cache: Arc::new(RwLock::new("default".into())),
            http_transport: Arc::new(RwLock::new(
                Arc::new(pelty_wasm::GuardedHttp) as Arc<dyn pelty_payments::http::HttpTransport>
            )),
            wasm_addons: Arc::new(RwLock::new(Default::default())),
            mixins_off: Arc::new(RwLock::new(Default::default())),
            addon_health: Arc::new(Default::default()),
            addon_kv,
            geoip,
        }
    }

    /// Current outbound HTTP transport (cloned out of the lock).
    pub fn http(&self) -> Arc<dyn pelty_payments::http::HttpTransport> {
        match self.http_transport.read() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    /// Replace the outbound HTTP transport (tests inject a mock).
    pub fn set_http(&self, t: Arc<dyn pelty_payments::http::HttpTransport>) {
        match self.http_transport.write() {
            Ok(mut g) => *g = t,
            Err(p) => *p.into_inner() = t,
        }
    }

    /// Load persisted plugin enable/disable state.
    pub async fn init(&self) -> pelty_core::AppResult<()> {
        addon_trust::keys(self)?;
        for (id, on) in pelty_database::extensions::plugin_states(&self.pool).await? {
            self.plugins.set_enabled(&id, on);
        }
        let id = routes::extensions::load_active_id(self).await?;
        self.set_active_theme(&id);
        routes::smtp::reload(self).await?;
        security::init(self).await?;
        wasm_addons::seed_bundled(self).await?;
        wasm_addons::load_enabled(self).await?;
        addon_events::load_kill_switch(self).await;
        if let Ok(cfg) = pelty_database::fraud::get_settings(&self.pool).await {
            fraud::sync_http_hook(self, &cfg);
        }
        Ok(())
    }

    /// The mailer to deliver with: admin SMTP settings if enabled, else the
    /// env-configured transport (PELTY_SMTP_URL) or the log fallback.
    pub fn active_mailer(&self) -> Arc<dyn pelty_notifications::Mailer> {
        self.smtp_runtime
            .read()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| self.mailer.clone())
    }

    pub fn active_theme(&self) -> String {
        self.active_theme_cache
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "default".into())
    }

    pub fn set_active_theme(&self, id: &str) {
        if let Ok(mut g) = self.active_theme_cache.write() {
            *g = id.to_string();
        }
    }

    /// Publish a domain event to plugins without blocking the request.
    pub fn emit(&self, event: pelty_extensions::events::DomainEvent) {
        let plugins = self.plugins.clone();
        let state = self.clone();
        tokio::spawn(async move {
            crate::provision::on_event(&state, &event).await;
            crate::approval::on_event(&state, &event).await;
            crate::routes::credit::auto_apply_on_event(&state, &event).await;
            plugins.dispatch(&event).await;
            crate::plugin_hooks::on_event(&state, &event).await;
            crate::notify::on_event(&state, &event).await;
            crate::webhooks_out::on_event(&state, &event).await;
            crate::event_mail::on_event(&state, &event).await;
            crate::wasm_addons::on_event(&state, &event).await;
        });
    }
}

/// Full application router: API, health, and the active theme's frontend
/// (or the default `frontend/dist`) as SPA fallback.
pub fn router(state: AppState) -> Router {
    let x_request_id = HeaderName::from_static("x-request-id");
    Router::new()
        .merge(routes::health::router())
        .nest(
            "/api/v1",
            routes::api_router()
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    addon_events::lifecycle,
                ))
                .route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    routes::feature_modules::gate,
                )),
        )
        .route(
            "/api/openapi.json",
            axum::routing::get(routes::openapi::spec),
        )
        .merge(whmcs_api::root_router())
        .merge(oidc_provider::root_router())
        .merge(dev_docs::root_router())
        .fallback(static_files::frontend)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            cluster::publish_admin_writes,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::ip_guard,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            trusted_proxy::rewrite,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            routes::general_settings::maintenance_guard,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            error::localize_errors,
        ))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security_headers::apply,
        ))
        .layer(CompressionLayer::new().gzip(true).br(true))
        .layer(PropagateRequestIdLayer::new(x_request_id.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(x_request_id, MakeRequestUuid))
        .with_state(state)
}
