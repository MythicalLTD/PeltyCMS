//! PostgreSQL pool, migrations and repositories. All SQL lives here.
pub mod billing;
pub use billing::add_funds;
pub use billing::autopay;
pub use billing::billable;
pub use billing::card_expiry;
pub use billing::coupons;
pub use billing::credit;
pub use billing::currencies;
pub use billing::exchange_rates;
pub use billing::gateway_admin;
pub use billing::gateway_rules;
pub use billing::gift_cards;
pub use billing::invoice_ops;
pub use billing::invoice_tools;
pub use billing::invoices;
pub use billing::legal_entities;
pub use billing::overdue;
pub use billing::quotes;
pub use billing::refunds;
pub use billing::tax;
pub use billing::transactions;
pub use billing::usage;
pub use billing::InvoiceService;
pub mod clients;
pub use clients::affiliate_extras;
pub use clients::affiliates;
pub use clients::avatars;
pub use clients::banned_emails;
pub use clients::client_extras;
pub use clients::client_group_rules;
pub use clients::client_groups;
pub use clients::customer_workspace;
pub use clients::customers;
pub use clients::fraud;
pub use clients::gdpr;
pub use clients::inactive_clients;
pub use clients::kyc;
pub use clients::ClientService;
pub mod auth;
pub use auth::api_keys;
pub use auth::mfa;
pub use auth::oauth_provider;
pub use auth::passkeys;
pub use auth::saml;
pub use auth::security;
pub use auth::security_questions;
pub use auth::sessions;
pub use auth::sso;
pub use auth::staff;
pub use auth::user_tokens;
pub use auth::users;
pub mod messaging;
pub use messaging::campaigns;
pub use messaging::email_log;
pub use messaging::email_templates;
pub use messaging::event_mail;
pub use messaging::link_tracking;
pub use messaging::plugin_webhooks;
pub use messaging::webhook_endpoints;
pub mod ops;
pub use ops::admin_ops;
pub use ops::admin_tools;
pub use ops::audit;
pub use ops::csv_import;
pub use ops::dashboard;
pub use ops::exports;
pub use ops::housekeeping;
pub use ops::job_stats;
pub use ops::jobs;
pub use ops::mass;
pub use ops::report_library;
pub use ops::reports;
pub mod commerce;
pub use commerce::bundles;
pub use commerce::catalog;
pub use commerce::order_approval;
pub use commerce::order_forms;
pub use commerce::orders;
pub use commerce::storefront;
pub use commerce::OrderService;
pub mod hosting;
pub use hosting::module_ext;
pub use hosting::provisioning;
pub use hosting::server_import;
pub use hosting::service_changes;
pub use hosting::services;
pub use hosting::ssl_checks;
pub use hosting::HostingService;
pub mod registry;
pub use registry::domain_defaults;
pub use registry::domain_pricing;
pub use registry::domains;
pub use registry::registrant_verify;
pub use registry::DomainService;
pub mod gaming;
pub use gaming::community;
pub use gaming::creator_codes;
pub use gaming::discord_roles;
pub use gaming::delivery as game_delivery;
pub use gaming::digital;
pub use gaming::games;
pub use gaming::marketplace;
pub use gaming::modules as feature_modules;
pub use gaming::reviews;
pub use gaming::sales as timed_sales;
pub use gaming::storefront as game_storefront;
pub mod helpdesk;
pub use helpdesk::downloads;
pub use helpdesk::live_chat;
#[allow(unused_imports)]
pub(crate) use helpdesk::live_chat_ext;
pub use helpdesk::network_status;
pub use helpdesk::noticeboard;
pub use helpdesk::projects;
pub use helpdesk::satisfaction;
pub use helpdesk::support;
pub use helpdesk::support_ext;
pub use helpdesk::ticket_attachments;
pub use helpdesk::ticket_macros;
pub use helpdesk::ticket_sla;
pub use helpdesk::ticket_statuses;
pub use helpdesk::tickets;
pub use helpdesk::TicketService;
pub mod config;
pub use config::automation_settings;
pub use config::branding;
pub use config::content;
pub use config::general_settings;
pub use config::languages;
pub use config::license;
pub use config::settings;
pub use config::setup_wizard;
pub use config::site;
pub use config::theme_customization;
pub use config::translations;
pub mod extensibility;
pub use extensibility::addon_kv;
pub use extensibility::addon_registry;
pub use extensibility::addons;
pub use extensibility::extensions;
pub use extensibility::vendor;

use pelty_core::AppError;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use std::str::FromStr;
use std::time::Duration;

pub use sqlx::PgPool as Pool;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Connect, creating the database if it does not exist (dev convenience).
pub async fn connect(url: &str, max: u32) -> Result<PgPool, sqlx::Error> {
    use sqlx::migrate::MigrateDatabase;
    if !sqlx::Postgres::database_exists(url).await.unwrap_or(true) {
        // Ignore races where another process created it concurrently.
        if let Err(e) = sqlx::Postgres::create_database(url).await {
            if !sqlx::Postgres::database_exists(url).await.unwrap_or(false) {
                return Err(e);
            }
        }
    }
    // Prepared statements are cached per connection; every query in this
    // crate uses static SQL so a modest cache covers all of them.
    let opts = PgConnectOptions::from_str(url)?
        .log_statements(tracing::log::LevelFilter::Trace)
        .statement_cache_capacity(256);
    PgPoolOptions::new()
        .max_connections(max)
        .min_connections(max.min(2))
        .acquire_timeout(Duration::from_secs(5))
        .idle_timeout(Some(Duration::from_secs(600)))
        .max_lifetime(Some(Duration::from_secs(1800)))
        // Skip the per-acquire ping round trip; broken connections surface as
        // query errors and are dropped by the pool.
        .test_before_acquire(false)
        .connect_with(opts)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    MIGRATOR.run(pool).await
}

pub async fn ping(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(pool)
        .await
        .is_ok()
}

/// Map DB errors to typed app errors without leaking internals.
pub fn db_err(e: sqlx::Error) -> AppError {
    if let sqlx::Error::Database(d) = &e {
        if d.is_unique_violation() {
            return AppError::Conflict("error.conflict");
        }
        match d.code().as_deref() {
            // CHECK constraint: input passed app validation but not the schema.
            Some("23514") => return AppError::Validation("error.validation"),
            // FK: referenced row (product, customer, currency…) does not exist.
            Some("23503") => return AppError::Validation("error.invalid_reference"),
            // RAISE EXCEPTION from a guard trigger (immutable invoice, lifecycle).
            Some("P0001") => return AppError::Conflict("error.immutable"),
            _ => {}
        }
    }
    tracing::error!(error = %e, "database error");
    AppError::Internal("database".into())
}

/// Execute raw SQL (tests/maintenance only; never with user input).
pub async fn raw_execute(pool: &PgPool, sql: &str) -> Result<u64, sqlx::Error> {
    sqlx::query(sqlx::AssertSqlSafe(sql))
        .execute(pool)
        .await
        .map(|r| r.rows_affected())
}
