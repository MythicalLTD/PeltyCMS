//! `cargo xtask seed` / `seed-extra`: idempotent demo data through the real
//! HTTP API (port of scripts/seed-demo.sh and seed-demo-extra.sh).
//! Env: PELTY_URL (default http://localhost:8080), ADMIN_EMAIL, ADMIN_PASSWORD,
//! CLIENT_PASSWORD; SEED_DEBUG=1 also prints refused requests. No secrets live here.
use crate::util::{Ctx, R};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

struct Env {
    base: String,
    admin_email: String,
    admin_password: String,
    client_password: String,
}

fn env(ctx: &Ctx) -> R<Env> {
    let need = |k: &str| {
        ctx.get(k).ok_or_else(|| format!("{k} is not set (seed needs ADMIN_EMAIL, ADMIN_PASSWORD, CLIENT_PASSWORD; PELTY_URL optional)"))
    };
    Ok(Env {
        base: format!(
            "{}/api/v1",
            ctx.get("PELTY_URL")
                .unwrap_or_else(|| "http://localhost:8080".into())
                .trim_end_matches('/')
        ),
        admin_email: need("ADMIN_EMAIL")?,
        admin_password: need("ADMIN_PASSWORD")?,
        client_password: need("CLIENT_PASSWORD")?,
    })
}

/// A cookie session against the API (CSRF token sent once logged in).
struct Session {
    base: String,
    agent: ureq::Agent,
    cookies: BTreeMap<String, String>,
    csrf: Option<String>,
}

impl Session {
    fn new(base: &str) -> Session {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .build()
            .into();
        Session {
            base: base.into(),
            agent,
            cookies: BTreeMap::new(),
            csrf: None,
        }
    }

    /// (status, JSON body or the text as a string). Network errors are status 0.
    fn req(&mut self, method: &str, path: &str, body: Option<&Value>) -> (u16, Value) {
        let mut b = ureq::http::Request::builder()
            .method(method)
            .uri(format!("{}{path}", self.base))
            .header("content-type", "application/json");
        if let Some(t) = &self.csrf {
            b = b.header("x-csrf-token", t);
        }
        if !self.cookies.is_empty() {
            b = b.header(
                "cookie",
                self.cookies
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join("; "),
            );
        }
        let res = match body {
            Some(v) => b
                .body(serde_json::to_vec(v).unwrap_or_default())
                .map_err(|e| e.to_string())
                .and_then(|r| self.agent.run(r).map_err(|e| e.to_string())),
            None => b
                .body(())
                .map_err(|e| e.to_string())
                .and_then(|r| self.agent.run(r).map_err(|e| e.to_string())),
        };
        let mut resp = match res {
            Ok(r) => r,
            Err(e) => return (0, Value::String(e)),
        };
        for h in resp.headers().get_all("set-cookie") {
            if let Some((k, v)) = h
                .to_str()
                .ok()
                .and_then(|s| s.split(';').next())
                .and_then(|kv| kv.split_once('='))
            {
                self.cookies.insert(k.trim().into(), v.trim().into());
            }
        }
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let t = text.trim_start();
        let v = if t.starts_with('{') || t.starts_with('[') {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        } else {
            Value::String(text)
        };
        (status, v)
    }

    fn login(&mut self, email: &str, password: &str) -> R {
        let (c, b) = self.req(
            "POST",
            "/auth/login",
            Some(&json!({"email": email, "password": password})),
        );
        if !(200..300).contains(&c) {
            return Err(format!("login failed for {email}: {c}"));
        }
        self.csrf = b
            .get("csrf_token")
            .and_then(Value::as_str)
            .map(String::from)
            .or_else(|| {
                self.cookies
                    .iter()
                    .find(|(k, _)| k.to_ascii_lowercase().contains("csrf"))
                    .map(|(_, v)| v.clone())
            });
        Ok(())
    }
}

fn ok(c: u16) -> bool {
    (200..300).contains(&c)
}

/// Python-style truthiness of a JSON value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        _ => true,
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or_default()
}

