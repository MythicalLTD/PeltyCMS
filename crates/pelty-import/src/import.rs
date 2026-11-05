//! Writes a [`WhmcsData`] snapshot into PeltyCMS.
//!
//! Guarantees:
//! * single transaction, all or nothing; `dry_run` rolls back at the end;
//! * idempotent, every created row is recorded in `import_map`
//!   (`source='whmcs'`), re-runs skip already-imported records;
//! * money copied as exact decimals; invoice numbers preserved verbatim;
//! * client logins keep their WHMCS hash as `whmcs:<hash>` when it is bcrypt
//!   or the legacy `md5(salt.pw):salt`: the first successful login verifies
//!   it and rehashes to Argon2id. Anything else gets an unusable hash, so
//!   the client must use "forgot password".
//! * card numbers are never read or written.
use crate::map;
use crate::whmcs::WhmcsData;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::{BTreeMap, HashMap};
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Stored in `users.password_hash` for imported accounts. Not a PHC string,
/// so `pelty_auth::password::verify` always returns false for it.
pub const LOCKED_PASSWORD: &str = "!whmcs-import:password-reset-required";

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub created: u64,
    pub skipped_existing: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub dry_run: bool,
    /// `whmcs`, `blesta` or `paymenter`.
    pub source: String,
    pub entities: BTreeMap<&'static str, Counts>,
    pub warnings: Vec<String>,
}

impl Report {
    pub(crate) fn created(&mut self, e: &'static str) {
        self.entities.entry(e).or_default().created += 1;
    }
    pub(crate) fn skipped(&mut self, e: &'static str) {
        self.entities.entry(e).or_default().skipped_existing += 1;
    }
    pub fn render(&self) -> String {
        let mut s = format!(
            "WHMCS import {}\n",
            if self.dry_run {
                "(DRY RUN, nothing written)"
            } else {
                "(committed)"
            }
        );
        for (k, c) in &self.entities {
            s += &format!(
                "  {k:<16} created {:>6}   already imported {:>6}\n",
                c.created, c.skipped_existing
            );
        }
        for w in &self.warnings {
            s += &format!("  warning: {w}\n");
        }
        s
    }
}

#[derive(Debug)]
pub struct ImportError(pub String);
impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ImportError {}
impl From<sqlx::Error> for ImportError {
    fn from(e: sqlx::Error) -> Self {
        Self(format!("database: {e}"))
    }
}

pub(crate) type Tx<'a> = Transaction<'a, Postgres>;

pub(crate) async fn lookup(tx: &mut Tx<'_>, entity: &str, sid: &str) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT target_id FROM import_map WHERE source=current_setting('pelty.import_source') AND entity=$1 AND source_id=$2",
    )
    .bind(entity)
    .bind(sid)
    .fetch_optional(&mut **tx)
    .await
}

pub(crate) async fn remember(tx: &mut Tx<'_>, entity: &str, sid: &str, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO import_map (source, entity, source_id, target_id) VALUES (current_setting('pelty.import_source'),$1,$2,$3)",
    )
    .bind(entity)
    .bind(sid)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) fn ts(d: Option<Date>) -> Option<OffsetDateTime> {
    d.map(|d| d.midnight().assume_utc())
}

/// Run the import. With `dry_run` every statement executes (so constraint
/// violations surface) and the transaction is rolled back.
pub async fn run(pool: &PgPool, data: &WhmcsData, dry_run: bool) -> Result<Report, ImportError> {
    run_source(pool, data, dry_run, "whmcs").await
}

/// Same as [`run`] but records rows in `import_map` under `source`
/// (`whmcs`, `blesta`, `paymenter`), so ids from different systems never clash.
pub async fn run_source(
    pool: &PgPool,
    data: &WhmcsData,
    dry_run: bool,
    source: &str,
) -> Result<Report, ImportError> {
    run_with(pool, data, dry_run, source, None).await
}

/// Seals a server secret for `servers.api_key_sealed` (the server's
/// SecretBox under PELTY_ENCRYPTION_KEY).
pub type Sealer<'a> = &'a (dyn Fn(&[u8]) -> Vec<u8> + Sync);

