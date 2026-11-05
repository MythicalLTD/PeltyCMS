//! WHMCS-compatible API (`POST /includes/api.php`, also
//! `/api/v1/compat/whmcs`), see `make docs`.
//!
//! An installable addon (`integration-whmcs-api`). Credentials are regular
//! PeltyCMS API keys: `identifier` = the key prefix (`pelty_xxxxxxxxxx`),
//! `secret` = the full key. Each WHMCS action is translated to the native
//! REST endpoint and executed through the real router with that key, so the
//! key's scopes, IP allow-list, audit and all business rules apply unchanged.
use crate::addons::Manifest;
use crate::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use pelty_database::addon_registry as reg;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::net::SocketAddr;
use tower::ServiceExt;

pub const ENGINES: &[&str] = &["whmcs_api"];
pub const ADDON_ID: &str = "integration-whmcs-api";

pub fn builtin_manifests() -> Vec<Manifest> {
    vec![Manifest {
        id: ADDON_ID.into(),
        name: "WHMCS-compatible API".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        kind: "integration".into(),
        engine: "whmcs_api".into(),
        description: "addons.desc.integration.whmcs_api".into(),
        author: "MythicalSystems".into(),
        permissions: vec![],
        settings: vec![],
        migrations: vec![],
        files: BTreeMap::new(),
        ..Default::default()
    }]
}

/// Supported actions (documented in the generated docs (`make docs`)).
pub const ACTIONS: &[&str] = &[
    "GetClients",
    "GetClientsDetails",
    "AddClient",
    "UpdateClient",
    "CloseClient",
    "GetClientsProducts",
    "GetProducts",
    "GetOrders",
    "AddOrder",
    "AcceptOrder",
    "CancelOrder",
    "FraudOrder",
    "GetInvoices",
    "GetInvoice",
    "AddInvoicePayment",
    "UpdateClientProduct",
    "ModuleSuspend",
    "ModuleUnsuspend",
    "ModuleTerminate",
    "ModuleCreate",
    "GetTickets",
    "GetTicket",
    "OpenTicket",
    "AddTicketReply",
    "GetCurrencies",
    "GetPaymentMethods",
    "GetSupportDepartments",
    // Orders
    "DeleteOrder",
    "PendingOrder",
    "GetOrderStatuses",
    "GetPromotions",
    // Billing
    "GetTransactions",
    "UpdateTransaction",
    "GetCredits",
    "AddCredit",
    "ApplyCredit",
    "AddBillableItem",
    "GetQuotes",
    "SendQuote",
    "GenInvoices",
    // Clients
    "GetClientsDomains",
    "GetContacts",
    "GetEmails",
    "AddClientNote",
    "GetClientNotes",
    "GetCancelledPackages",
    // Domains
    "GetTLDPricing",
    // Servers / system
    "GetServers",
    "GetHealthStatus",
    "GetToDoItems",
    "GetEmailTemplates",
    "GetAdminUsers",
    "GetActivityLog",
    "WhmcsDetails",
    // Support
    "UpdateTicket",
    "MergeTicket",
    "DeleteTicketReply",
    "UpdateTicketReply",
    "GetAnnouncements",
    "DeleteAnnouncement",
    // Affiliates
    "GetAffiliates",
    // Project management
    "GetProjects",
    "GetProject",
    "CreateProject",
    "UpdateProject",
    "AddProjectTask",
    "DeleteProjectTask",
    "AddProjectTime",
];

/// WHMCS order statuses (PeltyCMS has these four).
pub const ORDER_STATUSES: &[&str] = &["Pending", "Active", "Cancelled", "Fraud"];