/// The list in a list response (`[...]` or `{items|data|...: [...]}`).
fn items(b: &Value) -> Vec<Value> {
    if let Value::Array(a) = b {
        return a.clone();
    }
    for k in [
        "items",
        "data",
        "results",
        "products",
        "groups",
        "customers",
        "rules",
        "coupons",
        "servers",
        "articles",
        "categories",
        "announcements",
        "tickets",
        "departments",
        "invoices",
        "orders",
    ] {
        if let Some(Value::Array(a)) = b.get(k) {
            return a.clone();
        }
    }
    vec![]
}

/// `{"group": {...id...}}` -> the inner object.
fn unwrap(x: Value) -> Value {
    if let Value::Object(m) = &x {
        if !m.contains_key("id") {
            if let Some(inner) = m.values().find(|v| v.get("id").is_some()) {
                return inner.clone();
            }
        }
    }
    x
}

struct Seeder {
    a: Session,
    created: BTreeMap<String, u32>,
    existing: BTreeMap<String, u32>,
    refused: Vec<String>,
}

impl Seeder {
    /// A request whose failure is recorded under "refused".
    fn req(&mut self, m: &str, p: &str, body: Option<Value>) -> (u16, Value) {
        let (c, b) = self.a.req(m, p, body.as_ref());
        if c >= 400 || c == 0 {
            let t = match &b {
                Value::String(s) => s.chars().take(300).collect(),
                v => v.to_string().chars().take(300).collect::<String>(),
            };
            self.refused.push(format!("{m} {p} -> {c} {t}"));
        }
        (c, b)
    }

    fn get(&mut self, p: &str) -> Value {
        self.req("GET", p, None).1
    }

    fn created(&mut self, k: &str) {
        *self.created.entry(k.into()).or_default() += 1;
    }

    fn existing(&mut self, k: &str) {
        *self.existing.entry(k.into()).or_default() += 1;
    }

    /// Finds a matching row in `list_path`, else creates it.
    fn ensure(
        &mut self,
        kind: &str,
        list_path: &str,
        matches: impl Fn(&Value) -> bool,
        create_path: &str,
        body: Value,
    ) -> Option<Value> {
        let list = self.get(list_path);
        if let Some(x) = items(&list).into_iter().find(|x| matches(x)) {
            self.existing(kind);
            return Some(unwrap(x));
        }
        let (c, x) = self.req("POST", create_path, Some(body));
        if ok(c) {
            self.created(kind);
            return Some(unwrap(x));
        }
        None
    }

    /// Configurable options of a product, read from the public catalog.
    fn catalog_options(&mut self, pid: &Value) -> Vec<Value> {
        let (_, d) = self.a.req("GET", "/catalog?currency=EUR", None);
        let mut found = vec![];
        fn walk(v: &Value, pid: &Value, found: &mut Vec<Value>) {
            match v {
                Value::Object(m) => {
                    if m.get("id") == Some(pid) {
                        if let Some(Value::Array(o)) = m.get("options") {
                            found.extend(o.iter().cloned());
                        }
                    }
                    m.values().for_each(|w| walk(w, pid, found));
                }
                Value::Array(a) => a.iter().for_each(|w| walk(w, pid, found)),
                _ => {}
            }
        }
        walk(&d, pid, &mut found);
        found
    }
}

/// Decimal string/number -> millionths, for `total - amount_paid`.
fn micros(v: Option<&Value>) -> i128 {
    let t = match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return 0,
    };
    let (neg, t) = t
        .strip_prefix('-')
        .map(|r| (true, r))
        .unwrap_or((false, t.as_str()));
    let (int, frac) = t.split_once('.').unwrap_or((t, ""));
    let frac: String = frac.chars().chain(std::iter::repeat('0')).take(6).collect();
    let n = int.parse::<i128>().unwrap_or(0) * 1_000_000 + frac.parse::<i128>().unwrap_or(0);
    if neg {
        -n
    } else {
        n
    }
}

/// Rounds millionths to cents (half to even, like Decimal.quantize) as "12.34".
fn cents_str(m: i128) -> String {
    let (q, r) = (m.div_euclid(10_000), m.rem_euclid(10_000));
    let c = if r > 5_000 || (r == 5_000 && q % 2 != 0) {
        q + 1
    } else {
        q
    };
    format!("{}.{:02}", c / 100, c % 100)
}

