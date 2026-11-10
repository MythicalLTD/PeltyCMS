//! Domain events published by core modules and consumed by plugins and
//! addons. [`REGISTRY`] is the single source of truth: the generated docs
//! (`make docs`) render it, and a test fails when the code emits an event
//! (or a payload key) that is not registered here.
use serde::Serialize;

/// Where an event comes from and what listeners may do with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Published after the fact (`on_event`, plugin webhooks, `during`/
    /// `after`/`finished` addon hooks). Listeners only observe.
    Domain,
    /// Synchronous pre-event: addon `hooks` run in priority order before
    /// the action and may continue, cancel or modify the payload.
    Pre,
}

/// One payload key.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Field {
    pub name: &'static str,
    /// JSON type: `uuid`, `string`, `decimal` (exact, as a string), `int`, `bool`, `array`, `object`.
    pub ty: &'static str,
    /// Present on every emission (otherwise only on some code paths).
    pub always: bool,
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct EventSpec {
    pub name: &'static str,
    pub kind: EventKind,
    pub description: &'static str,
    pub payload: &'static [Field],
    /// Listeners may cancel the action (pre-events only).
    pub cancellable: bool,
    /// Listeners may rewrite the payload (pre-events only; the caller decides which keys it honours).
    pub modifiable: bool,
    /// Payload keys the host actually reads back after the `before` phase,
    /// with the rule it applies (`items[].unit_price` = that key on every
    /// array element). Empty when the event is not modifiable.
    pub modify: &'static [(&'static str, &'static str)],
    /// Version of PeltyCMS that first emitted it.
    pub since: &'static str,
}

impl EventSpec {
    /// Addon hook phases this event is delivered in.
    pub fn phases(&self) -> &'static [&'static str] {
        match self.kind {
            EventKind::Pre => &["before"],
            // Domain events are published after the commit: `after` and
            // `finished` hooks plus `on-event` (see `wasm_addons::on_event`).
            EventKind::Domain => &["after", "finished"],
        }
    }

    /// Category for the docs index: the part before the first dot
    /// (`invoice.paid` → `invoice`; pre-events `login.before` → `login`).
    pub fn category(&self) -> &'static str {
        self.name.split('.').next().unwrap_or(self.name)
    }
}

const fn f(name: &'static str, ty: &'static str, description: &'static str) -> Field {
    Field {
        name,
        ty,
        always: true,
        description,
    }
}
const fn o(name: &'static str, ty: &'static str, description: &'static str) -> Field {
    Field {
        name,
        ty,
        always: false,
        description,
    }
}
const fn domain(
    name: &'static str,
    description: &'static str,
    payload: &'static [Field],
) -> EventSpec {
    EventSpec {
        name,
        kind: EventKind::Domain,
        description,
        payload,
        cancellable: false,
        modifiable: false,
        modify: &[],
        since: "0.1.0",
    }
}
const fn pre(
    name: &'static str,
    description: &'static str,
    payload: &'static [Field],
    modify: &'static [(&'static str, &'static str)],
) -> EventSpec {
    EventSpec {
        name,
        kind: EventKind::Pre,
        description,
        payload,
        cancellable: true,
        modifiable: !modify.is_empty(),
        modify,
        since: "0.1.0",
    }
}

