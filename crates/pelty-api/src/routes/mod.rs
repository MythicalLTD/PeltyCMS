//! HTTP API router. Route modules are grouped by domain (finance, commerce, hosting, registry, clients,
//! identity, helpdesk, messaging, extensibility, config, ops) and re-exported here so
//! `crate::routes::<module>` keeps working. `api_router` merges every domain router.
//! Covered by the integration suite and the pelty-docs route inventory tests.
pub mod finance;
pub use finance::billable;
pub use finance::billing;
pub use finance::coupons;
pub use finance::credit;
pub use finance::currencies;
pub use finance::einvoice;
pub use finance::gateways;
pub use finance::invoice_ops;
pub use finance::invoice_pdf;
pub use finance::legal_entities;
pub use finance::overdue;
pub use finance::payment_methods;
pub use finance::quote_pdf;
pub use finance::quotes;
pub use finance::refunds;
pub use finance::renewals;
pub use finance::tax;
pub use finance::usage;
pub mod commerce;
pub use commerce::bundles;
pub use commerce::catalog;
pub use commerce::data_feeds;
pub use commerce::order_forms;
pub use commerce::store;
pub mod hosting;
pub use hosting::modules_ext;
pub use hosting::provisioning;
pub use hosting::service_changes;
pub use hosting::ssl;
pub mod registry;
pub use registry::domain_pricing;
pub use registry::domains;
pub mod clients;
pub use clients::account;
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
pub use clients::kyc;
pub mod identity;
pub use identity::admin_security;
pub use identity::api_keys;
pub use identity::auth;
pub use identity::mfa;
pub use identity::passkeys;
pub use identity::security_questions;
pub use identity::sso;
pub use identity::staff;
pub mod helpdesk;
pub use helpdesk::content;
pub use helpdesk::downloads;
pub use helpdesk::live_chat;
pub use helpdesk::network_status;
pub use helpdesk::projects;
pub use helpdesk::support;
pub use helpdesk::support_ext;
pub use helpdesk::ticket_ai;
pub use helpdesk::ticket_attachments;
pub use helpdesk::ticket_macros;
pub use helpdesk::ticket_sla;
pub use helpdesk::ticket_statuses;
pub use helpdesk::tickets;
pub mod messaging;
pub use messaging::campaigns;
pub use messaging::email_log;
pub use messaging::email_templates;
pub use messaging::link_tracking;
pub use messaging::smtp;
pub use messaging::webhook_endpoints;
pub use messaging::webhooks;
pub mod extensibility;
pub use extensibility::extensions;
pub use extensibility::headless;
pub use extensibility::plugins;
pub use extensibility::vendor;
pub mod config;
pub use config::automation;
pub use config::general_settings;
pub use config::geo;
pub use config::i18n;
pub use config::license;
pub use config::media;
pub use config::settings;
pub use config::setup_wizard;
pub use config::site;
pub mod ops;
pub use ops::admin;
pub use ops::admin_ops;
pub use ops::admin_tools;
pub use ops::bulk;
pub use ops::csv_import;
pub use ops::exports;
pub use ops::mass;
pub use ops::reports;
pub mod gaming;
pub use gaming::community;
pub use gaming::delivery as game_delivery;
pub use gaming::digital;
pub use gaming::games;
pub use gaming::marketplace;
pub use gaming::modules as feature_modules;
pub use gaming::reviews;
pub use gaming::storefront as game_storefront;
pub mod health;
pub mod openapi;