pub fn seed(ctx: &Ctx) -> R {
    let e = env(ctx)?;
    let mut a = Session::new(&e.base);
    a.login(&e.admin_email, &e.admin_password)?;
    let mut sd = Seeder {
        a,
        created: BTreeMap::new(),
        existing: BTreeMap::new(),
        refused: vec![],
    };
    let res = seed_inner(&mut sd, &e);
    if ctx.get("SEED_DEBUG").is_some() {
        println!("REFUSED:\n{}", sd.refused.join("\n"));
    }
    res?;
    let out = json!({"created": sd.created, "existing": sd.existing, "refused": sd.refused});
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}

fn seed_inner(sd: &mut Seeder, e: &Env) -> R {
    // Product groups
    let mut groups: BTreeMap<&str, Option<Value>> = BTreeMap::new();
    for name in ["Game Servers", "Web Hosting"] {
        let slug = name.to_lowercase().replace(' ', "-");
        let g = sd.ensure(
            "group",
            "/admin/catalog/groups",
            |x| s(x, "name") == name,
            "/admin/catalog/groups",
            json!({"name": name, "slug": slug, "sort_order": 0, "hidden": false}),
        );
        groups.insert(name, g);
    }

    // Mock provisioning server (webhook module pointed at a non-routable mock URL)
    let srv = sd.ensure(
        "server",
        "/admin/servers",
        |x| s(x, "name") == "Demo Node FRA-1",
        "/admin/servers",
        json!({"name": "Demo Node FRA-1", "module": "webhook", "base_url": "http://127.0.0.1:9/mock", "enabled": true,
               "max_services": 200, "config": {}, "api_key": "demo-mock-key-not-a-secret"}),
    );

    let prices = |m: &str, q: &str, y: &str| {
        json!([{"currency": "EUR", "cycle": "monthly", "amount": m, "setup_fee": "0.00"},
               {"currency": "EUR", "cycle": "quarterly", "amount": q, "setup_fee": "0.00"},
               {"currency": "EUR", "cycle": "annual", "amount": y, "setup_fee": "0.00"}])
    };
    let products = [
        (
            "Game Servers",
            "minecraft-2gb",
            "Minecraft 2 GB",
            "2 GB RAM, 2 vCPU, 20 GB NVMe",
            prices("4.99", "13.99", "49.90"),
        ),
        (
            "Game Servers",
            "minecraft-8gb",
            "Minecraft 8 GB",
            "8 GB RAM, 4 vCPU, 80 GB NVMe",
            prices("16.99", "47.99", "169.90"),
        ),
        (
            "Game Servers",
            "rust-pro",
            "Rust Pro",
            "12 GB RAM, dedicated cores",
            prices("24.99", "70.99", "249.90"),
        ),
        (
            "Web Hosting",
            "web-starter",
            "Web Starter",
            "1 site, 10 GB SSD, free SSL",
            prices("2.99", "8.49", "29.90"),
        ),
        (
            "Web Hosting",
            "web-business",
            "Web Business",
            "Unlimited sites, 100 GB SSD, daily backups",
            prices("9.99", "28.49", "99.90"),
        ),
        (
            "Web Hosting",
            "web-agency",
            "Web Agency",
            "Reseller-ready, 250 GB SSD, priority support",
            prices("19.99", "56.99", "199.90"),
        ),
    ];
    let pl = sd.get("/admin/catalog/products");
    let existing: BTreeMap<String, Value> = items(&pl)
        .into_iter()
        .map(|x| (s(&x, "slug").to_string(), x))
        .collect();
    let mut by_slug: BTreeMap<&str, Value> = BTreeMap::new();
    for (grp, slug, name, desc, pr) in products {
        let x = if let Some(x) = existing.get(slug) {
            sd.existing("product");
            x.clone()
        } else {
            let gid = groups
                .get(grp)
                .cloned()
                .flatten()
                .and_then(|g| g.get("id").cloned())
                .unwrap_or(Value::Null);
            let (c, x) = sd.req("POST", "/admin/catalog/products", Some(json!({"name": name, "slug": slug, "description": desc, "group_id": gid, "status": "active"})));
            if !ok(c) {
                continue;
            }
            sd.created("product");
            unwrap(x)
        };
        let id = x["id"].clone();
        let ids = id.as_str().unwrap_or_default().to_string();
        sd.req(
            "PUT",
            &format!("/admin/catalog/products/{ids}/prices"),
            Some(json!({"prices": pr})),
        );
        if let Some(srv) = srv.as_ref().filter(|v| truthy(Some(v))) {
            if grp == "Game Servers" {
                sd.req(
                    "PUT",
                    &format!("/admin/products/{ids}/module"),
                    Some(json!({"module": "webhook", "server_id": srv.get("id"), "config": {}})),
                );
            }
        }
        let opts: Vec<Value> = sd
            .catalog_options(&id)
            .into_iter()
            .filter(|o| s(o, "name") == "Server location")
            .collect();
        for dup in opts.iter().skip(1) {
            // self-heal duplicates from earlier runs
            sd.req(
                "DELETE",
                &format!("/admin/catalog/products/{ids}/options/{}", s(dup, "id")),
                None,
            );
        }
        if slug == "minecraft-8gb" && opts.is_empty() {
            sd.req("POST", &format!("/admin/catalog/products/{ids}/options"), Some(json!({
                "name": "Server location", "option_type": "dropdown", "required": true, "qty_min": 0, "qty_max": 0,
                "choices": [{"label": "Frankfurt", "prices": []},
                            {"label": "Bucharest", "prices": [{"currency": "EUR", "cycle": "monthly", "amount": "1.00"}]}]})));
        }
        by_slug.insert(slug, x);
    }

    sd.ensure("coupon", "/admin/coupons", |x| s(x, "code") == "WELCOME10", "/admin/coupons",
        json!({"code": "WELCOME10", "kind": "percent", "value": "10", "description": "10% off first order",
               "active": true, "recurring": false, "max_uses": 500, "max_uses_per_customer": 1, "product_ids": []}));
    sd.ensure(
        "tax-rule",
        "/admin/tax-rules",
        |x| s(x, "name") == "RO VAT",
        "/admin/tax-rules",
        json!({"name": "RO VAT", "rate": "19.00", "country": "RO", "region": "", "is_vat": true,
               "compound": false, "enabled": true, "priority": 1}),
    );

    // Clients: register publicly (sets password), then fill profile as admin
    let clients = [
        ("Andrei", "Popescu", "RO", "Bucharest", "andrei.popescu"),
        ("Maria", "Ionescu", "RO", "Cluj-Napoca", "maria.ionescu"),
        ("Lukas", "Huber", "AT", "Vienna", "lukas.huber"),
        ("Sophie", "Martin", "FR", "Lyon", "sophie.martin"),
        ("James", "Carter", "GB", "London", "james.carter"),
        ("Elena", "Dumitru", "RO", "Iasi", "elena.dumitru"),
    ];
    let find_customer = |sd: &mut Seeder, email: &str| -> Option<Value> {
        let cl = sd.get("/admin/customers?per_page=200");
        items(&cl).into_iter().find(|x| s(x, "email") == email)
    };
    let mut customers: Vec<Value> = vec![];
    for (fn_, ln, co, city, local) in clients {
        let email = format!("{local}@demo.local");
        let mut cust = find_customer(sd, &email);
        if cust.is_none() {
            let mut u = Session::new(&e.base);
            u.req("POST", "/auth/register", Some(&json!({"email": email, "password": e.client_password, "display_name": format!("{fn_} {ln}")})));
            // first client request creates the customer row linked to the login
            let mut u = Session::new(&e.base);
            u.login(&email, &e.client_password)?;
            u.req("GET", "/client/invoices", None);
            cust = find_customer(sd, &email);
            if cust.is_none() {
                let (c, x) = sd.req(
                    "POST",
                    "/admin/customers",
                    Some(
                        json!({"email": email, "first_name": fn_, "last_name": ln, "country": co}),
                    ),
                );
                cust = ok(c).then(|| x.get("data").cloned().unwrap_or(x));
            }
            if cust.is_some() {
                sd.created("client");
            }
        } else {
            sd.existing("client");
        }
        let Some(mut cust) = cust else { continue };
        if cust.get("id").is_none() {
            cust = find_customer(sd, &email).unwrap_or(cust);
        }
        match cust.get("id").and_then(Value::as_str).map(String::from) {
            Some(id) => {
                sd.a.req("PUT", &format!("/admin/customers/{id}"), Some(&json!({"first_name": fn_, "last_name": ln, "country": co,
                    "city": city, "currency": "EUR", "address_line1": "Str. Demo 1", "postal_code": "010101"})));
                customers.push(cust);
            }
            None => {
                let t: String = cust.to_string().chars().take(200).collect();
                sd.refused.push(format!("customer {email}: no id in {t}"));
            }
        }
    }

    // Orders, invoices and payments, each idempotent per client
    let invs = items(&sd.get("/admin/invoices?per_page=200"));
    let have_order: Vec<Value> = items(&sd.get("/admin/orders?per_page=200"))
        .iter()
        .map(|x| x.get("customer_id").cloned().unwrap_or(Value::Null))
        .collect();
    let plan = [
        "minecraft-2gb",
        "web-business",
        "minecraft-8gb",
        "web-starter",
        "rust-pro",
        "web-agency",
    ];
    for (i, cust) in customers.iter().enumerate() {
        let cid = cust["id"].clone();
        let prod = plan.get(i).and_then(|p| by_slug.get(p)).cloned();
        if have_order.contains(&cid) {
            sd.existing("order");
        } else if let Some(prod) = prod {
            let mut config = Map::new();
            for o in sd.catalog_options(&prod["id"]) {
                if let Some(ch) = o
                    .get("choices")
                    .and_then(Value::as_array)
                    .filter(|c| !c.is_empty())
                {
                    config.insert(s(&o, "id").into(), ch[i % ch.len()]["id"].clone());
                }
            }
            let (c, _) = sd.req("POST", "/admin/orders", Some(json!({"customer_id": cid, "approve": true, "currency": "EUR",
                "coupon": if i == 0 { json!("WELCOME10") } else { Value::Null },
                "items": [{"product_id": prod["id"], "cycle": "monthly", "fields": {}, "addons": [], "config": config}]})));
            if ok(c) {
                sd.created("order");
            }
        }
        let mine: Vec<&Value> = {
            let own: Vec<&Value> = invs
                .iter()
                .filter(|x| x.get("customer_id") == Some(&cid))
                .collect();
            let with_support: Vec<&Value> = own
                .iter()
                .copied()
                .filter(|x| x.to_string().contains("Priority support"))
                .collect();
            if with_support.is_empty() {
                own.into_iter()
                    .filter(|x| !truthy(x.get("order_id")))
                    .collect()
            } else {
                with_support
            }
        };
        let x = if let Some(x) = mine.first() {
            sd.existing("invoice");
            (*x).clone()
        } else {
            let (c, x) = sd.req("POST", "/admin/invoices", Some(json!({"customer_id": cid, "currency": "EUR", "finalize": true,
                "items": [{"description": "Domain registration example.com (1y)", "quantity": "1", "unit_price": "11.99"},
                          {"description": "Priority support add-on", "quantity": "1", "unit_price": "5.00"}]})));
            if !ok(c) {
                continue;
            }
            sd.created("invoice");
            unwrap(x)
        };
        if i % 2 == 0 && s(&x, "status") != "paid" {
            let xid = s(&x, "id").to_string();
            let full = unwrap(sd.get(&format!("/admin/invoices/{xid}")));
            let due = micros(full.get("total")) - micros(full.get("amount_paid"));
            if s(&full, "status") != "paid" && due > 0 {
                let short: String = xid.chars().take(8).collect();
                let (c, _) = sd.req("POST", &format!("/admin/invoices/{xid}/payments"), Some(json!({"amount": cents_str(due), "gateway": "bank_transfer",
                    "idempotency_key": format!("seed-paid-{xid}"), "reference": format!("SEED-BANK-{short}")})));
                if ok(c) {
                    sd.created("payment");
                }
            }
        }
    }

    // Tickets
    let dep = sd.ensure("department", "/admin/ticket-departments", |x| s(x, "name") == "Technical Support", "/admin/ticket-departments",
        json!({"name": "Technical Support", "description": "Servers, panels, DNS", "is_active": true}));
    let tl = items(&sd.get("/admin/tickets?per_page=200"));
    let tickets = [
        (
            "Server not starting after update",
            "high",
            "My Minecraft server crashes on boot since last night.",
            "We rolled back the Java version on your node, please try again.",
        ),
        (
            "Invoice question",
            "low",
            "Can I get the invoice with my company VAT number?",
            "Sure, we updated your profile and reissued it.",
        ),
        (
            "DNS propagation",
            "medium",
            "My domain still points to the old IP.",
            "Propagation can take up to 24h; TTL is now 300s.",
        ),
        (
            "Upgrade to 8 GB plan",
            "medium",
            "How do I upgrade without losing my world?",
            "Upgrades are in place, data is kept. Prorated invoice sent.",
        ),
    ];
    if let Some(dep) = dep {
        for (i, (subj, pr, body, reply)) in tickets.into_iter().enumerate() {
            let reply_body = json!({"body": reply, "internal": false, "attachment_ids": []});
            if let Some(t) = tl.iter().find(|x| s(x, "subject") == subj) {
                sd.existing("ticket");
                let tid = s(t, "id").to_string();
                let full = sd.get(&format!("/admin/tickets/{tid}"));
                if !full.to_string().contains(reply)
                    && ok(sd
                        .req(
                            "POST",
                            &format!("/admin/tickets/{tid}/reply"),
                            Some(reply_body),
                        )
                        .0)
                {
                    sd.created("ticket-reply");
                }
                continue;
            }
            let cid = customers
                .get(i)
                .map(|c| c["id"].clone())
                .unwrap_or(Value::Null);
            let (c, t) = sd.req("POST", "/admin/tickets", Some(json!({"department_id": dep["id"], "subject": subj, "body": body, "priority": pr,
                "customer_id": cid, "attachment_ids": []})));
            if ok(c) {
                let t = unwrap(t);
                sd.created("ticket");
                if ok(sd
                    .req(
                        "POST",
                        &format!("/admin/tickets/{}/reply", s(&t, "id")),
                        Some(reply_body),
                    )
                    .0)
                {
                    sd.created("ticket-reply");
                }
            }
        }
    }

    let cat = sd.ensure("kb-category", "/admin/kb/categories", |x| s(x, "slug") == "getting-started", "/admin/kb/categories",
        json!({"slug": "getting-started", "name": "Getting started", "description": "First steps", "sort_order": 1}));
    if let Some(cat) = cat {
        for (slug, title, body) in [
            (
                "connect-to-your-server",
                "Connect to your game server",
                "Use the panel link from your service page...",
            ),
            (
                "pay-an-invoice",
                "How to pay an invoice",
                "Open Invoices, choose an unpaid invoice and click Pay...",
            ),
            (
                "reset-password",
                "Reset your password",
                "Click 'Forgot password' on the login page...",
            ),
        ] {
            sd.ensure("kb-article", "/admin/kb/articles", |x| s(x, "slug") == slug, "/admin/kb/articles",
                json!({"category_id": cat["id"], "slug": slug, "title": title, "body": body, "is_published": true}));
        }
    }
    sd.ensure("announcement", "/admin/announcements", |x| s(x, "title") == "Welcome to PeltyCMS demo", "/admin/announcements",
        json!({"title": "Welcome to PeltyCMS demo", "body": "New Frankfurt NVMe nodes are live.", "is_published": true}));

    // Demo accounts are trusted: mark every demo user's email as confirmed (no permanent banner).
    for u in items(&sd.get("/admin/users?limit=500")) {
        if !truthy(u.get("email_verified_at")) {
            sd.req(
                "POST",
                &format!("/admin/users/{}/verify-email", s(&u, "id")),
                None,
            );
        }
    }
    Ok(())
}