pub const REGISTRY: &[EventSpec] = &[
    // ---- users ----
    domain("user.registered", "A new account was created (password sign-up, SSO or OAuth).", &[f("user_id", "uuid", "New user"), o("via", "string", "SSO/OAuth provider id when not a password sign-up")]),
    domain("user.logged_in", "A user signed in successfully.", &[f("user_id", "uuid", "User")]),
    domain("user.login_alert", "Sign-in from a new IP for an account with login alerts on.", &[f("user_id", "uuid", "User"), f("ip", "string", "Client IP"), f("user_agent", "string", "Browser user agent"), f("location", "string", "Resolved place, or a placeholder when GeoIP has none"), o("country", "string", "ISO country code"), o("city", "string", "City"), o("region", "string", "Region")]),
    domain("customer.closed", "A customer account was closed (GDPR erasure or admin close).", &[f("customer_id", "uuid", "Customer")]),
    // ---- orders ----
    domain("order.created", "A customer placed an order.", &[f("order_id", "uuid", "Order"), f("customer_id", "uuid", "Customer"), f("total", "decimal", "Order total"), f("currency", "string", "ISO 4217 code")]),
    domain("order.approved", "An order was accepted and its services created.", &[f("order_id", "uuid", "Order"), f("services", "array", "Created service ids"), o("auto", "bool", "Approved automatically on payment")]),
    domain("order.cancelled", "An order was cancelled.", &[f("order_id", "uuid", "Order"), o("fraud", "bool", "Cancelled by the fraud check")]),
    domain("order.refunded", "A paid order was refunded with a credit note.", &[f("order_id", "uuid", "Order"), f("credit_note_id", "uuid", "Credit note")]),
    domain("order.reopened", "Staff put a cancelled or fraud order back to pending.", &[f("order_id", "uuid", "Order"), f("by", "uuid", "Staff user")]),
    domain("order.deleted", "Staff deleted an order.", &[f("order_id", "uuid", "Order"), f("by", "uuid", "Staff user")]),
    domain("order.fraud_review", "The fraud check held an order for review.", &[f("order_id", "uuid", "Order"), f("customer_id", "uuid", "Customer"), f("score", "decimal", "Fraud score"), f("check_id", "uuid", "Fraud check")]),
    domain("order.fraud_rejected", "The fraud check, or staff reviewing it, rejected an order (cancelled as fraud).", &[f("order_id", "uuid", "Order"), f("customer_id", "uuid", "Customer"), f("score", "decimal", "Fraud score"), o("check_id", "uuid", "Fraud check (staff decision)"), o("by_staff", "bool", "Rejected from the review queue")]),
    domain("order.fraud_approved", "Staff cleared an order held for fraud review.", &[f("order_id", "uuid", "Order"), f("customer_id", "uuid", "Customer"), f("check_id", "uuid", "Fraud check")]),
    // ---- invoices ----
    domain("invoice.created", "An invoice was issued (order, renewal, late fee, debit note, project or billable items).", &[f("invoice_id", "uuid", "Invoice"), o("customer_id", "uuid", "Customer"), o("service_id", "uuid", "Renewed service"), o("renewal", "bool", "Renewal invoice"), o("on_demand", "bool", "Renewal requested early by the client"), o("late_fee_for", "uuid", "Overdue invoice the fee is for"), o("debit_for", "uuid", "Invoice the debit note corrects"), o("number", "string", "Invoice number"), o("project_id", "uuid", "Project"), o("billable", "bool", "From billable items")]),
    domain("invoice.paid", "An invoice became fully paid (gateway, credit, autopay or admin).", &[f("invoice_id", "uuid", "Invoice"), o("customer_id", "uuid", "Customer (only on checkout auto-paid invoices)")]),
    domain("invoice.voided", "An invoice was voided.", &[f("invoice_id", "uuid", "Invoice")]),
    domain("invoice.cancelled", "An unpaid invoice was cancelled by automation.", &[f("invoice_id", "uuid", "Invoice"), f("reason", "string", "Why, e.g. `overdue_auto_cancel`")]),
    domain("invoice.refunded", "A payment on the invoice was refunded.", &[f("invoice_id", "uuid", "Invoice"), o("credit_note_id", "uuid", "Credit note"), o("refund_id", "uuid", "Refund"), o("amount", "decimal", "Refunded amount")]),
    domain("invoice.merged", "Several unpaid invoices were merged into a new one.", &[f("invoice_id", "uuid", "New invoice"), f("sources", "array", "Merged invoice ids"), f("by_staff", "bool", "Merged by staff")]),
    domain("invoice.split", "Staff split lines off an invoice.", &[f("invoice_id", "uuid", "Original invoice"), f("into", "array", "Resulting invoice ids")]),
    domain("invoice.duplicated", "Staff duplicated an invoice as a draft.", &[f("invoice_id", "uuid", "New draft"), f("from", "uuid", "Source invoice")]),
    domain("invoice.due_soon", "Daily reminder: an invoice is due in a few days.", &[f("invoice_id", "uuid", "Invoice"), f("customer_id", "uuid", "Customer"), f("days_left", "int", "Days until due")]),
    domain("invoice.overdue", "Daily reminder: an invoice is past due.", &[f("invoice_id", "uuid", "Invoice"), f("customer_id", "uuid", "Customer"), f("days_overdue", "int", "Days past due")]),
    // ---- payments ----
    domain("payment.succeeded", "A payment was captured.", &[o("invoice_id", "uuid", "Invoice"), o("payment_id", "uuid", "Payment"), o("gateway", "string", "Gateway id"), o("provider_ref", "string", "Gateway reference"), o("autopay", "bool", "Charged by autopay")]),
    domain("payment.failed", "A payment attempt failed.", &[o("invoice_id", "uuid", "Invoice"), o("payment_id", "uuid", "Payment"), o("gateway", "string", "Gateway id"), o("provider_ref", "string", "Gateway reference"), o("autopay", "bool", "Autopay attempt"), o("attempt", "int", "Autopay attempt number")]),
    domain("payment.disputed", "A gateway reported a chargeback or dispute.", &[f("gateway", "string", "Gateway id"), f("provider_ref", "string", "Gateway reference"), o("payment_id", "uuid", "Payment"), o("event_type", "string", "Gateway event type")]),
    domain("payment_method.expiring", "A saved card expires soon.", &[f("payment_method_id", "uuid", "Payment method"), f("customer_id", "uuid", "Customer")]),
    // ---- digital products ----
    domain("license.issued", "A software license key was issued (purchase or staff).", &[f("license_id", "uuid", "License"), f("product_id", "uuid", "Product"), f("customer_id", "uuid", "Customer"), o("service_id", "uuid", "Service it belongs to")]),
    domain("license.revoked", "A software license was revoked (staff, termination, refund or chargeback).", &[o("license_id", "uuid", "License"), o("service_id", "uuid", "Service"), o("reason", "string", "Event that caused it")]),
    domain("digital.version_released", "A new version of a digital product was uploaded.", &[f("product_id", "uuid", "Product"), f("version_id", "uuid", "Version"), f("version", "string", "Version label")]),
    domain("digital.asset_granted", "A purchase needs an external asset granted (e.g. FiveM escrow resource).", &[f("grant_id", "uuid", "Grant"), f("asset", "string", "External asset id"), f("identity", "string", "Customer identity for the asset (FiveM license), may be empty"), f("customer_id", "uuid", "Customer"), f("service_id", "uuid", "Service")]),
    domain("digital.asset_revoked", "An external asset grant was revoked (termination, refund or chargeback).", &[f("grant_id", "uuid", "Grant"), f("asset", "string", "External asset id"), f("identity", "string", "Customer identity"), f("customer_id", "uuid", "Customer"), f("service_id", "uuid", "Service")]),
    // ---- services ----
    domain("service.created", "A service was created from an order.", &[f("service_id", "uuid", "Service"), f("order_id", "uuid", "Order")]),
    domain("service.provisioned", "A provisioning module action succeeded.", &[f("service_id", "uuid", "Service"), f("action", "string", "`create`, `suspend`, …"), f("module", "string", "Module id")]),
    domain("service.provision_failed", "A provisioning module action failed.", &[f("service_id", "uuid", "Service"), f("action", "string", "Module action"), f("error", "string", "Error message")]),
    domain("service.suspended", "A service was suspended.", &[f("service_id", "uuid", "Service"), o("from", "string", "Previous status"), o("reason", "string", "Why")]),
    domain("service.unsuspended", "A service was reactivated.", &[f("service_id", "uuid", "Service"), o("from", "string", "Previous status"), o("reason", "string", "Why")]),
    domain("service.terminated", "A service was terminated.", &[f("service_id", "uuid", "Service"), o("from", "string", "Previous status"), o("reason", "string", "Why"), o("provisioned", "bool", "The module terminated it remotely")]),
    domain("service.cancelled", "A cancellation request was applied.", &[f("service_id", "uuid", "Service")]),
    domain("service.activated", "Staff moved a pending (or otherwise inactive) service to active.", &[f("service_id", "uuid", "Service"), o("from", "string", "Previous status"), o("reason", "string", "Why")]),
    domain("service.change_requested", "An upgrade/downgrade was requested (applied at once when free, otherwise invoiced).", &[f("service_id", "uuid", "Service"), f("change_id", "uuid", "Change request"), f("kind", "string", "`upgrade` or `downgrade`"), f("product_id", "uuid", "Target product"), f("status", "string", "`applied` or `pending`"), o("invoice_id", "uuid", "Invoice for the difference")]),
    domain("service.cancellation_requested", "A client asked to cancel a service.", &[f("service_id", "uuid", "Service"), f("request_id", "uuid", "Cancellation request"), f("kind", "string", "`immediate` or `end_of_period`"), o("reason", "string", "Reason given")]),
    domain("service.cancellation_withdrawn", "A client withdrew a pending cancellation request.", &[f("service_id", "uuid", "Service")]),
    domain("service.cancellation_decided", "Staff approved or rejected a cancellation request.", &[f("service_id", "uuid", "Service"), f("request_id", "uuid", "Cancellation request"), f("status", "string", "New request status"), f("approved", "bool", "Approved")]),
    // ---- customers, users, staff ----
    domain("customer.created", "Staff created a customer account.", &[f("customer_id", "uuid", "Customer"), f("email", "string", "Login email"), f("by", "uuid", "Staff user")]),
    domain("customer.updated", "A customer profile changed (client or staff).", &[f("customer_id", "uuid", "Customer"), f("by_staff", "bool", "Changed by staff"), o("status", "string", "Account status after an admin edit")]),
    domain("customer.contact_added", "A client added a sub-account contact with its own login.", &[f("customer_id", "uuid", "Customer"), f("contact_id", "uuid", "Contact"), o("user_id", "uuid", "Contact's user"), f("permissions", "array", "Granted permissions")]),
    domain("customer.contact_invited", "A client invited a contact by email.", &[f("customer_id", "uuid", "Customer"), f("contact_id", "uuid", "Contact"), f("permissions", "array", "Granted permissions")]),
    domain("customer.contact_updated", "A contact's permissions changed.", &[f("customer_id", "uuid", "Customer"), f("contact_id", "uuid", "Contact"), f("permissions", "array", "Granted permissions")]),
    domain("customer.contact_removed", "A contact was removed.", &[f("customer_id", "uuid", "Customer"), f("contact_id", "uuid", "Contact")]),
    domain("user.logged_out", "A user signed out.", &[f("user_id", "uuid", "User")]),
    domain("user.password_reset_requested", "A password reset link was sent.", &[f("user_id", "uuid", "User")]),
    domain("user.password_reset", "A password was reset with a reset link.", &[f("user_id", "uuid", "User"), o("sessions_revoked", "int", "Other sessions signed out")]),
    domain("user.email_verified", "A user verified their email address.", &[f("user_id", "uuid", "User")]),
    domain("user.mfa_enabled", "A user turned on two-factor authentication.", &[f("user_id", "uuid", "User")]),
    domain("user.mfa_disabled", "A user turned off two-factor authentication.", &[f("user_id", "uuid", "User")]),
    domain("user.roles_changed", "Staff roles of a user changed.", &[f("user_id", "uuid", "User"), f("roles", "array", "New role names"), f("by", "uuid", "Staff user")]),
    domain("role.created", "A staff role was created.", &[f("role_id", "uuid", "Role"), f("name", "string", "Role name"), f("permissions", "array", "Permissions")]),
    domain("role.updated", "A staff role changed.", &[f("role_id", "uuid", "Role"), f("name", "string", "Role name"), f("permissions", "array", "Permissions")]),
    domain("role.deleted", "A staff role was deleted.", &[f("role_id", "uuid", "Role")]),
    domain("api_key.created", "A user created an API key.", &[f("api_key_id", "uuid", "Key"), f("user_id", "uuid", "Owner"), f("scopes", "array", "Scopes")]),
    domain("api_key.revoked", "An API key was revoked.", &[f("api_key_id", "uuid", "Key"), f("user_id", "uuid", "Owner"), f("by_staff", "bool", "Revoked by staff")]),
    domain("webhook_endpoint.disabled", "An outgoing webhook endpoint was disabled automatically after too many failed deliveries (never sent to webhook endpoints themselves).", &[f("endpoint_id", "uuid", "Endpoint"), f("reason", "string", "Why")]),
    domain("kyc.document_uploaded", "A client uploaded an identity document.", &[f("customer_id", "uuid", "Customer"), f("document_id", "uuid", "Document"), f("kind", "string", "Document kind")]),
    domain("kyc.document_reviewed", "Staff approved or rejected an identity document.", &[f("customer_id", "uuid", "Customer"), f("document_id", "uuid", "Document"), f("approved", "bool", "Approved"), f("status", "string", "Customer's KYC status after review"), f("by", "uuid", "Staff user")]),
    domain("kyc.status_changed", "Staff overrode a customer's KYC status.", &[f("customer_id", "uuid", "Customer"), f("status", "string", "New status"), f("by", "uuid", "Staff user")]),
    domain("gdpr.erasure_requested", "A user asked for their data to be erased.", &[f("request_id", "uuid", "Request"), f("user_id", "uuid", "User")]),
    domain("gdpr.erasure_cancelled", "A user withdrew their erasure request.", &[f("user_id", "uuid", "User")]),
    domain("gdpr.erasure_rejected", "Staff rejected an erasure request.", &[f("request_id", "uuid", "Request"), f("by", "uuid", "Staff user")]),
    domain("gdpr.erased", "A user's personal data was erased.", &[f("request_id", "uuid", "Request"), f("user_id", "uuid", "Erased user"), f("by", "uuid", "Staff user")]),
    domain("gdpr.exported", "A data export was downloaded.", &[f("user_id", "uuid", "Exported user"), f("by_staff", "bool", "Downloaded by staff")]),
    // ---- domains ----
    domain("domain.ordered", "A domain registration or transfer was ordered.", &[f("domain_id", "uuid", "Domain"), f("customer_id", "uuid", "Customer"), f("name", "string", "Domain name"), f("kind", "string", "`register` or `transfer`"), f("invoice_id", "uuid", "Invoice")]),
    domain("domain.nameservers_changed", "A domain's nameservers changed.", &[f("domain_id", "uuid", "Domain"), f("nameservers", "array", "New nameservers")]),
    domain("domain.contact_changed", "A domain's registrant contact changed.", &[f("domain_id", "uuid", "Domain")]),
    domain("domain.auto_renew_changed", "Auto-renew was switched on or off.", &[f("domain_id", "uuid", "Domain"), f("auto_renew", "bool", "New value")]),
    domain("domain.epp_revealed", "A domain's transfer (EPP) code was shown.", &[f("domain_id", "uuid", "Domain"), f("by", "uuid", "User who revealed it")]),
    domain("domain.renewal_requested", "A client requested an early renewal (invoiced).", &[f("domain_id", "uuid", "Domain"), f("invoice_id", "uuid", "Renewal invoice"), f("years", "int", "Years")]),
    domain("domain.updated", "Staff edited a domain's status or expiry.", &[f("domain_id", "uuid", "Domain"), o("status", "string", "New status"), f("by", "uuid", "Staff user")]),
    domain("domain.synced", "Staff synced a domain with its registrar.", &[f("domain_id", "uuid", "Domain")]),
    domain("domain.dns_changed", "A domain's DNS records were replaced.", &[f("domain_id", "uuid", "Domain"), f("records", "int", "Record count"), f("by_staff", "bool", "Changed by staff")]),
    // ---- catalog, coupons ----
    domain("catalog.product_created", "A product was created.", &[f("product_id", "uuid", "Product"), f("slug", "string", "Slug")]),
    domain("catalog.product_updated", "A product was edited.", &[f("product_id", "uuid", "Product")]),
    domain("catalog.prices_changed", "A product's price table was replaced.", &[f("product_id", "uuid", "Product"), f("count", "int", "Price rows")]),
    domain("coupon.created", "A coupon was created.", &[f("coupon_id", "uuid", "Coupon"), f("code", "string", "Code")]),
    domain("coupon.updated", "A coupon was edited.", &[f("coupon_id", "uuid", "Coupon"), f("code", "string", "Code")]),
    domain("coupon.deleted", "A coupon was deleted.", &[f("coupon_id", "uuid", "Coupon")]),
    // ---- credit ----
    domain("credit.adjusted", "Staff added or removed account credit.", &[f("customer_id", "uuid", "Customer"), f("amount", "decimal", "Signed amount"), f("currency", "string", "ISO 4217 code"), f("balance", "decimal", "New balance"), f("by", "uuid", "Staff user")]),
    domain("credit.applied", "Account credit paid (part of) an invoice.", &[f("customer_id", "uuid", "Customer"), f("invoice_id", "uuid", "Invoice"), f("amount", "decimal", "Amount applied"), o("payment_id", "uuid", "Payment"), f("auto", "bool", "Applied automatically on issue")]),
    domain("gift_card.redeemed", "A client redeemed a gift card into credit.", &[f("customer_id", "uuid", "Customer"), f("currency", "string", "ISO 4217 code"), f("amount", "decimal", "Amount credited")]),
    // ---- email, automation, addons ----
    domain("email.sent", "A transactional email was delivered to the mailer.", &[f("template", "string", "Template name"), f("to", "string", "Recipient"), f("subject", "string", "Final subject"), f("mailer", "string", "Mailer used")]),
    domain("email.failed", "A transactional email failed (the job retries with backoff).", &[f("template", "string", "Template name"), f("to", "string", "Recipient"), f("subject", "string", "Final subject"), o("error", "string", "Mailer error")]),
    domain("automation.task_completed", "A scheduled/system job finished (recurring billing, overdue run, autopay, domain sync, housekeeping, …).", &[f("kind", "string", "Job kind, e.g. `billing.recurring`"), f("job_id", "int", "Job id"), f("result", "object", "The job's result summary")]),
    domain("automation.task_failed", "A scheduled/system job failed.", &[f("kind", "string", "Job kind"), f("job_id", "int", "Job id"), f("status", "string", "`queued` (will retry) or `dead`"), f("error", "string", "Error")]),
    domain("automation.task_tripped", "A scheduled task (built-in or `<addon>:<job>`) was disabled after `PELTY_CRON_MAX_FAILURES` failed runs in a row; reset it in Admin → Jobs or with `cron reset`.", &[f("kind", "string", "Task name"), f("failures", "int", "Failed runs in a row"), f("error", "string", "Last error")]),
    domain("addon.installed", "An addon was installed.", &[f("addon_id", "string", "Addon id"), f("version", "string", "Version"), f("kind", "string", "Addon kind")]),
    domain("addon.enabled", "An addon was enabled.", &[f("addon_id", "string", "Addon id")]),
    domain("addon.disabled", "An addon was disabled.", &[f("addon_id", "string", "Addon id")]),
    domain("addon.uninstalled", "An addon was uninstalled.", &[f("addon_id", "string", "Addon id")]),
    domain("addon.settings_updated", "An addon's settings were saved.", &[f("addon_id", "string", "Addon id")]),
    // ---- support, sales ----
    domain("ticket.opened", "A support ticket was opened (by the client, staff or the email pipe).", &[f("ticket_id", "uuid", "Ticket"), o("number", "string", "Ticket number"), o("customer_id", "uuid", "Customer"), o("department_id", "uuid", "Department"), o("priority", "string", "`low`, `medium`, `high` or `urgent`"), o("by_staff", "bool", "Opened by staff on the customer's behalf")]),
    domain("ticket.replied", "A reply was posted on a ticket.", &[f("ticket_id", "uuid", "Ticket"), f("by_staff", "bool", "Reply came from staff")]),
    domain("ticket.closed", "A ticket was closed.", &[f("ticket_id", "uuid", "Ticket"), o("reason", "string", "Why: `inactive`, `client` or `staff`"), o("by_staff", "bool", "Closed by staff")]),
    domain("ticket.updated", "Staff changed a ticket's status, priority, department or assignee.", &[f("ticket_id", "uuid", "Ticket"), o("status", "string", "New status"), o("priority", "string", "New priority"), o("department_id", "uuid", "New department"), o("assigned_to", "uuid", "New assignee"), f("by", "uuid", "Staff user")]),
    domain("ticket.sla_warning", "A ticket passed its SLA warning threshold (first response or resolution).", &[f("ticket_id", "uuid", "Ticket"), f("number", "int", "Ticket number"), f("subject", "string", "Subject"), f("target", "string", "first_response or resolution"), f("due_at", "int", "Due (Unix seconds)"), o("assigned_to", "uuid", "Assignee")]),
    domain("ticket.sla_breached", "A ticket missed an SLA target; the policy's escalation was applied.", &[f("ticket_id", "uuid", "Ticket"), f("number", "int", "Ticket number"), f("subject", "string", "Subject"), f("target", "string", "first_response or resolution"), f("due_at", "int", "Due (Unix seconds)"), o("assigned_to", "uuid", "Assignee before escalation"), f("escalated", "bool", "Priority, assignee or department changed")]),
    domain("ticket.assigned", "A ticket was assigned to a staff member.", &[f("ticket_id", "uuid", "Ticket"), f("assigned_to", "uuid", "Assignee"), f("by", "uuid", "Staff user")]),
    domain("ticket.note_added", "Staff added an internal note to a ticket.", &[f("ticket_id", "uuid", "Ticket"), f("by", "uuid", "Staff user")]),
    domain("chat.converted", "A chat transcript was preserved as a ticket.", &[f("chat_id", "uuid", "Chat"), f("ticket_id", "uuid", "Ticket"), f("by", "uuid", "Staff user")]),
    domain("chat.started", "A visitor started a live chat.", &[f("chat_id", "uuid", "Chat"), o("name", "string", "Visitor name"), o("email", "string", "Visitor email")]),
    domain("quote.sent", "A quote was sent to a customer.", &[f("quote_id", "uuid", "Quote")]),
    domain("quote.created", "Staff created a quote.", &[f("quote_id", "uuid", "Quote"), f("by", "uuid", "Staff user")]),
    domain("quote.updated", "Staff edited a quote.", &[f("quote_id", "uuid", "Quote"), f("by", "uuid", "Staff user")]),
    domain("quote.deleted", "Staff deleted a quote.", &[f("quote_id", "uuid", "Quote"), f("by", "uuid", "Staff user")]),
    domain("quote.accepted", "The customer accepted a quote (an invoice is issued).", &[f("quote_id", "uuid", "Quote"), f("customer_id", "uuid", "Customer"), o("invoice_id", "uuid", "Invoice issued for it")]),
    domain("quote.declined", "The customer declined a quote.", &[f("quote_id", "uuid", "Quote"), f("customer_id", "uuid", "Customer")]),
    domain("affiliate.payout_requested", "An affiliate asked for a payout.", &[f("affiliate_id", "uuid", "Affiliate"), f("payout_id", "uuid", "Payout"), f("amount", "decimal", "Amount"), f("currency", "string", "ISO 4217 code")]),
    // ---- pre-events (addon hooks) ----
    pre("login.before", "Before a password sign-in is checked. Cancel to refuse the login (the client gets 409 `error.addon_hook_cancelled`). The payload is read-only: changes are ignored.", &[f("email", "string", "Submitted email"), o("ip", "string", "Client IP"), o("user_agent", "string", "Browser user agent")], &[]),
    pre("client.before_register", "Before a sign-up. Cancel to refuse it; `display_name` may be rewritten. The password never leaves the host.", &[f("email", "string", "Submitted email"), f("display_name", "string", "Display name (modifiable)"), o("ip", "string", "Client IP")], &[("display_name", "String of at most 200 bytes; anything else keeps the submitted name")]),
    pre("order.before_place", "Before an order is placed. Cancel to refuse; `coupon` may be set or cleared.", &[f("customer_id", "uuid", "Customer"), f("currency", "string", "ISO 4217 code"), f("items", "array", "Cart lines"), o("coupon", "string", "Coupon code (modifiable)")], &[("coupon", "Non-empty string of at most 64 bytes sets the code, `null` removes it, any other value keeps the submitted code. The code is then validated like a typed one")]),
    pre("invoice.before_create", "Before an admin creates an invoice. Line descriptions may be rewritten; amounts only with `money.modify`.", &[f("customer_id", "uuid", "Customer"), f("currency", "string", "ISO 4217 code"), f("finalize", "bool", "Issue immediately"), f("items", "array", "`{description, quantity, unit_price}` lines (decimals as strings)")], &[("items[].description", "Non-blank string of at most 500 bytes, matched by position; otherwise the original line text"), ("items[].unit_price", "Needs `capabilities.money.modify`; an exact decimal string (at most 4 places), audited as `addon.money_modified`. Without the capability the change is reverted")]),
    pre("customer.before_create", "Before staff create a customer. Cancel to refuse; read-only.", &[f("email", "string", "Login email"), f("by_staff", "bool", "Always `true`")], &[]),
    pre("customer.before_update", "Before a profile is saved (client or staff). Cancel to refuse; any profile field may be rewritten, the result must still deserialize and validate as a profile, otherwise the submitted profile is kept.", &[f("customer_id", "uuid", "Customer"), f("by_staff", "bool", "Changed by staff"), o("organization", "string", "Organization"), o("first_name", "string", "First name"), o("last_name", "string", "Last name"), o("phone", "string", "Phone"), o("address_line1", "string", "Address line 1"), o("address_line2", "string", "Address line 2"), o("city", "string", "City"), o("region", "string", "State/region"), o("postal_code", "string", "Postcode"), o("country", "string", "ISO country code"), o("tax_id", "string", "Tax id"), o("vat_number", "string", "VAT number"), o("currency", "string", "Preferred currency")], &[("organization", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("first_name", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("last_name", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("phone", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("address_line1", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("address_line2", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("city", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("region", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("postal_code", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("country", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("tax_id", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("vat_number", "Profile field; the rewritten profile is validated as a whole and dropped when invalid"), ("currency", "Profile field; the rewritten profile is validated as a whole and dropped when invalid")]),
    pre("user.before_password_reset", "Before a reset link is sent. Cancel to suppress it silently (the client still gets 204, no account probing). Read-only.", &[f("user_id", "uuid", "User"), f("email", "string", "Account email"), o("ip", "string", "Client IP")], &[]),
    pre("ticket.before_open", "Before a ticket is opened (client or staff). Cancel to refuse; subject, body, priority and department may be rewritten (e.g. auto-routing or triage).", &[f("customer_id", "uuid", "Customer"), f("subject", "string", "Subject (modifiable)"), f("body", "string", "First message (modifiable)"), f("priority", "string", "Priority (modifiable)"), f("department_id", "uuid", "Department (modifiable)"), o("service_id", "uuid", "Related service"), f("by_staff", "bool", "Opened by staff")], &[("subject", "Non-blank string of at most 200 bytes"), ("body", "Non-blank string of at most 20000 bytes"), ("priority", "One of `low`, `medium`, `high`, `urgent`"), ("department_id", "A UUID of an existing department (validated like a submitted one)")]),
    pre("ticket.before_reply", "Before a reply or internal note is posted. Cancel to refuse; the message may be rewritten (e.g. redaction or signatures).", &[f("ticket_id", "uuid", "Ticket"), f("body", "string", "Message (modifiable)"), f("by_staff", "bool", "From staff"), f("internal", "bool", "Internal note")], &[("body", "Non-blank string of at most 20000 bytes")]),
    pre("service.before_action", "Before staff change a service's status (activate, suspend, unsuspend, terminate, cancel). Cancel to refuse; the reason may be rewritten.", &[f("service_id", "uuid", "Service"), f("action", "string", "Requested action"), f("reason", "string", "Reason (modifiable)"), f("by", "uuid", "Staff user")], &[("reason", "String of at most 500 bytes")]),
    pre("service.before_change", "Before an upgrade/downgrade is requested (client or staff). Cancel to refuse that path; read-only.", &[f("service_id", "uuid", "Service"), f("product_id", "uuid", "Target product"), f("by", "uuid", "Requesting user")], &[]),
    pre("service.before_cancellation_request", "Before a client's cancellation request is recorded. Cancel to refuse (e.g. a retention flow); kind and reason may be rewritten.", &[f("service_id", "uuid", "Service"), f("kind", "string", "`immediate` or `end_of_period` (modifiable)"), f("reason", "string", "Reason (modifiable)"), f("user_id", "uuid", "Requesting user")], &[("kind", "`immediate` or `end_of_period`"), ("reason", "String of at most 2000 bytes")]),
    pre("billing.before_overdue_run", "Before the daily overdue automation (reminders, late fees, suspensions, terminations, auto-cancel). Cancel to postpone the whole run to the next day; read-only.", &[f("date", "string", "Run date (YYYY-MM-DD)")], &[]),
    pre("order.before_approve", "Before staff approve an order (services are created). Cancel to refuse; read-only.", &[f("order_id", "uuid", "Order"), f("by", "uuid", "Staff user")], &[]),
    pre("order.before_cancel", "Before staff cancel an order. Cancel to refuse; read-only.", &[f("order_id", "uuid", "Order"), f("fraud", "bool", "Marked as fraud"), f("by", "uuid", "Staff user")], &[]),
    pre("order.before_delete", "Before staff delete an order. Cancel to refuse; read-only.", &[f("order_id", "uuid", "Order"), f("by", "uuid", "Staff user")], &[]),
    pre("quote.before_accept", "Before a client accepts a quote. Cancel to refuse (e.g. pending internal approval); read-only.", &[f("quote_id", "uuid", "Quote"), f("customer_id", "uuid", "Customer")], &[]),
    pre("domain.before_order", "Before a domain registration/transfer is ordered. Cancel to refuse (e.g. a blocklist); read-only.", &[f("customer_id", "uuid", "Customer"), f("name", "string", "Domain name"), f("tld", "string", "TLD"), f("kind", "string", "`register` or `transfer`"), f("years", "int", "Years"), f("privacy", "bool", "ID protection"), f("dns_management", "bool", "DNS management")], &[]),
    pre("domain.before_nameservers", "Before a client changes nameservers. Cancel to refuse; the list may be rewritten.", &[f("domain_id", "uuid", "Domain"), f("name", "string", "Domain name"), f("nameservers", "array", "Nameservers (modifiable)"), f("by_staff", "bool", "Changed by staff")], &[("nameservers", "Array of hostnames, re-validated like a submitted list; an invalid list fails the request with 422")]),
    pre("credit.before_adjust", "Before staff add or remove credit. Cancel to refuse; the note may be rewritten. The amount is read-only.", &[f("customer_id", "uuid", "Customer"), f("amount", "decimal", "Signed amount"), f("currency", "string", "ISO 4217 code"), f("note", "string", "Note (modifiable)"), f("by", "uuid", "Staff user")], &[("note", "String of at most 500 bytes")]),
    pre("gdpr.before_erase", "Before staff erase a user's data. Cancel to refuse (e.g. a legal hold); read-only.", &[f("request_id", "uuid", "Erasure request"), f("user_id", "uuid", "User"), f("by", "uuid", "Staff user")], &[]),
    pre("email.before_send", "In the mail worker, before a transactional email is delivered. Cancel to drop it; subject and text may be rewritten (HTML is regenerated from the text). Security mails (password reset, verification, invites, …) never reach this hook.", &[f("template", "string", "Template name"), f("locale", "string", "Locale"), f("to", "string", "Recipient"), f("subject", "string", "Rendered subject (modifiable)"), f("text", "string", "Rendered text body (modifiable)")], &[("subject", "Non-blank string of at most 500 bytes (newlines become spaces)"), ("text", "Non-blank string of at most 200000 bytes")]),
];

/// Registry entry for `name`.
pub fn spec(name: &str) -> Option<&'static EventSpec> {
    REGISTRY.iter().find(|e| e.name == name)
}

/// Is `name` a registered domain event (plugins and addons may subscribe)?
pub fn is_domain_event(name: &str) -> bool {
    spec(name).is_some_and(|e| e.kind == EventKind::Domain)
}

/// Is `name` a registered synchronous pre-event?
pub fn is_pre_event(name: &str) -> bool {
    spec(name).is_some_and(|e| e.kind == EventKind::Pre)
}

#[derive(Debug, Clone, Serialize)]
pub struct DomainEvent {
    pub name: &'static str,
    pub payload: serde_json::Value,
}

impl DomainEvent {
    pub fn new(name: &'static str, payload: serde_json::Value) -> Self {
        DomainEvent { name, payload }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_is_consistent() {
        let mut names: Vec<_> = REGISTRY.iter().map(|e| e.name).collect();
        names.sort();
        let n = names.len();
        names.dedup();
        assert_eq!(n, names.len(), "duplicate event names");
        for e in REGISTRY {
            assert!(
                !e.description.is_empty() && !e.payload.is_empty(),
                "{}",
                e.name
            );
            assert_eq!(e.cancellable, e.kind == EventKind::Pre, "{}", e.name);
            assert_eq!(e.modifiable, !e.modify.is_empty(), "{}", e.name);
            for (k, rule) in e.modify {
                let top = k.split(['[', '.']).next().unwrap();
                assert!(
                    e.payload.iter().any(|p| p.name == top) && !rule.is_empty(),
                    "{}: modify key {k} not in payload",
                    e.name
                );
            }
            assert!(
                e.name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'.' || b == b'_'),
                "{}",
                e.name
            );
        }
    }
}