/// Commands answered without calling the native API.
pub fn static_reply(action: &str) -> Option<Value> {
    match action {
        "GetOrderStatuses" => Some(json!({
            "result": "success",
            "totalresults": ORDER_STATUSES.len(),
            "statuses": { "status": ORDER_STATUSES.iter().map(|t| json!({ "title": t })).collect::<Vec<_>>() },
        })),
        "WhmcsDetails" => Some(
            json!({ "result": "success", "whmcs": { "version": "8.0.0-compat", "canonicalversion": "8.0.0", "product": "PeltyCMS", "peltycms": env!("CARGO_PKG_VERSION") } }),
        ),
        _ => None,
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/compat/whmcs", post(handle))
}

/// Root-level drop-in path for existing WHMCS integrations.
pub fn root_router() -> Router<AppState> {
    Router::new().route("/includes/api.php", post(handle))
}

fn err(msg: &str) -> Response {
    (
        StatusCode::OK,
        Json(json!({ "result": "error", "message": msg })),
    )
        .into_response()
}

/// WHMCS billing cycle names → PeltyCMS cycles.
pub fn cycle(c: &str) -> String {
    match c.to_ascii_lowercase().replace([' ', '-'], "").as_str() {
        "onetime" | "free" | "freeaccount" => "one_time",
        "monthly" => "monthly",
        "quarterly" => "quarterly",
        "semiannually" | "semiannual" => "semiannual",
        "annually" | "annual" => "annual",
        "biennially" | "biennial" => "biennial",
        "triennially" | "triennial" => "triennial",
        other => return other.to_string(),
    }
    .into()
}

fn s<'a>(p: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    p.get(k)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

fn page(p: &Map<String, Value>) -> String {
    let limit = s(p, "limitnum")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(25)
        .clamp(1, 250);
    let start = s(p, "limitstart")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    format!("page={}&per_page={limit}", start / limit + 1)
}

fn enc(v: &str) -> String {
    form_urlencoded::byte_serialize(v.as_bytes()).collect()
}

/// Translate an action to (method, path, body). Pure (unit-tested).
pub fn translate(
    action: &str,
    p: &Map<String, Value>,
) -> Result<(Method, String, Option<Value>), String> {
    let need = |k: &str| {
        s(p, k)
            .map(str::to_string)
            .ok_or_else(|| format!("Missing parameter: {k}"))
    };
    let q = |extra: &str| {
        let mut x = page(p);
        if let Some(st) = s(p, "status") {
            x.push_str(&format!("&status={}", enc(&st.to_ascii_lowercase())));
        }
        if let Some(se) = s(p, "search") {
            x.push_str(&format!("&q={}", enc(se)));
        }
        if !extra.is_empty() {
            x.push('&');
            x.push_str(extra);
        }
        x
    };
    let profile = || {
        let mut b = Map::new();
        for (w, n) in [
            ("firstname", "first_name"),
            ("lastname", "last_name"),
            ("companyname", "organization"),
            ("phonenumber", "phone"),
            ("address1", "address_line1"),
            ("address2", "address_line2"),
            ("city", "city"),
            ("state", "region"),
            ("postcode", "postal_code"),
            ("country", "country"),
            ("tax_id", "vat_number"),
            ("currency", "currency"),
        ] {
            if let Some(v) = s(p, w) {
                b.insert(n.into(), json!(v));
            }
        }
        b
    };
    let svc_action = |a: &str| -> Result<(Method, String, Option<Value>), String> {
        let id = s(p, "serviceid")
            .or_else(|| s(p, "accountid"))
            .ok_or("Missing parameter: serviceid")?;
        Ok((
            Method::POST,
            format!("/api/v1/admin/services/{id}/action"),
            Some(json!({ "action": a, "reason": s(p, "suspendreason").unwrap_or("") })),
        ))
    };
    Ok(match action {
        "GetClients" => (
            Method::GET,
            format!("/api/v1/admin/customers?{}", q("")),
            None,
        ),
        "GetClientsDetails" => (
            Method::GET,
            format!("/api/v1/admin/customers/{}", need("clientid")?),
            None,
        ),
        "AddClient" => {
            let mut b = profile();
            b.insert("email".into(), json!(need("email")?));
            (
                Method::POST,
                "/api/v1/admin/customers".into(),
                Some(Value::Object(b)),
            )
        }
        "UpdateClient" => (
            Method::PATCH,
            format!("/api/v1/admin/customers/{}", need("clientid")?),
            Some(Value::Object(profile())),
        ),
        "CloseClient" => (
            Method::POST,
            format!("/api/v1/admin/customers/{}/close", need("clientid")?),
            None,
        ),
        "GetClientsProducts" => {
            let extra = s(p, "clientid")
                .map(|c| format!("customer_id={}", enc(c)))
                .unwrap_or_default();
            (
                Method::GET,
                format!("/api/v1/admin/services?{}", q(&extra)),
                None,
            )
        }
        "GetProducts" => (Method::GET, "/api/v1/admin/catalog/products".into(), None),
        "GetOrders" => (Method::GET, format!("/api/v1/admin/orders?{}", q("")), None),
        "AddOrder" => {
            let pid = need("pid")?;
            let cyc = cycle(s(p, "billingcycle").unwrap_or("monthly"));
            let mut b = json!({ "customer_id": need("clientid")?, "items": [{ "product_id": pid, "cycle": cyc }] });
            if let Some(c) = s(p, "promocode") {
                b["coupon"] = json!(c);
            }
            if s(p, "noinvoice").is_none() && s(p, "autoaccept").is_some() {
                b["approve"] = json!(true);
            }
            (Method::POST, "/api/v1/admin/orders".into(), Some(b))
        }
        "AcceptOrder" => (
            Method::POST,
            format!("/api/v1/admin/orders/{}/approve", need("orderid")?),
            Some(json!({})),
        ),
        "CancelOrder" => (
            Method::POST,
            format!("/api/v1/admin/orders/{}/cancel", need("orderid")?),
            Some(json!({ "fraud": false })),
        ),
        "FraudOrder" => (
            Method::POST,
            format!("/api/v1/admin/orders/{}/cancel", need("orderid")?),
            Some(json!({ "fraud": true })),
        ),
        "GetInvoices" => {
            let extra = s(p, "userid")
                .map(|c| format!("customer_id={}", enc(c)))
                .unwrap_or_default();
            (
                Method::GET,
                format!("/api/v1/admin/invoices?{}", q(&extra)),
                None,
            )
        }
        "GetInvoice" => (
            Method::GET,
            format!("/api/v1/admin/invoices/{}", need("invoiceid")?),
            None,
        ),
        "AddInvoicePayment" => {
            let id = need("invoiceid")?;
            let trans = need("transid")?;
            let mut b = json!({ "idempotency_key": format!("whmcs:{trans}"), "reference": trans, "gateway": s(p, "gateway").unwrap_or("manual") });
            b["amount"] = json!(need("amount")?);
            (
                Method::POST,
                format!("/api/v1/admin/invoices/{id}/payments"),
                Some(b),
            )
        }
        "ModuleSuspend" => svc_action("suspend")?,
        "ModuleUnsuspend" => svc_action("unsuspend")?,
        "ModuleTerminate" => svc_action("terminate")?,
        "ModuleCreate" => svc_action("activate")?,
        "UpdateClientProduct" => match s(p, "status").map(str::to_ascii_lowercase).as_deref() {
            Some("suspended") => svc_action("suspend")?,
            Some("active") => svc_action("unsuspend")?,
            Some("terminated") => svc_action("terminate")?,
            Some("cancelled") => svc_action("cancel")?,
            _ => return Err("Only status changes are supported".into()),
        },
        "GetTickets" => (
            Method::GET,
            format!("/api/v1/admin/tickets?{}", q("")),
            None,
        ),
        "GetTicket" => (
            Method::GET,
            format!("/api/v1/admin/tickets/{}", need("ticketid")?),
            None,
        ),
        "OpenTicket" => {
            let mut b = json!({ "department_id": need("deptid")?, "subject": need("subject")?, "body": need("message")?, "customer_id": need("clientid")? });
            if let Some(pr) = s(p, "priority") {
                b["priority"] = json!(pr.to_ascii_lowercase());
            }
            if let Some(sv) = s(p, "serviceid") {
                b["service_id"] = json!(sv);
            }
            (Method::POST, "/api/v1/admin/tickets".into(), Some(b))
        }
        "AddTicketReply" => (
            Method::POST,
            format!("/api/v1/admin/tickets/{}/reply", need("ticketid")?),
            Some(json!({ "body": need("message")?, "internal": false })),
        ),
        "GetCurrencies" => (Method::GET, "/api/v1/admin/currencies".into(), None),
        "GetPaymentMethods" => (Method::GET, "/api/v1/admin/gateways".into(), None),
        "GetSupportDepartments" => (Method::GET, "/api/v1/admin/ticket-departments".into(), None),
        // Orders
        "DeleteOrder" => (
            Method::DELETE,
            format!("/api/v1/admin/orders/{}", need("orderid")?),
            None,
        ),
        "PendingOrder" => (
            Method::POST,
            format!("/api/v1/admin/orders/{}/pending", need("orderid")?),
            None,
        ),
        // Answered by `static_reply`; the native call only authenticates the
        // key and checks the matching scope.
        "GetOrderStatuses" => (
            Method::GET,
            "/api/v1/admin/orders?page=1&per_page=1".into(),
            None,
        ),
        "WhmcsDetails" => (Method::GET, "/api/v1/admin/currencies".into(), None),
        "GetPromotions" => (Method::GET, "/api/v1/admin/coupons".into(), None),
        // Billing
        "GetTransactions" => {
            let mut extra = s(p, "clientid")
                .map(|c| format!("customer_id={}", enc(c)))
                .unwrap_or_default();
            if let Some(t) = s(p, "transid") {
                extra.push_str(&format!(
                    "{}q={}",
                    if extra.is_empty() { "" } else { "&" },
                    enc(t)
                ));
            }
            (
                Method::GET,
                format!("/api/v1/admin/transactions?{}", q(&extra)),
                None,
            )
        }
        "UpdateTransaction" => (
            Method::PATCH,
            format!("/api/v1/admin/transactions/{}", need("transactionid")?),
            Some(json!({ "note": s(p, "description").unwrap_or("") })),
        ),
        "GetCredits" => (
            Method::GET,
            format!("/api/v1/admin/customers/{}/credit", need("clientid")?),
            None,
        ),
        "AddCredit" => {
            let amt = need("amount")?;
            let amt = if s(p, "type").is_some_and(|t| t.eq_ignore_ascii_case("remove")) {
                format!("-{}", amt.trim_start_matches('-'))
            } else {
                amt
            };
            (
                Method::POST,
                format!("/api/v1/admin/customers/{}/credit", need("clientid")?),
                Some(
                    json!({ "currency": need("currency")?.to_ascii_uppercase(), "amount": amt, "note": s(p, "description").unwrap_or("") }),
                ),
            )
        }
        "ApplyCredit" => {
            let b = match s(p, "amount") {
                Some(a) if !a.eq_ignore_ascii_case("full") => json!({ "amount": a }),
                _ => json!({}),
            };
            (
                Method::POST,
                format!("/api/v1/admin/invoices/{}/apply-credit", need("invoiceid")?),
                Some(b),
            )
        }
        "AddBillableItem" => {
            let action = match s(p, "invoiceaction")
                .unwrap_or("noinvoice")
                .to_ascii_lowercase()
                .as_str()
            {
                "nextcron" | "nextinvoice" => "next_invoice",
                "duedate" => "invoice_now",
                "recur" => "recurring",
                _ => "hold",
            };
            let mut b = json!({ "customer_id": need("clientid")?, "description": need("description")?, "unit_price": need("amount")?, "invoice_action": action });
            if let Some(h) = s(p, "hours").or(s(p, "quantity")) {
                b["quantity"] = json!(h);
            }
            if let Some(c) = s(p, "recurcycle") {
                b["recur_cycle"] = json!(cycle(c));
            }
            (Method::POST, "/api/v1/admin/billable-items".into(), Some(b))
        }
        "GetQuotes" => (Method::GET, format!("/api/v1/admin/quotes?{}", q("")), None),
        "SendQuote" => (
            Method::POST,
            format!("/api/v1/admin/quotes/{}/send", need("quoteid")?),
            Some(json!({})),
        ),
        "GenInvoices" => (
            Method::POST,
            format!("/api/v1/admin/automation/{}/run", crate::worker::RECURRING),
            None,
        ),
        // Clients
        "GetClientsDomains" => {
            let extra = s(p, "clientid")
                .map(|c| format!("customer_id={}", enc(c)))
                .unwrap_or_default();
            (
                Method::GET,
                format!("/api/v1/admin/domains?{}", q(&extra)),
                None,
            )
        }
        "GetContacts" => (
            Method::GET,
            format!(
                "/api/v1/admin/customers/{}/subaccounts",
                need("userid").or_else(|_| need("clientid"))?
            ),
            None,
        ),
        "GetEmails" => (
            Method::GET,
            format!("/api/v1/admin/customers/{}/emails", need("clientid")?),
            None,
        ),
        "AddClientNote" => (
            Method::POST,
            format!(
                "/api/v1/admin/customers/{}/notes",
                need("userid").or_else(|_| need("clientid"))?
            ),
            Some(
                json!({ "body": need("notes")?, "sticky": s(p, "sticky").is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) }),
            ),
        ),
        "GetClientNotes" => (
            Method::GET,
            format!(
                "/api/v1/admin/customers/{}/notes",
                need("userid").or_else(|_| need("clientid"))?
            ),
            None,
        ),
        "GetCancelledPackages" => (
            Method::GET,
            format!("/api/v1/admin/cancellations?{}", q("")),
            None,
        ),
        "GetTLDPricing" => (Method::GET, "/api/v1/admin/tlds".into(), None),
        // Servers / system
        "GetServers" => (Method::GET, "/api/v1/admin/servers".into(), None),
        "GetHealthStatus" => (Method::GET, "/api/v1/admin/servers/health".into(), None),
        "GetToDoItems" => (Method::GET, "/api/v1/admin/todos".into(), None),
        "GetEmailTemplates" => (Method::GET, "/api/v1/admin/email-templates".into(), None),
        "GetAdminUsers" => (Method::GET, format!("/api/v1/admin/users?{}", q("")), None),
        "GetActivityLog" => (
            Method::GET,
            format!("/api/v1/admin/audit-logs?{}", q("")),
            None,
        ),
        // Support
        "UpdateTicket" => {
            let mut b = Map::new();
            if let Some(v) = s(p, "status") {
                b.insert(
                    "status".into(),
                    json!(v.to_ascii_lowercase().replace([' ', '-'], "_")),
                );
            }
            if let Some(v) = s(p, "priority") {
                b.insert("priority".into(), json!(v.to_ascii_lowercase()));
            }
            if let Some(v) = s(p, "deptid") {
                b.insert("department_id".into(), json!(v));
            }
            if let Some(v) = s(p, "flag") {
                b.insert(
                    "assigned_to".into(),
                    if v == "0" { Value::Null } else { json!(v) },
                );
            }
            (
                Method::PATCH,
                format!("/api/v1/admin/tickets/{}", need("ticketid")?),
                Some(Value::Object(b)),
            )
        }
        "MergeTicket" => {
            // WHMCS merges `mergeticketids` into `ticketid`; one source per call here.
            let ids: Vec<String> = need("mergeticketids")?
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect();
            if ids.len() != 1 {
                return Err("Only one mergeticketids value per call is supported".into());
            }
            (
                Method::POST,
                format!("/api/v1/admin/tickets/{}/merge", ids[0]),
                Some(json!({ "target_id": need("ticketid")? })),
            )
        }
        "DeleteTicketReply" => (
            Method::DELETE,
            format!(
                "/api/v1/admin/tickets/{}/messages/{}",
                need("ticketid")?,
                need("replyid")?
            ),
            None,
        ),
        "UpdateTicketReply" => (
            Method::PUT,
            format!(
                "/api/v1/admin/tickets/{}/messages/{}",
                need("ticketid")?,
                need("replyid")?
            ),
            Some(json!({ "body": need("message")? })),
        ),
        "GetAnnouncements" => (Method::GET, "/api/v1/admin/announcements".into(), None),
        "DeleteAnnouncement" => (
            Method::DELETE,
            format!("/api/v1/admin/announcements/{}", need("announcementid")?),
            None,
        ),
        "GetAffiliates" => (Method::GET, "/api/v1/admin/affiliates".into(), None),
        // Project management
        "GetProjects" => (Method::GET, "/api/v1/admin/projects".into(), None),
        "GetProject" => (
            Method::GET,
            format!("/api/v1/admin/projects/{}", need("projectid")?),
            None,
        ),
        "CreateProject" | "UpdateProject" => {
            let mut b = json!({ "title": need("title")?, "description": s(p, "description").unwrap_or("") });
            if let Some(c) = s(p, "userid") {
                b["customer_id"] = json!(c);
            }
            if let Some(st) = s(p, "status") {
                b["status"] = json!(st.to_ascii_lowercase());
            }
            if let Some(d) = s(p, "duedate") {
                b["due_date"] = json!(d);
            }
            if action == "CreateProject" {
                (Method::POST, "/api/v1/admin/projects".into(), Some(b))
            } else {
                (
                    Method::PUT,
                    format!("/api/v1/admin/projects/{}", need("projectid")?),
                    Some(b),
                )
            }
        }
        "AddProjectTask" => {
            let mut b = json!({ "title": need("task")? });
            if let Some(d) = s(p, "duedate") {
                b["due_date"] = json!(d);
            }
            if let Some(a) = s(p, "adminid") {
                b["assignee_id"] = json!(a);
            }
            (
                Method::POST,
                format!("/api/v1/admin/projects/{}/tasks", need("projectid")?),
                Some(b),
            )
        }
        "DeleteProjectTask" => (
            Method::DELETE,
            format!(
                "/api/v1/admin/projects/{}/tasks/{}",
                need("projectid")?,
                need("taskid")?
            ),
            None,
        ),
        "AddProjectTime" => (
            Method::POST,
            format!("/api/v1/admin/projects/{}/time", need("projectid")?),
            Some(
                json!({ "task_id": s(p, "taskid"), "minutes": need("minutes")?, "note": s(p, "notes").unwrap_or("") }),
            ),
        ),
        _ => return Err("Command Not Found".into()),
    })
}