/// Extra demo data for newer features (port of seed-demo-extra.sh). Run after `seed`.
pub fn seed_extra(ctx: &Ctx) -> R {
    let e = env(ctx)?;
    let mut a = Session::new(&e.base);
    a.login(&e.admin_email, &e.admin_password)?;
    let mut res = Map::new();
    // Lists of objects: the response itself or its first non-empty list of objects.
    fn items(b: &Value) -> Vec<Value> {
        match b {
            Value::Array(a) => a.iter().filter(|x| x.is_object()).cloned().collect(),
            Value::Object(m) => m
                .values()
                .find_map(|v| {
                    v.as_array()
                        .filter(|a| !a.is_empty() && a.iter().all(Value::is_object))
                        .cloned()
                })
                .unwrap_or_default(),
            _ => vec![],
        }
    }
    let rec = |res: &mut Map<String, Value>, k: &str, r: &(u16, Value)| {
        res.insert(
            k.into(),
            if r.0 >= 300 || r.0 == 0 {
                json!(r.0)
            } else {
                json!("ok")
            },
        );
    };
    let exists = |res: &mut Map<String, Value>, k: &str| {
        res.insert(k.into(), json!("exists"));
    };

    // live chat on
    let (c, mut g) = a.req("GET", "/admin/general-settings", None);
    if c == 200 {
        if let Value::Object(m) = &mut g {
            let support = m.entry("support").or_insert_with(|| json!({}));
            if let Value::Object(sup) = support {
                sup.insert("live_chat".into(), json!(true));
            }
            let r = a.req("PUT", "/admin/general-settings", Some(&g));
            rec(&mut res, "live_chat_setting", &r);
        }
    }
    // server group
    let gl = a.req("GET", "/admin/server-groups", None).1;
    if !items(&gl).iter().any(|x| s(x, "name") == "EU Nodes") {
        let r = a.req(
            "POST",
            "/admin/server-groups",
            Some(&json!({"name": "EU Nodes", "strategy": "least_services"})),
        );
        rec(&mut res, "server_group", &r);
        if r.0 != 0 && r.0 < 300 {
            let srv: Vec<Value> = items(&a.req("GET", "/admin/servers", None).1)
                .iter()
                .map(|x| x["id"].clone())
                .collect();
            let m = a.req(
                "PUT",
                &format!("/admin/server-groups/{}/servers", s(&r.1, "id")),
                Some(&json!({"servers": srv})),
            );
            rec(&mut res, "server_group_members", &m);
        }
    } else {
        exists(&mut res, "server_group");
    }
    // module log entries via health check + test
    let r = a.req("POST", "/admin/servers/health/check", Some(&json!({})));
    rec(&mut res, "health_check", &r);
    for srv in items(&a.req("GET", "/admin/servers", None).1) {
        a.req(
            "POST",
            &format!("/admin/servers/{}/test", s(&srv, "id")),
            Some(&json!({})),
        );
    }
    let n = items(&a.req("GET", "/admin/module-log", None).1).len();
    res.insert("module_log_entries".into(), json!(n));
    let mut prods = items(&a.req("GET", "/admin/catalog/products", None).1);
    if prods.is_empty() {
        prods = items(&a.req("GET", "/admin/products", None).1);
    }
    if let Some(p0) = prods.first() {
        let pid = s(p0, "id").to_string();
        let r = a.req(
            "PUT",
            &format!("/admin/catalog/products/{pid}/stock"),
            Some(&json!({"stock_enabled": true, "stock_qty": 25, "allow_backorder": false})),
        );
        rec(&mut res, "stock", &r);
        let fl = a.req("GET", "/admin/order-forms", None).1;
        if !items(&fl).iter().any(|x| s(x, "slug") == "vps-wizard") {
            let ids: Vec<Value> = prods.iter().take(3).map(|p| p["id"].clone()).collect();
            let r = a.req("POST", "/admin/order-forms", Some(&json!({"slug": "vps-wizard", "name": "VPS Wizard", "layout": "wizard", "product_ids": ids,
                "steps": ["product", "configure", "review", "checkout"],
                "fields": [{"key": "purpose", "label": "Purpose", "kind": "select", "options": ["web", "game"], "required": false}], "redirect_url": "/dashboard"})));
            rec(&mut res, "order_form", &r);
        } else {
            exists(&mut res, "order_form");
        }
    }
    let cl = a.req("GET", "/admin/coupons", None).1;
    if !items(&cl).iter().any(|x| s(x, "code") == "WELCOME20") {
        let r = a.req("POST", "/admin/coupons", Some(&json!({"code": "WELCOME20", "kind": "percent", "value": "20", "recurring": true, "recurring_cycles": 3, "applies_addons": true})));
        rec(&mut res, "promo", &r);
    } else {
        exists(&mut res, "promo");
    }
    if items(&a.req("GET", "/admin/gift-cards", None).1).is_empty() {
        let r = a.req(
            "POST",
            "/admin/gift-cards",
            Some(&json!({"currency": "EUR", "amount": "25"})),
        );
        rec(&mut res, "gift_card", &r);
    } else {
        exists(&mut res, "gift_card");
    }
    if items(&a.req("GET", "/admin/oauth-clients", None).1).is_empty() {
        let r = a.req("POST", "/admin/oauth-clients", Some(&json!({"name": "Demo Panel", "redirect_uris": ["https://panel.demo.local/oauth/callback"]})));
        rec(&mut res, "oauth_app", &r);
    } else {
        exists(&mut res, "oauth_app");
    }
    let invs = items(&a.req("GET", "/admin/invoices?per_page=50", None).1);
    if !invs.iter().any(|i| s(i, "kind") == "debit_note") {
        let base = invs.iter().find(|i| {
            let kind = i.get("kind").and_then(Value::as_str).unwrap_or("invoice");
            kind == "invoice" && matches!(s(i, "status"), "unpaid" | "paid")
        });
        if let Some(base) = base {
            let r = a.req("POST", &format!("/admin/invoices/{}/debit-note", s(base, "id")),
                Some(&json!({"lines": [{"description": "Extra bandwidth", "quantity": "2", "unit_price": "1.25"}], "reason": "Overage October"})));
            rec(&mut res, "debit_note", &r);
        }
    } else {
        exists(&mut res, "debit_note");
    }
    if items(&a.req("GET", "/admin/chats", None).1).is_empty() {
        let r = Session::new(&e.base).req("POST", "/chat/sessions", Some(&json!({"name": "Vera", "email": "vera@demo.local", "message": "Hi, is DDoS protection included?", "page": "/store"})));
        rec(&mut res, "live_chat", &r);
    } else {
        exists(&mut res, "live_chat");
    }
    let mut u = Session::new(&e.base);
    u.login("andrei.popescu@demo.local", &e.client_password)?;
    let (c, af) = u.req("GET", "/client/affiliate", None);
    if !ok(c) || !(af.is_object() && (truthy(af.get("id")) || truthy(af.get("affiliate")))) {
        let r = u.req("POST", "/client/affiliate", None);
        rec(&mut res, "affiliate", &r);
    } else {
        exists(&mut res, "affiliate");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&Value::Object(res)).unwrap_or_default()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_and_unwrap() {
        assert_eq!(
            cents_str(micros(Some(&json!("16.99"))) - micros(Some(&json!("0.00")))),
            "16.99"
        );
        assert_eq!(
            cents_str(micros(Some(&json!(21.5))) - micros(Some(&json!("5")))),
            "16.50"
        );
        assert_eq!(cents_str(micros(Some(&json!("0.125")))), "0.12");
        assert_eq!(cents_str(micros(Some(&json!("0.135")))), "0.14");
        assert_eq!(unwrap(json!({"group": {"id": "g1"}}))["id"], "g1");
        assert_eq!(items(&json!({"data": [1, 2]})).len(), 2);
        assert!(!truthy(Some(&json!(""))) && truthy(Some(&json!("x"))));
    }
}