use crate::AppState;
use axum::Router;
use serde::Deserialize;
use uuid::Uuid;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .merge(auth::router())
        .merge(feature_modules::router())
        .merge(games::router())
        .merge(game_delivery::router())
        .merge(digital::router())
        .merge(marketplace::router())
        .merge(reviews::router())
        .merge(community::router())
        .merge(game_storefront::router())
        .merge(csv_import::router())
        .merge(live_chat::router())
        .merge(crate::addons::router())
        .merge(crate::updates::router())
        .merge(crate::translations::router())
        .merge(crate::duo::router())
        .merge(crate::addon_ext::router())
        .merge(crate::captcha::router())
        .merge(crate::oauth::router())
        .merge(crate::saml::router())
        .merge(crate::oidc_provider::router())
        .merge(crate::security::router())
        .merge(crate::whmcs_api::router())
        .merge(site::router())
        .merge(support::router())
        .merge(domains::router())
        .merge(domain_pricing::router())
        .merge(kyc::router())
        .merge(usage::router())
        .merge(bundles::router())
        .merge(ticket_statuses::router())
        .merge(email_log::router())
        .merge(banned_emails::router())
        .merge(sso::router())
        .merge(automation::router())
        .merge(support_ext::router())
        .merge(mfa::router())
        .merge(admin_security::router())
        .merge(avatars::router())
        .merge(account::router())
        .merge(admin::router())
        .merge(customers::router())
        .merge(customer_workspace::router())
        .merge(catalog::router())
        .merge(coupons::router())
        .merge(provisioning::router())
        .merge(modules_ext::router())
        .merge(service_changes::router())
        .merge(affiliates::router())
        .merge(quotes::router())
        .merge(downloads::router())
        .merge(billing::router())
        .merge(store::router())
        .merge(order_forms::router())
        .merge(i18n::router())
        .merge(geo::router())
        .merge(extensions::router())
        .merge(webhooks::router())
        .merge(plugins::router())
        .merge(webhook_endpoints::router())
        .merge(settings::router())
        .merge(smtp::router())
        .merge(tax::router())
        .merge(overdue::router())
        .merge(payment_methods::router())
        .merge(exports::router())
        .merge(legal_entities::router())
        .merge(credit::router())
        .merge(invoice_pdf::router())
        .merge(quote_pdf::router())
        .merge(refunds::router())
        .merge(tickets::router())
        .merge(ticket_attachments::router())
        .merge(email_templates::router())
        .merge(content::router())
        .merge(network_status::router())
        .merge(admin_tools::router())
        .merge(security_questions::router())
        .merge(link_tracking::router())
        .merge(data_feeds::router())
        .merge(renewals::router())
        .merge(vendor::router())
        .merge(passkeys::router())
        .merge(projects::router())
        .merge(setup_wizard::router())
        .merge(license::router())
        .merge(admin_ops::router())
        .merge(general_settings::router())
        .merge(media::router())
        .merge(client_extras::router())
        .merge(staff::router())
        .merge(campaigns::router())
        .merge(bulk::router())
        .merge(mass::router())
        .merge(affiliate_extras::router())
        .merge(billable::router())
        .merge(currencies::router())
        .merge(invoice_ops::router())
        .merge(api_keys::router())
        .merge(reports::router())
        .merge(gdpr::router())
        .merge(ssl::router())
        .merge(fraud::router())
        .merge(ticket_macros::router())
        .merge(ticket_sla::router())
        .merge(ticket_ai::router())
        .merge(gateways::router())
        .merge(client_groups::router())
        .merge(client_group_rules::router())
        .merge(einvoice::router())
        .merge(headless::router())
        .merge(openapi::router())
}

#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct Page {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
}

impl Page {
    /// (limit, offset, page, per_page) with sane bounds.
    pub fn bounds(&self) -> (i64, i64, i64, i64) {
        let per = self.per_page.unwrap_or(25).clamp(1, 100);
        let page = self.page.unwrap_or(1).max(1);
        (per, (page - 1) * per, page, per)
    }
}

/// Standard paginated envelope.
pub fn paged<T: serde::Serialize>(items: T, page: i64, per: i64, total: i64) -> serde_json::Value {
    serde_json::json!({ "items": items, "page": page, "per_page": per, "total": total })
}

/// Query for list endpoints with optional search / status filters.
#[derive(Debug, Deserialize, utoipa::IntoParams)]
pub struct ListQuery {
    pub page: Option<i64>,
    pub per_page: Option<i64>,
    pub q: Option<String>,
    pub status: Option<String>,
    /// Scope a list to one customer (services, …).
    pub customer_id: Option<Uuid>,
}

impl ListQuery {
    pub fn bounds(&self) -> (i64, i64, i64, i64) {
        Page {
            page: self.page,
            per_page: self.per_page,
        }
        .bounds()
    }
    pub fn q(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }
    pub fn status(&self) -> Option<&str> {
        self.status.as_deref().filter(|s| !s.is_empty())
    }
}