/// Wrap a native response in the WHMCS envelope (`result`, plus WHMCS-style
/// list counters for paged results). Native field names are kept.
pub fn envelope(action: &str, native: Value) -> Value {
    let mut out = Map::new();
    out.insert("result".into(), json!("success"));
    if let (Some(items), Some(total)) = (native.get("items"), native.get("total")) {
        let n = items.as_array().map(|a| a.len()).unwrap_or(0);
        let per = native["per_page"].as_i64().unwrap_or(n as i64);
        let page = native["page"].as_i64().unwrap_or(1);
        out.insert("totalresults".into(), total.clone());
        out.insert("startnumber".into(), json!((page - 1) * per));
        out.insert("numreturned".into(), json!(n));
        let key = match action {
            "GetClients" => "clients",
            "GetClientsProducts" => "products",
            "GetOrders" => "orders",
            "GetInvoices" => "invoices",
            "GetTickets" => "tickets",
            "GetTransactions" => "transactions",
            "GetClientsDomains" => "domains",
            "GetQuotes" => "quotes",
            "GetAdminUsers" => "admin_users",
            "GetActivityLog" => "activity",
            "GetCancelledPackages" => "packages",
            _ => "items",
        };
        out.insert(key.into(), items.clone());
        return Value::Object(out);
    }
    match native {
        Value::Object(m) => {
            for (k, v) in m {
                out.entry(k).or_insert(v);
            }
        }
        other => {
            out.insert("data".into(), other);
        }
    }
    Value::Object(out)
}