/// [`run_source`] plus a sealer for server credentials. Without one,
/// servers are imported disabled with an empty key (re-enter it in Admin).
pub async fn run_with(
    pool: &PgPool,
    data: &WhmcsData,
    dry_run: bool,
    source: &str,
    seal: Option<Sealer<'_>>,
) -> Result<Report, ImportError> {
    let mut rep = Report {
        dry_run,
        source: source.to_string(),
        ..Default::default()
    };
    let src = source;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('pelty.import_source', $1, true)")
        .bind(source)
        .execute(&mut *tx)
        .await?;

    // ---- currencies
    let mut cur: HashMap<i64, String> = HashMap::new();
    let mut default_cur = String::from("EUR");
    for c in &data.currencies {
        let code = c.code.trim().to_ascii_uppercase();
        if code.len() != 3 || !code.chars().all(|x| x.is_ascii_uppercase()) {
            rep.warnings.push(format!(
                "currency {} has invalid code {:?}, skipped",
                c.id, c.code
            ));
            continue;
        }
        let r = sqlx::query(
            "INSERT INTO currencies (code, exponent) VALUES ($1, 2) ON CONFLICT (code) DO NOTHING",
        )
        .bind(&code)
        .execute(&mut *tx)
        .await?;
        if r.rows_affected() == 1 {
            rep.created("currencies")
        } else {
            rep.skipped("currencies")
        }
        if c.default {
            default_cur = code.clone();
        }
        cur.insert(c.id, code);
    }
    let cur_of = |id: i64, cur: &HashMap<i64, String>, d: &str| {
        cur.get(&id).cloned().unwrap_or_else(|| d.to_string())
    };

    // ---- client groups
    let mut cgroups: HashMap<i64, Uuid> = HashMap::new();
    for g in &data.client_groups {
        let sid = g.id.to_string();
        if let Some(id) = lookup(&mut tx, "client_group", &sid).await? {
            cgroups.insert(g.id, id);
            rep.skipped("client_groups");
            continue;
        }
        let name = if g.groupname.trim().is_empty() {
            format!("WHMCS group {}", g.id)
        } else {
            map::clip(g.groupname.trim(), 80)
        };
        // name is UNIQUE: reuse an existing group with the same name.
        let existing: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM client_groups WHERE name=$1")
                .bind(&name)
                .fetch_optional(&mut *tx)
                .await?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO client_groups (id, name, discount_percent, late_fee_exempt) VALUES ($1,$2,$3,$4)")
                    .bind(id).bind(&name).bind(g.discountpercent.clamp(Decimal::ZERO, Decimal::ONE_HUNDRED)).bind(g.susptermexempt)
                    .execute(&mut *tx).await?;
                rep.created("client_groups");
                id
            }
        };
        remember(&mut tx, "client_group", &sid, id).await?;
        cgroups.insert(g.id, id);
    }

    // ---- clients -> users (locked) + customers
    let customer_role: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM roles WHERE name='customer'")
            .fetch_optional(&mut *tx)
            .await?;
    let mut customers: HashMap<i64, (Uuid, String)> = HashMap::new();
    for c in &data.clients {
        let sid = c.id.to_string();
        let currency = cur_of(c.currency, &cur, &default_cur);
        if let Some(id) = lookup(&mut tx, "customer", &sid).await? {
            customers.insert(c.id, (id, currency));
            rep.skipped("customers");
            continue;
        }
        let email = c.email.trim().to_string();
        let mut owner = None;
        if email.len() >= 3 && email.contains('@') && email.len() <= 254 {
            let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM users WHERE email=$1")
                .bind(&email)
                .fetch_optional(&mut *tx)
                .await?;
            if exists.is_some() {
                rep.warnings.push(format!("client {} email {email} already has a PeltyCMS login; customer imported without login", c.id));
            } else {
                let uid = Uuid::new_v4();
                let name = map::clip(format!("{} {}", c.firstname, c.lastname).trim(), 100);
                let hash = crate::whmcs_crypt::login_hash(&c.password);
                if hash.is_some() {
                    rep.created("password_hashes_kept");
                }
                sqlx::query("INSERT INTO users (id, email, password_hash, display_name, is_active) VALUES ($1,$2,$3,$4,$5)")
                    .bind(uid).bind(&email).bind(hash.as_deref().unwrap_or(LOCKED_PASSWORD)).bind(name)
                    .bind(map::client_status(&c.status) != "closed")
                    .execute(&mut *tx).await?;
                if let Some(r) = customer_role {
                    sqlx::query("INSERT INTO user_roles (user_id, role_id) VALUES ($1,$2)")
                        .bind(uid)
                        .bind(r)
                        .execute(&mut *tx)
                        .await?;
                }
                remember(&mut tx, "user", &sid, uid).await?;
                rep.created("users");
                owner = Some(uid);
            }
        } else {
            rep.warnings.push(format!(
                "client {} has invalid email {:?}; imported without login",
                c.id, c.email
            ));
        }
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO customers (id, owner_user_id, organization, first_name, last_name, email, phone, address_line1, address_line2, city, region, postal_code, country, tax_id, currency, status, client_group_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
            .bind(id).bind(owner)
            .bind(map::clip(&c.companyname, 200)).bind(map::clip(&c.firstname, 100)).bind(map::clip(&c.lastname, 100))
            .bind(map::clip(&email, 254)).bind(map::clip(&c.phonenumber, 40))
            .bind(map::clip(&c.address1, 200)).bind(map::clip(&c.address2, 200)).bind(map::clip(&c.city, 100))
            .bind(map::clip(&c.state, 100)).bind(map::clip(&c.postcode, 20)).bind(map::country(&c.country))
            .bind(map::clip(&c.tax_id, 40)).bind(&currency).bind(map::client_status(&c.status)).bind(cgroups.get(&c.groupid))
            .execute(&mut *tx).await?;
        remember(&mut tx, "customer", &sid, id).await?;
        rep.created("customers");
        customers.insert(c.id, (id, currency));
    }

    // ---- product groups
    let mut groups: HashMap<i64, Uuid> = HashMap::new();
    for g in &data.product_groups {
        let sid = g.id.to_string();
        if let Some(id) = lookup(&mut tx, "product_group", &sid).await? {
            groups.insert(g.id, id);
            rep.skipped("product_groups");
            continue;
        }
        let id = Uuid::new_v4();
        let name = if g.name.trim().is_empty() {
            format!("Group {}", g.id)
        } else {
            map::clip(&g.name, 120)
        };
        sqlx::query("INSERT INTO product_groups (id, name, slug, sort_order, hidden) VALUES ($1,$2,$3,$4,$5)")
            .bind(id).bind(name).bind(map::slug(&g.name, &format!("{src}-g"), g.id)).bind(g.order).bind(g.hidden)
            .execute(&mut *tx).await?;
        remember(&mut tx, "product_group", &sid, id).await?;
        rep.created("product_groups");
        groups.insert(g.id, id);
    }

    // ---- products + prices
    let mut products: HashMap<i64, Uuid> = HashMap::new();
    for p in &data.products {
        let sid = p.id.to_string();
        if let Some(id) = lookup(&mut tx, "product", &sid).await? {
            products.insert(p.id, id);
            rep.skipped("products");
            continue;
        }
        let Some(&gid) = groups.get(&p.gid) else {
            rep.warnings.push(format!(
                "product {} references missing group {}, skipped",
                p.id, p.gid
            ));
            continue;
        };
        let id = Uuid::new_v4();
        let status = if p.retired {
            "retired"
        } else if p.hidden {
            "hidden"
        } else {
            "active"
        };
        let name = if p.name.trim().is_empty() {
            format!("Product {}", p.id)
        } else {
            map::clip(&p.name, 200)
        };
        sqlx::query("INSERT INTO products (id, group_id, name, slug, description, status, module_config, sort_order) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(id).bind(gid).bind(name).bind(map::slug(&p.name, &format!("{src}-p"), p.id))
            .bind(map::clip(&p.description, 5000)).bind(status)
            .bind(serde_json::json!({ "whmcs_module": p.servertype }))
            .bind(p.order).execute(&mut *tx).await?;
        for pr in data.pricing.iter().filter(|x| x.relid == p.id) {
            let Some(code) = cur.get(&pr.currency) else {
                continue;
            };
            let rows = [
                ("monthly", pr.monthly, pr.msetupfee),
                ("quarterly", pr.quarterly, pr.qsetupfee),
                ("semiannual", pr.semiannually, pr.ssetupfee),
                ("annual", pr.annually, pr.asetupfee),
                ("biennial", pr.biennially, pr.bsetupfee),
                ("triennial", pr.triennially, pr.tsetupfee),
            ];
            for (cycle, amount, setup) in rows {
                if !map::price_enabled(amount) {
                    continue;
                }
                let setup = if map::price_enabled(setup) {
                    setup
                } else {
                    Decimal::ZERO
                };
                sqlx::query("INSERT INTO product_prices (id, product_id, currency, cycle, amount, setup_fee) VALUES ($1,$2,$3,$4,$5,$6)")
                    .bind(Uuid::new_v4()).bind(id).bind(code).bind(cycle).bind(amount).bind(setup)
                    .execute(&mut *tx).await?;
                rep.created("product_prices");
            }
        }
        remember(&mut tx, "product", &sid, id).await?;
        rep.created("products");
        products.insert(p.id, id);
    }

    let servers = crate::import_more::servers(&mut tx, data, &mut rep, seal).await?;

    // ---- services (tblhosting)
    let mut services: HashMap<i64, Uuid> = HashMap::new();
    for h in &data.hosting {
        let sid = h.id.to_string();
        if let Some(id) = lookup(&mut tx, "service", &sid).await? {
            services.insert(h.id, id);
            rep.skipped("services");
            continue;
        }
        let (Some((cid, ccur)), Some(&pid)) =
            (customers.get(&h.userid), products.get(&h.packageid))
        else {
            rep.warnings.push(format!(
                "service {} references missing client/product, skipped",
                h.id
            ));
            continue;
        };
        let Some(cycle) = map::cycle(&h.billingcycle) else {
            rep.warnings.push(format!(
                "service {} has unknown cycle {:?}, skipped",
                h.id, h.billingcycle
            ));
            continue;
        };
        let status = map::service_status(&h.domainstatus);
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO services (id, customer_id, product_id, status, cycle, currency, recurring_amount, next_due_date, activated_at, terminated_at, created_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,COALESCE($11, now()))")
            .bind(id).bind(cid).bind(pid).bind(status).bind(cycle).bind(ccur)
            .bind(h.amount.max(Decimal::ZERO)).bind(h.nextduedate)
            .bind(if status == "pending" { None } else { ts(h.regdate) })
            .bind(if status == "terminated" { ts(h.nextduedate) } else { None })
            .bind(ts(h.regdate))
            .execute(&mut *tx).await?;
        if let Some(srv) = servers.get(&h.server) {
            sqlx::query("UPDATE services SET server_id=$2, module_data=$3 WHERE id=$1")
                .bind(id).bind(srv)
                .bind(serde_json::json!({ "username": map::clip(&h.username, 100), "whmcs_server": h.server }))
                .execute(&mut *tx).await?;
        }
        remember(&mut tx, "service", &sid, id).await?;
        rep.created("services");
        services.insert(h.id, id);
    }

    // ---- invoices + items + transactions
    let mut paid: HashMap<i64, Decimal> = HashMap::new();
    for t in &data.transactions {
        if t.invoiceid > 0 {
            *paid.entry(t.invoiceid).or_default() += t.amountin - t.amountout;
        }
    }
    let mut invoices: HashMap<i64, Uuid> = HashMap::new();
    for inv in &data.invoices {
        let sid = inv.id.to_string();
        if let Some(id) = lookup(&mut tx, "invoice", &sid).await? {
            invoices.insert(inv.id, id);
            rep.skipped("invoices");
            continue;
        }
        let Some((cid, ccur)) = customers.get(&inv.userid) else {
            rep.warnings.push(format!(
                "invoice {} references missing client {}, skipped",
                inv.id, inv.userid
            ));
            continue;
        };
        let status = map::invoice_status(&inv.status);
        let number = map::invoice_number(inv.id, &inv.invoicenum);
        let taken: Option<Uuid> = sqlx::query_scalar("SELECT id FROM invoices WHERE number=$1")
            .bind(&number)
            .fetch_optional(&mut *tx)
            .await?;
        if taken.is_some() {
            rep.warnings.push(format!(
                "invoice {} number {number} already exists in PeltyCMS, skipped",
                inv.id
            ));
            continue;
        }
        let id = Uuid::new_v4();
        let tax_total = inv.tax + inv.tax2;
        let amount_paid =
            (paid.get(&inv.id).copied().unwrap_or_default() + inv.credit).max(Decimal::ZERO);
        // Insert as draft so the item guard allows lines, then finalize.
        sqlx::query("INSERT INTO invoices (id, customer_id, currency, status, subtotal, tax_total, total, amount_paid, issued_at, due_at, paid_at, created_at) \
                     VALUES ($1,$2,$3,'draft',$4,$5,$6,$7,$8,$9,$10,COALESCE($8, now()))")
            .bind(id).bind(cid).bind(ccur).bind(inv.subtotal).bind(tax_total).bind(inv.total)
            .bind(amount_paid).bind(ts(inv.date)).bind(ts(inv.duedate)).bind(ts(inv.datepaid))
            .execute(&mut *tx).await?;
        for (n, it) in data
            .invoice_items
            .iter()
            .filter(|x| x.invoiceid == inv.id)
            .enumerate()
        {
            let desc = if it.description.trim().is_empty() {
                "Item".to_string()
            } else {
                map::clip(&it.description, 500)
            };
            let rate = if it.taxed {
                inv.taxrate.clamp(Decimal::ZERO, Decimal::ONE_HUNDRED)
            } else {
                Decimal::ZERO
            };
            let svc = if it.type_.eq_ignore_ascii_case("hosting") {
                services.get(&it.relid).copied()
            } else {
                None
            };
            sqlx::query("INSERT INTO invoice_items (id, invoice_id, description, quantity, unit_price, tax_rate, amount, tax_amount, service_id, sort_order) \
                         VALUES ($1,$2,$3,1,$4,$5,$4,0,$6,$7)")
                .bind(Uuid::new_v4()).bind(id).bind(desc).bind(it.amount).bind(rate).bind(svc).bind(n as i32)
                .execute(&mut *tx).await?;
            rep.created("invoice_items");
        }
        if status != "draft" {
            sqlx::query("UPDATE invoices SET number=$2, status=$3, finalized_at=COALESCE(issued_at, now()) WHERE id=$1")
                .bind(id).bind(&number).bind(status).execute(&mut *tx).await?;
        }
        remember(&mut tx, "invoice", &sid, id).await?;
        rep.created("invoices");
        invoices.insert(inv.id, id);
    }

    for t in &data.transactions {
        let sid = t.id.to_string();
        if lookup(&mut tx, "transaction", &sid).await?.is_some() {
            rep.skipped("payments");
            continue;
        }
        let cust = customers.get(&t.userid);
        let currency = if t.currency > 0 {
            cur.get(&t.currency).cloned()
        } else {
            cust.map(|c| c.1.clone())
        }
        .unwrap_or_else(|| default_cur.clone());
        let user_id: Option<Uuid> =
            sqlx::query_scalar("SELECT owner_user_id FROM customers WHERE id=$1")
                .bind(cust.map(|c| c.0))
                .fetch_optional(&mut *tx)
                .await?
                .flatten();
        let (amount, status) = if t.amountin > Decimal::ZERO {
            (t.amountin, "succeeded")
        } else if t.amountout > Decimal::ZERO {
            (t.amountout, "refunded")
        } else {
            continue;
        };
        let id = Uuid::new_v4();
        let gateway = if t.gateway.trim().is_empty() {
            src.to_string()
        } else {
            format!("{src}:{}", t.gateway.trim())
        };
        let r_ins = sqlx::query("INSERT INTO payments (id, user_id, gateway, provider_ref, amount, currency, status, idempotency_key, invoice_id, customer_id, applied_at, created_at) \
                     VALUES ($1,$2,$3,NULL,$4,$5,$6,$7,$8,$9,$10,COALESCE($10, now())) ON CONFLICT (idempotency_key) DO NOTHING")
            .bind(id).bind(user_id).bind(gateway).bind(amount).bind(&currency).bind(status)
            .bind(format!("{src}:tx:{}:{}", t.id, map::clip(&t.transid, 120)))
            .bind(invoices.get(&t.invoiceid)).bind(cust.map(|c| c.0)).bind(ts(t.date))
            .execute(&mut *tx).await?;
        if r_ins.rows_affected() == 0 {
            rep.warnings.push(format!(
                "transaction {} already exists as a payment, skipped",
                t.id
            ));
            continue;
        }
        remember(&mut tx, "transaction", &sid, id).await?;
        rep.created("payments");
    }

    import_extras(
        &mut tx, data, &mut rep, &cur, &customers, &products, &services,
    )
    .await?;
    import_domains_options_credit(&mut tx, data, &mut rep, &cur, &customers, &products).await?;
    crate::import_more::rest(&mut tx, data, &mut rep, &default_cur, &customers, &products).await?;

    if dry_run {
        tx.rollback().await?;
    } else {
        tx.commit().await?;
    }
    Ok(rep)
}

const CYCLES: [&str; 6] = [
    "monthly",
    "quarterly",
    "semiannual",
    "annual",
    "biennial",
    "triennial",
];

fn pricing_rows(p: &crate::whmcs::Pricing) -> [(&'static str, Decimal, Decimal); 6] {
    [
        (CYCLES[0], p.monthly, p.msetupfee),
        (CYCLES[1], p.quarterly, p.qsetupfee),
        (CYCLES[2], p.semiannually, p.ssetupfee),
        (CYCLES[3], p.annually, p.asetupfee),
        (CYCLES[4], p.biennially, p.bsetupfee),
        (CYCLES[5], p.triennially, p.tsetupfee),
    ]
}

/// Addons, custom fields, tickets, announcements and the knowledge base.
#[allow(clippy::too_many_arguments)]
async fn import_extras(
    tx: &mut Tx<'_>,
    data: &WhmcsData,
    rep: &mut Report,
    cur: &HashMap<i64, String>,
    customers: &HashMap<i64, (Uuid, String)>,
    products: &HashMap<i64, Uuid>,
    services: &HashMap<i64, Uuid>,
) -> Result<(), ImportError> {
    let src = rep.source.clone();
    let src = src.as_str();
    // ---- addons (+ prices, product links)
    let mut addons: HashMap<i64, Uuid> = HashMap::new();
    for a in &data.addons {
        let sid = a.id.to_string();
        if let Some(id) = lookup(tx, "addon", &sid).await? {
            addons.insert(a.id, id);
            rep.skipped("addons");
            continue;
        }
        let id = Uuid::new_v4();
        let status = if a.retired {
            "retired"
        } else if a.hidden {
            "hidden"
        } else {
            "active"
        };
        let name = if a.name.trim().is_empty() {
            format!("Addon {}", a.id)
        } else {
            map::clip(&a.name, 200)
        };
        sqlx::query("INSERT INTO addons (id, name, description, status, sort_order) VALUES ($1,$2,$3,$4,$5)")
            .bind(id).bind(name).bind(map::clip(&a.description, 2000)).bind(status).bind(a.weight)
            .execute(&mut **tx).await?;
        let fixed_cycle = map::cycle(&a.billingcycle).filter(|c| *c == "one_time");
        for pr in data.addon_pricing.iter().filter(|p| p.relid == a.id) {
            let Some(code) = cur.get(&pr.currency) else {
                continue;
            };
            if fixed_cycle.is_some() {
                // One-time addons keep their price in the `monthly` column.
                if map::price_enabled(pr.monthly) {
                    let setup = if map::price_enabled(pr.msetupfee) {
                        pr.msetupfee
                    } else {
                        Decimal::ZERO
                    };
                    sqlx::query("INSERT INTO addon_prices (addon_id, currency, cycle, amount, setup_fee) VALUES ($1,$2,'one_time',$3,$4)")
                        .bind(id).bind(code).bind(pr.monthly).bind(setup).execute(&mut **tx).await?;
                    rep.created("addon_prices");
                }
                continue;
            }
            for (cycle, amount, setup) in pricing_rows(pr) {
                if !map::price_enabled(amount) {
                    continue;
                }
                let setup = if map::price_enabled(setup) {
                    setup
                } else {
                    Decimal::ZERO
                };
                sqlx::query("INSERT INTO addon_prices (addon_id, currency, cycle, amount, setup_fee) VALUES ($1,$2,$3,$4,$5)")
                    .bind(id).bind(code).bind(cycle).bind(amount).bind(setup).execute(&mut **tx).await?;
                rep.created("addon_prices");
            }
        }
        for pid in a
            .packages
            .split(',')
            .filter_map(|x| x.trim().parse::<i64>().ok())
        {
            if let Some(p) = products.get(&pid) {
                sqlx::query("INSERT INTO addon_products (addon_id, product_id) VALUES ($1,$2) ON CONFLICT DO NOTHING")
                    .bind(id).bind(p).execute(&mut **tx).await?;
            }
        }
        remember(tx, "addon", &sid, id).await?;
        rep.created("addons");
        addons.insert(a.id, id);
    }

    for ha in &data.hosting_addons {
        let sid = ha.id.to_string();
        if lookup(tx, "service_addon", &sid).await?.is_some() {
            rep.skipped("service_addons");
            continue;
        }
        let (Some(svc), Some(addon)) = (services.get(&ha.hostingid), addons.get(&ha.addonid))
        else {
            rep.warnings.push(format!(
                "hosting addon {} ({}) references missing service/addon, skipped",
                ha.id, ha.name
            ));
            continue;
        };
        let cycle = map::cycle(&ha.billingcycle).unwrap_or("one_time");
        let id = Uuid::new_v4();
        let r = sqlx::query("INSERT INTO service_addons (id, service_id, addon_id, status, cycle, recurring_amount, created_at) \
                             VALUES ($1,$2,$3,$4,$5,$6,COALESCE($7, now())) ON CONFLICT (service_id, addon_id) DO NOTHING")
            .bind(id).bind(svc).bind(addon).bind(map::addon_status(&ha.status)).bind(cycle)
            .bind(ha.recurring.max(Decimal::ZERO)).bind(ts(ha.regdate))
            .execute(&mut **tx).await?;
        if r.rows_affected() == 0 {
            rep.warnings.push(format!(
                "hosting addon {}: service already has this addon, skipped",
                ha.id
            ));
            continue;
        }
        remember(tx, "service_addon", &sid, id).await?;
        rep.created("service_addons");
    }

    // ---- custom fields (product fields only; PeltyCMS has no client fields yet)
    let mut fields: HashMap<i64, (Uuid, &'static str)> = HashMap::new();
    let mut client_fields = 0usize;
    for f in &data.custom_fields {
        if !f.type_.eq_ignore_ascii_case("product") {
            client_fields += 1;
            continue;
        }
        let sid = f.id.to_string();
        let (kind, choices) = map::field_kind(&f.fieldtype, &f.fieldoptions);
        if let Some(id) = lookup(tx, "custom_field", &sid).await? {
            fields.insert(f.id, (id, kind));
            rep.skipped("custom_fields");
            continue;
        }
        let Some(pid) = products.get(&f.relid) else {
            rep.warnings.push(format!(
                "custom field {} references missing product {}, skipped",
                f.id, f.relid
            ));
            continue;
        };
        let id = Uuid::new_v4();
        let name = if f.fieldname.trim().is_empty() {
            format!("Field {}", f.id)
        } else {
            map::clip(f.fieldname.trim(), 120)
        };
        // WHMCS regexes are PHP/PCRE-delimited; keep them only when they fit.
        let pattern = if f.regexpr.chars().count() <= 200 {
            f.regexpr.clone()
        } else {
            String::new()
        };
        sqlx::query("INSERT INTO product_custom_fields (id, product_id, name, description, kind, choices, pattern, required, admin_only, sort_order) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(id).bind(pid).bind(name).bind(map::clip(&f.description, 500)).bind(kind)
            .bind(serde_json::json!(choices)).bind(pattern).bind(f.required).bind(f.adminonly).bind(f.sortorder)
            .execute(&mut **tx).await?;
        remember(tx, "custom_field", &sid, id).await?;
        rep.created("custom_fields");
        fields.insert(f.id, (id, kind));
    }
    if client_fields > 0 {
        rep.warnings.push(format!("{client_fields} client-level custom field(s) not imported (no client custom fields in PeltyCMS yet)"));
    }
    // Values -> services.custom_fields (field uuid -> value). Merged with
    // `||`, so re-runs are idempotent and don't drop existing keys.
    let mut per_service: HashMap<Uuid, serde_json::Map<String, serde_json::Value>> = HashMap::new();
    for v in &data.custom_field_values {
        let (Some((fid, kind)), Some(svc)) = (fields.get(&v.fieldid), services.get(&v.relid))
        else {
            continue;
        };
        if let Some(val) = map::field_value(kind, &v.value) {
            per_service.entry(*svc).or_default().insert(
                fid.to_string(),
                serde_json::Value::String(map::clip(&val, 2000)),
            );
        }
    }
    for (svc, m) in per_service {
        let n = m.len() as u64;
        let r = sqlx::query("UPDATE services SET custom_fields = custom_fields || $2 WHERE id=$1 AND NOT custom_fields @> $2")
            .bind(svc).bind(serde_json::Value::Object(m)).execute(&mut **tx).await?;
        let e = rep.entities.entry("custom_values").or_default();
        if r.rows_affected() == 1 {
            e.created += n
        } else {
            e.skipped_existing += n
        }
    }

    // ---- ticket departments (matched by name) + tickets + replies
    let mut depts: HashMap<i64, Uuid> = HashMap::new();
    for d in &data.ticket_departments {
        let name = if d.name.trim().is_empty() {
            format!("Department {}", d.id)
        } else {
            map::clip(d.name.trim(), 100)
        };
        let existing: Option<Uuid> = sqlx::query_scalar("SELECT id FROM ticket_departments WHERE lower(name)=lower($1) ORDER BY sort_order LIMIT 1")
            .bind(&name).fetch_optional(&mut **tx).await?;
        let id = match existing {
            Some(id) => {
                rep.skipped("ticket_departments");
                id
            }
            None => {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO ticket_departments (id, name, description, is_active, sort_order) VALUES ($1,$2,$3,$4,$5)")
                    .bind(id).bind(name).bind(&d.description).bind(!d.hidden).bind(d.order)
                    .execute(&mut **tx).await?;
                rep.created("ticket_departments");
                id
            }
        };
        depts.insert(d.id, id);
    }
    let fallback_dept: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM ticket_departments ORDER BY sort_order, name LIMIT 1")
            .fetch_optional(&mut **tx)
            .await?;
    let mut tickets: HashMap<i64, Uuid> = HashMap::new();
    for t in &data.tickets {
        let sid = t.id.to_string();
        if let Some(id) = lookup(tx, "ticket", &sid).await? {
            tickets.insert(t.id, id);
            rep.skipped("tickets");
            continue;
        }
        let Some((cid, _)) = customers.get(&t.userid) else {
            rep.warnings.push(format!(
                "ticket {} (#{}) has no imported client (guest tickets are not supported), skipped",
                t.id, t.tid
            ));
            continue;
        };
        let Some(dept) = depts.get(&t.did).copied().or(fallback_dept) else {
            rep.warnings
                .push(format!("ticket {} has no department, skipped", t.id));
            continue;
        };
        let owner: Option<Uuid> =
            sqlx::query_scalar("SELECT owner_user_id FROM customers WHERE id=$1")
                .bind(cid)
                .fetch_one(&mut **tx)
                .await?;
        let status = map::ticket_status(&t.status);
        let subject = if t.title.trim().is_empty() {
            format!("Ticket #{}", t.tid)
        } else {
            map::clip(t.title.trim(), 200)
        };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO tickets (id, customer_id, opened_by, department_id, subject, status, priority, created_at, last_reply_at, closed_at) \
                     VALUES ($1,$2,$3,$4,$5,$6,$7,COALESCE($8, now()),COALESCE($9,$8, now()),$10)")
            .bind(id).bind(cid).bind(owner).bind(dept).bind(subject).bind(status).bind(map::ticket_priority(&t.urgency))
            .bind(ts(t.date)).bind(ts(t.lastreply))
            .bind(if status == "closed" { ts(t.lastreply.or(t.date)) } else { None })
            .execute(&mut **tx).await?;
        let body = if t.message.trim().is_empty() {
            "(empty)".to_string()
        } else {
            map::clip(&t.message, 20000)
        };
        sqlx::query("INSERT INTO ticket_messages (id, ticket_id, author_id, is_staff, body, created_at) VALUES ($1,$2,$3,false,$4,COALESCE($5, now()))")
            .bind(Uuid::new_v4()).bind(id).bind(owner).bind(body).bind(ts(t.date))
            .execute(&mut **tx).await?;
        remember(tx, "ticket", &sid, id).await?;
        rep.created("tickets");
        tickets.insert(t.id, id);
    }
    for r in &data.ticket_replies {
        let sid = r.id.to_string();
        if lookup(tx, "ticket_reply", &sid).await?.is_some() {
            rep.skipped("ticket_replies");
            continue;
        }
        let Some(tid) = tickets.get(&r.tid) else {
            continue;
        };
        let staff = !r.admin.trim().is_empty();
        let author: Option<Uuid> = if staff {
            None // WHMCS admins are not imported; the staff name is kept in the body.
        } else {
            sqlx::query_scalar("SELECT opened_by FROM tickets WHERE id=$1")
                .bind(tid)
                .fetch_one(&mut **tx)
                .await?
        };
        let mut body = if r.message.trim().is_empty() {
            "(empty)".to_string()
        } else {
            r.message.clone()
        };
        if staff {
            body = format!("[{}]\n{body}", r.admin.trim());
        }
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO ticket_messages (id, ticket_id, author_id, is_staff, body, created_at) VALUES ($1,$2,$3,$4,$5,COALESCE($6, now()))")
            .bind(id).bind(tid).bind(author).bind(staff).bind(map::clip(&body, 20000)).bind(ts(r.date))
            .execute(&mut **tx).await?;
        remember(tx, "ticket_reply", &sid, id).await?;
        rep.created("ticket_replies");
    }

    // ---- announcements
    for a in &data.announcements {
        let sid = a.id.to_string();
        if lookup(tx, "announcement", &sid).await?.is_some() {
            rep.skipped("announcements");
            continue;
        }
        let id = Uuid::new_v4();
        let title = if a.title.trim().is_empty() {
            format!("Announcement {}", a.id)
        } else {
            map::clip(a.title.trim(), 200)
        };
        sqlx::query("INSERT INTO announcements (id, title, body, is_published, published_at, created_at) VALUES ($1,$2,$3,$4,COALESCE($5, now()),COALESCE($5, now()))")
            .bind(id).bind(title).bind(map::clip(&a.announcement, 50000)).bind(a.published).bind(ts(a.date))
            .execute(&mut **tx).await?;
        remember(tx, "announcement", &sid, id).await?;
        rep.created("announcements");
    }

    // ---- knowledge base
    let mut kbcats: HashMap<i64, Uuid> = HashMap::new();
    for c in &data.kb_categories {
        let sid = c.id.to_string();
        if let Some(id) = lookup(tx, "kb_category", &sid).await? {
            kbcats.insert(c.id, id);
            rep.skipped("kb_categories");
            continue;
        }
        let id = Uuid::new_v4();
        let name = if c.name.trim().is_empty() {
            format!("Category {}", c.id)
        } else {
            map::clip(c.name.trim(), 120)
        };
        sqlx::query("INSERT INTO kb_categories (id, slug, name, description) VALUES ($1,$2,$3,$4)")
            .bind(id)
            .bind(map::slug(&c.name, &format!("{src}-kc"), c.id))
            .bind(name)
            .bind(&c.description)
            .execute(&mut **tx)
            .await?;
        remember(tx, "kb_category", &sid, id).await?;
        rep.created("kb_categories");
        kbcats.insert(c.id, id);
    }
    let needs_uncat = data
        .kb_articles
        .iter()
        .any(|a| !kbcats.contains_key(&a.categoryid));
    if needs_uncat && !data.kb_articles.is_empty() {
        let id = match lookup(tx, "kb_category", "uncategorized").await? {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4();
                sqlx::query("INSERT INTO kb_categories (id, slug, name) VALUES ($1,'whmcs-uncategorized','Imported') ON CONFLICT (slug) DO NOTHING")
                    .bind(id).execute(&mut **tx).await?;
                let id: Uuid = sqlx::query_scalar(
                    "SELECT id FROM kb_categories WHERE slug='whmcs-uncategorized'",
                )
                .fetch_one(&mut **tx)
                .await?;
                remember(tx, "kb_category", "uncategorized", id).await?;
                id
            }
        };
        kbcats.insert(-1, id);
    }
    for a in &data.kb_articles {
        let sid = a.id.to_string();
        if lookup(tx, "kb_article", &sid).await?.is_some() {
            rep.skipped("kb_articles");
            continue;
        }
        let Some(cat) = kbcats.get(&a.categoryid).or(kbcats.get(&-1)) else {
            continue;
        };
        let id = Uuid::new_v4();
        let title = if a.title.trim().is_empty() {
            format!("Article {}", a.id)
        } else {
            map::clip(a.title.trim(), 200)
        };
        if a.article.chars().count() > 100_000 {
            rep.warnings
                .push(format!("kb article {} truncated to 100000 chars", a.id));
        }
        sqlx::query("INSERT INTO kb_articles (id, category_id, slug, title, body, is_published, views) VALUES ($1,$2,$3,$4,$5,$6,$7)")
            .bind(id).bind(cat).bind(map::slug(&a.title, &format!("{src}-ka"), a.id)).bind(title)
            .bind(map::clip(&a.article, 100_000)).bind(!a.private).bind(a.views.max(0))
            .execute(&mut **tx).await?;
        remember(tx, "kb_article", &sid, id).await?;
        rep.created("kb_articles");
    }
    Ok(())
}

/// Domains, configurable options (with prices) and client credit balances.
async fn import_domains_options_credit(
    tx: &mut Tx<'_>,
    data: &WhmcsData,
    rep: &mut Report,
    cur: &HashMap<i64, String>,
    customers: &HashMap<i64, (Uuid, String)>,
    products: &HashMap<i64, Uuid>,
) -> Result<(), ImportError> {
    // ---- domains
    for d in &data.domains {
        let sid = d.id.to_string();
        if lookup(tx, "domain", &sid).await?.is_some() {
            rep.skipped("domains");
            continue;
        }
        let Some((cid, _)) = customers.get(&d.userid) else {
            rep.warnings.push(format!("domain {} belongs to unknown client {}, skipped", d.id, d.userid));
            continue;
        };
        let name = d.domain.trim().trim_end_matches('.').to_ascii_lowercase();
        let Some(tld) = map::domain_tld(&name) else {
            rep.warnings.push(format!("domain {} has invalid name {:?}, skipped", d.id, d.domain));
            continue;
        };
        let status = map::domain_status(&d.status);
        let taken: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM domains WHERE lower(name) = $1 AND status NOT IN ('cancelled','failed'))")
            .bind(&name).fetch_one(&mut **tx).await?;
        if taken && !matches!(status, "cancelled" | "failed") {
            rep.warnings.push(format!("domain {} ({name}) already exists in PeltyCMS, skipped", d.id));
            continue;
        }
        let kind = if d.type_.eq_ignore_ascii_case("transfer") { "transfer" } else { "register" };
        let registrar = map::clip(&d.registrar.trim().to_ascii_lowercase(), 60);
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO domains (id, customer_id, name, tld, kind, status, registrar, years, registered_on, expires_on, auto_renew, privacy, dns_management, admin_notes) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)")
            .bind(id).bind(cid).bind(&name).bind(&tld).bind(kind).bind(status)
            .bind(&registrar).bind(d.registrationperiod.clamp(1, 10)).bind(d.registrationdate).bind(d.expirydate)
            .bind(!d.donotrenew).bind(d.idprotection).bind(d.dnsmanagement)
            .bind(format!("Imported from WHMCS (domain #{})", d.id))
            .execute(&mut **tx).await?;
        remember(tx, "domain", &sid, id).await?;
        rep.created("domains");
    }

    // ---- configurable options: one copy per linked product
    let mut subs_by_opt: HashMap<i64, Vec<&crate::whmcs::ConfigSub>> = HashMap::new();
    for s in &data.config_subs {
        subs_by_opt.entry(s.configid).or_default().push(s);
    }
    for g in &data.config_groups {
        for pid in &g.product_ids {
            let Some(product) = products.get(pid) else {
                rep.warnings.push(format!("config group {} links unknown product {pid}", g.id));
                continue;
            };
            for o in data.config_options.iter().filter(|o| o.gid == g.id && !o.hidden) {
                let sid = format!("{}:{pid}", o.id);
                if lookup(tx, "config_option", &sid).await?.is_some() {
                    rep.skipped("config_options");
                    continue;
                }
                let name = o.optionname.split('|').next_back().unwrap_or("").trim();
                let name = if name.is_empty() { format!("Option {}", o.id) } else { map::clip(name, 120) };
                let otype = map::config_option_type(o.optiontype);
                let (qmin, qmax) = if otype == "quantity" {
                    let lo = o.qtyminimum.clamp(0, 100_000);
                    let hi = if o.qtymaximum <= 0 { 100 } else { o.qtymaximum.clamp(lo, 100_000) };
                    (lo, hi.max(lo))
                } else {
                    (0, 100)
                };
                let oid = Uuid::new_v4();
                sqlx::query("INSERT INTO product_config_options (id, product_id, name, required, sort_order, option_type, qty_min, qty_max) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                    .bind(oid).bind(product).bind(&name).bind(matches!(otype, "dropdown" | "radio")).bind(o.order).bind(otype).bind(qmin).bind(qmax)
                    .execute(&mut **tx).await?;
                remember(tx, "config_option", &sid, oid).await?;
                rep.created("config_options");
                let mut subs = subs_by_opt.get(&o.id).cloned().unwrap_or_default();
                subs.sort_by_key(|s| (s.sortorder, s.id));
                for sub in subs.into_iter().filter(|s| !s.hidden) {
                    let label = sub.optionname.split('|').next_back().unwrap_or("").trim();
                    let label = if label.is_empty() { format!("Choice {}", sub.id) } else { map::clip(label, 120) };
                    let chid = Uuid::new_v4();
                    sqlx::query("INSERT INTO product_config_choices (id, option_id, label, sort_order) VALUES ($1,$2,$3,$4)")
                        .bind(chid).bind(oid).bind(&label).bind(sub.sortorder)
                        .execute(&mut **tx).await?;
                    rep.created("config_choices");
                    for pr in data.config_pricing.iter().filter(|p| p.relid == sub.id) {
                        let Some(code) = cur.get(&pr.currency) else { continue };
                        for (cycle, amount, _setup) in pricing_rows(pr) {
                            if !map::price_enabled(amount) {
                                continue;
                            }
                            sqlx::query("INSERT INTO product_config_prices (choice_id, currency, cycle, amount) VALUES ($1,$2,$3,$4) ON CONFLICT DO NOTHING")
                                .bind(chid).bind(code).bind(cycle).bind(amount)
                                .execute(&mut **tx).await?;
                        }
                    }
                }
            }
        }
    }

    // ---- client credit balances -> one opening ledger entry
    for c in &data.clients {
        if c.credit <= Decimal::ZERO {
            continue;
        }
        let sid = c.id.to_string();
        if lookup(tx, "credit", &sid).await?.is_some() {
            rep.skipped("credit");
            continue;
        }
        let Some((cid, currency)) = customers.get(&c.id) else { continue };
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO credit_ledger (id, customer_id, currency, amount, kind, note) VALUES ($1,$2,$3,$4,'adjustment',$5)")
            .bind(id).bind(cid).bind(currency).bind(c.credit.round_dp(2)).bind("Opening credit balance imported from WHMCS")
            .execute(&mut **tx).await?;
        remember(tx, "credit", &sid, id).await?;
        rep.created("credit");
    }
    Ok(())
}