async fn params(
    req: Request,
) -> Result<(Map<String, Value>, Option<ConnectInfo<SocketAddr>>), String> {
    let ci = req.extensions().get::<ConnectInfo<SocketAddr>>().cloned();
    let ct = req
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let bytes = to_bytes(req.into_body(), 1 << 20)
        .await
        .map_err(|_| "Invalid request".to_string())?;
    let map = if ct.starts_with("application/json") {
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(Value::Object(m)) => m
                .into_iter()
                .map(|(k, v)| {
                    (
                        k,
                        if let Value::Number(n) = v {
                            json!(n.to_string())
                        } else {
                            v
                        },
                    )
                })
                .collect(),
            _ => return Err("Invalid request".into()),
        }
    } else {
        form_urlencoded::parse(&bytes)
            .map(|(k, v)| (k.into_owned(), json!(v.into_owned())))
            .collect()
    };
    Ok((map, ci))
}

#[utoipa::path(post, path = "/api/v1/compat/whmcs", responses((status = 200, description = "WHMCS envelope: result success|error")))]
pub async fn handle(State(st): State<AppState>, req: Request) -> Response {
    match reg::get(&st.pool, ADDON_ID).await {
        Ok(Some(a)) if a.enabled => {}
        _ => return (StatusCode::NOT_FOUND, Json(json!({ "result": "error", "message": "WHMCS API compatibility addon is not installed" }))).into_response(),
    }
    let (p, ci) = match params(req).await {
        Ok(x) => x,
        Err(e) => return err(&e),
    };
    if s(&p, "responsetype").is_some_and(|r| !r.eq_ignore_ascii_case("json")) {
        return err("Only responsetype=json is supported");
    }
    let (Some(ident), Some(secret)) = (s(&p, "identifier"), s(&p, "secret")) else {
        return err("Authentication Failed");
    };
    if !secret.starts_with(&format!("{ident}_"))
        || !ident.starts_with(crate::extract::API_KEY_PREFIX)
    {
        return err("Authentication Failed");
    }
    let action = s(&p, "action").unwrap_or("");
    let fixed = static_reply(action);
    let (method, path, body) = match translate(action, &p) {
        Ok(x) => x,
        Err(e) => return err(&e),
    };
    let mut b = Request::builder()
        .method(method)
        .uri(&path)
        .header(header::AUTHORIZATION, format!("Bearer {secret}"));
    let body = match body {
        Some(v) => {
            b = b.header(header::CONTENT_TYPE, "application/json");
            Body::from(v.to_string())
        }
        None => Body::empty(),
    };
    let Ok(mut inner) = b.body(body) else {
        return err("Invalid request");
    };
    if let Some(ci) = ci {
        inner.extensions_mut().insert(ci);
    }
    let app = Router::new()
        .nest("/api/v1", crate::routes::api_router())
        .layer(axum::middleware::from_fn_with_state(
            st.clone(),
            crate::error::localize_errors,
        ))
        .with_state(st.clone());
    let res = match app.oneshot(inner).await {
        Ok(r) => r,
        Err(e) => match e {},
    };
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 8 << 20).await.unwrap_or_default();
    let v: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    if status == StatusCode::UNAUTHORIZED {
        return err("Authentication Failed");
    }
    if status == StatusCode::FORBIDDEN {
        return err("Permission denied for this action");
    }
    if !status.is_success() {
        let msg = v["error"]["message"]
            .as_str()
            .or(v["error"]["code"].as_str())
            .unwrap_or("Request failed")
            .to_string();
        return err(&msg);
    }
    if let Some(f) = fixed {
        return Json(f).into_response();
    }
    Json(envelope(action, v)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn m(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }
    #[test]
    fn translates_actions() {
        let (mth, path, _) = translate(
            "GetClients",
            &m(json!({"limitstart": "50", "limitnum": "25", "search": "a b"})),
        )
        .unwrap();
        assert_eq!(
            (mth, path.as_str()),
            (
                Method::GET,
                "/api/v1/admin/customers?page=3&per_page=25&q=a+b"
            )
        );
        let (_, path, body) = translate(
            "AddInvoicePayment",
            &m(json!({"invoiceid": "x", "transid": "T1", "amount": "10.50"})),
        )
        .unwrap();
        assert_eq!(path, "/api/v1/admin/invoices/x/payments");
        assert_eq!(body.unwrap()["idempotency_key"], "whmcs:T1");
        let (_, _, body) = translate(
            "AddOrder",
            &m(json!({"clientid": "c", "pid": "p", "billingcycle": "Semi-Annually"})),
        )
        .unwrap();
        assert_eq!(body.unwrap()["items"][0]["cycle"], "semiannual");
        assert!(translate("Nope", &Map::new()).is_err());
        assert_eq!(
            translate("GetInvoice", &Map::new()).unwrap_err(),
            "Missing parameter: invoiceid"
        );
        for a in ACTIONS {
            // every advertised action is routed (may fail only on missing params)
            if let Err(e) = translate(a, &Map::new()) {
                assert!(
                    e.starts_with("Missing parameter") || e.starts_with("Only status"),
                    "{a}: {e}"
                );
            } else if static_reply(a).is_none() {
                // fine: routed with defaults
            }
        }
        let (mth, path, _) = translate("DeleteOrder", &m(json!({"orderid": "o1"}))).unwrap();
        assert_eq!(
            (mth, path.as_str()),
            (Method::DELETE, "/api/v1/admin/orders/o1")
        );
        let (_, _, b) = translate(
            "AddCredit",
            &m(json!({"clientid": "c", "amount": "5.00", "currency": "eur", "type": "remove"})),
        )
        .unwrap();
        assert_eq!(
            (
                b.as_ref().unwrap()["amount"].as_str(),
                b.as_ref().unwrap()["currency"].as_str()
            ),
            (Some("-5.00"), Some("EUR"))
        );
        let (_, path, b) = translate(
            "MergeTicket",
            &m(json!({"ticketid": "t1", "mergeticketids": "t2"})),
        )
        .unwrap();
        assert_eq!(
            (path.as_str(), b.unwrap()["target_id"].as_str()),
            ("/api/v1/admin/tickets/t2/merge", Some("t1"))
        );
        assert!(translate(
            "MergeTicket",
            &m(json!({"ticketid": "t1", "mergeticketids": "t2,t3"}))
        )
        .is_err());
        let (_, _, b) = translate("AddBillableItem", &m(json!({"clientid": "c", "description": "x", "amount": "1", "invoiceaction": "nextinvoice"}))).unwrap();
        assert_eq!(b.unwrap()["invoice_action"], "next_invoice");
        assert_eq!(static_reply("GetOrderStatuses").unwrap()["totalresults"], 4);
        let e = envelope(
            "GetClients",
            json!({"items": [1, 2], "total": 7, "page": 2, "per_page": 2}),
        );
        assert_eq!(
            e,
            json!({"result": "success", "totalresults": 7, "startnumber": 2, "numreturned": 2, "clients": [1, 2]})
        );
    }
}
