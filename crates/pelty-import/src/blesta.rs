//! Blesta -> PeltyCMS. Reads a JSON export whose top-level keys are Blesta
//! table names (`clients`, `contacts`, `packages`, `pricings`, ...) holding
//! arrays of rows with Blesta's column names, and converts it to the
//! WHMCS-shaped model consumed by [`crate::import::run_source`].
//!
//! Produce the export with `scripts/export-blesta.sql` (MySQL `JSON_ARRAYAGG`)
//! or any tool that dumps those tables to JSON. Ids may be numbers or strings.
use crate::foreign::{self, dec, int, text, Currencies};
use crate::map::parse_date;
use crate::whmcs::*;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::collections::HashMap;

macro_rules! row {
    ($name:ident { $($f:ident : $t:ty = $de:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Default, Deserialize)]
        #[serde(default)]
        pub struct $name { $( #[serde(deserialize_with = $de)] pub $f: $t, )* }
    };
}

row!(BCurrency {
    code: String = "text"
});
row!(BClientGroup {
    id: i64 = "int",
    name: String = "text"
});
row!(BClient {
    id: i64 = "int",
    client_group_id: i64 = "int",
    status: String = "text"
});
row!(BContact {
    client_id: i64 = "int",
    contact_type: String = "text",
    first_name: String = "text",
    last_name: String = "text",
    company: String = "text",
    email: String = "text",
    address1: String = "text",
    address2: String = "text",
    city: String = "text",
    state: String = "text",
    zip: String = "text",
    country: String = "text",
});
row!(BPackageGroup {
    id: i64 = "int",
    name: String = "text"
});
row!(BPackageGroupLink {
    package_id: i64 = "int",
    package_group_id: i64 = "int"
});
row!(BPackage {
    id: i64 = "int",
    name: String = "text",
    description: String = "text",
    status: String = "text",
    module: String = "text",
});
row!(BPackagePricing {
    id: i64 = "int",
    package_id: i64 = "int",
    pricing_id: i64 = "int"
});
row!(BPricing {
    id: i64 = "int",
    term: i64 = "int",
    period: String = "text",
    price: Decimal = "dec",
    setup_fee: Decimal = "dec",
    currency: String = "text",
});
row!(BService {
    id: i64 = "int",
    client_id: i64 = "int",
    pricing_id: i64 = "int",
    status: String = "text",
    date_added: String = "text",
    date_renews: String = "text",
    override_price: String = "text",
    name: String = "text",
});
row!(BInvoice {
    id: i64 = "int",
    id_value: String = "text",
    client_id: i64 = "int",
    date_billed: String = "text",
    date_due: String = "text",
    date_closed: String = "text",
    subtotal: Decimal = "dec",
    total: Decimal = "dec",
    status: String = "text",
    currency: String = "text",
});
row!(BInvoiceLine {
    id: i64 = "int",
    invoice_id: i64 = "int",
    service_id: i64 = "int",
    description: String = "text",
    qty: Decimal = "dec",
    amount: Decimal = "dec",
});
row!(BTransaction {
    id: i64 = "int",
    client_id: i64 = "int",
    amount: Decimal = "dec",
    currency: String = "text",
    gateway: String = "text",
    transaction_id: String = "text",
    status: String = "text",
    date_added: String = "text",
});
row!(BApplied {
    transaction_id: i64 = "int",
    invoice_id: i64 = "int"
});
row!(BDepartment {
    id: i64 = "int",
    name: String = "text",
    description: String = "text"
});
row!(BTicket {
    id: i64 = "int",
    code: String = "text",
    department_id: i64 = "int",
    client_id: i64 = "int",
    summary: String = "text",
    status: String = "text",
    priority: String = "text",
    date_added: String = "text",
});
row!(BReply {
    id: i64 = "int",
    ticket_id: i64 = "int",
    staff_id: i64 = "int",
    details: String = "text",
    date_added: String = "text",
    r#type: String = "text",
});

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BlestaExport {
    pub currencies: Vec<BCurrency>,
    pub client_groups: Vec<BClientGroup>,
    pub clients: Vec<BClient>,
    pub contacts: Vec<BContact>,
    pub package_groups: Vec<BPackageGroup>,
    pub package_group: Vec<BPackageGroupLink>,
    pub packages: Vec<BPackage>,
    pub package_pricing: Vec<BPackagePricing>,
    pub pricings: Vec<BPricing>,
    pub services: Vec<BService>,
    pub invoices: Vec<BInvoice>,
    pub invoice_lines: Vec<BInvoiceLine>,
    pub transactions: Vec<BTransaction>,
    pub transaction_applied: Vec<BApplied>,
    pub support_departments: Vec<BDepartment>,
    pub support_tickets: Vec<BTicket>,
    pub support_replies: Vec<BReply>,
}

pub fn parse(json: &str) -> Result<BlestaExport, serde_json::Error> {
    serde_json::from_str(json)
}

fn ticket_status(s: &str) -> &'static str {
    match s {
        "awaiting_reply" => "Answered",
        "on_hold" => "On Hold",
        "closed" => "Closed",
        _ => "Open",
    }
}

/// Converts the export. Returns the model plus conversion warnings.
pub fn convert(b: &BlestaExport) -> (WhmcsData, Vec<String>) {
    let mut w = WhmcsData::default();
    let mut warn = Vec::new();
    let mut cur = Currencies::default();
    for c in &b.currencies {
        cur.id(&c.code);
    }

    for g in &b.client_groups {
        w.client_groups.push(ClientGroup {
            id: g.id,
            groupname: g.name.clone(),
            ..Default::default()
        });
    }
    let primary: HashMap<i64, &BContact> = b
        .contacts
        .iter()
        .filter(|c| c.contact_type.is_empty() || c.contact_type == "primary")
        .map(|c| (c.client_id, c))
        .collect();
    // A client's currency = currency of its first invoice, else the default.
    let mut client_cur: HashMap<i64, String> = HashMap::new();
    for i in &b.invoices {
        client_cur
            .entry(i.client_id)
            .or_insert_with(|| i.currency.clone());
    }
    for c in &b.clients {
        let Some(k) = primary.get(&c.id) else {
            warn.push(format!(
                "blesta client {} has no primary contact, skipped",
                c.id
            ));
            continue;
        };
        w.clients.push(Client {
            id: c.id,
            firstname: k.first_name.clone(),
            lastname: k.last_name.clone(),
            companyname: k.company.clone(),
            email: k.email.clone(),
            address1: k.address1.clone(),
            address2: k.address2.clone(),
            city: k.city.clone(),
            state: k.state.clone(),
            postcode: k.zip.clone(),
            country: k.country.clone(),
            currency: cur.id(client_cur.get(&c.id).map(String::as_str).unwrap_or("")),
            status: if c.status == "active" {
                "Active".into()
            } else {
                "Inactive".into()
            },
            groupid: c.client_group_id,
            ..Default::default()
        });
    }

    for g in &b.package_groups {
        w.product_groups.push(ProductGroup {
            id: g.id,
            name: g.name.clone(),
            hidden: false,
            order: 0,
        });
    }
    let pkg_group: HashMap<i64, i64> = b
        .package_group
        .iter()
        .map(|l| (l.package_id, l.package_group_id))
        .collect();
    let default_group = if b.packages.iter().any(|p| !pkg_group.contains_key(&p.id)) {
        let id = b.package_groups.iter().map(|g| g.id).max().unwrap_or(0) + 1;
        w.product_groups.push(ProductGroup {
            id,
            name: "Blesta".into(),
            hidden: false,
            order: 0,
        });
        id
    } else {
        0
    };
    for p in &b.packages {
        w.products.push(Product {
            id: p.id,
            gid: pkg_group.get(&p.id).copied().unwrap_or(default_group),
            name: p.name.clone(),
            description: p.description.clone(),
            hidden: p.status == "restricted",
            retired: p.status == "inactive",
            servertype: p.module.clone(),
            order: 0,
        });
    }
    let pricings: HashMap<i64, &BPricing> = b.pricings.iter().map(|p| (p.id, p)).collect();
    // package_pricing.id -> (package_id, pricing)
    let mut pp: HashMap<i64, (i64, &BPricing)> = HashMap::new();
    let mut rows: HashMap<(i64, i64), Pricing> = HashMap::new();
    for l in &b.package_pricing {
        let Some(p) = pricings.get(&l.pricing_id) else {
            continue;
        };
        pp.insert(l.id, (l.package_id, p));
        let Some(cy) = foreign::cycle(&p.period, p.term) else {
            warn.push(format!(
                "blesta pricing {}: unsupported period {} {}, skipped",
                p.id, p.term, p.period
            ));
            continue;
        };
        let cid = cur.id(&p.currency);
        let row = rows
            .entry((l.package_id, cid))
            .or_insert_with(|| foreign::disabled_pricing(l.package_id, cid));
        foreign::set_price(row, cy, p.price, p.setup_fee);
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by_key(|r| (r.relid, r.currency));
    w.pricing = rows;

    for s in &b.services {
        let Some((pkg, p)) = pp.get(&s.pricing_id) else {
            warn.push(format!(
                "blesta service {}: unknown pricing {}, skipped",
                s.id, s.pricing_id
            ));
            continue;
        };
        let amount = s
            .override_price
            .trim()
            .parse::<Decimal>()
            .unwrap_or(p.price);
        w.hosting.push(Hosting {
            id: s.id,
            userid: s.client_id,
            packageid: *pkg,
            regdate: parse_date(&s.date_added),
            domain: s.name.clone(),
            billingcycle: foreign::cycle(&p.period, p.term)
                .unwrap_or("One Time")
                .into(),
            nextduedate: parse_date(&s.date_renews),
            amount,
            domainstatus: match s.status.as_str() {
                "active" => "Active",
                "suspended" => "Suspended",
                "canceled" => "Cancelled",
                _ => "Pending",
            }
            .into(),
            ..Default::default()
        });
    }

    for i in &b.invoices {
        let status = match i.status.as_str() {
            "void" => "Cancelled",
            "draft" | "proforma" if i.date_closed.is_empty() => "Draft",
            _ if !i.date_closed.is_empty() => "Paid",
            _ => "Unpaid",
        };
        w.invoices.push(Invoice {
            id: i.id,
            userid: i.client_id,
            invoicenum: i.id_value.clone(),
            date: parse_date(&i.date_billed),
            duedate: parse_date(&i.date_due),
            datepaid: parse_date(&i.date_closed),
            subtotal: i.subtotal,
            tax: i.total - i.subtotal,
            total: i.total,
            status: status.into(),
            ..Default::default()
        });
    }
    for l in &b.invoice_lines {
        let qty = if l.qty.is_zero() { Decimal::ONE } else { l.qty };
        w.invoice_items.push(InvoiceItem {
            id: l.id,
            invoiceid: l.invoice_id,
            type_: if l.service_id > 0 {
                "Hosting".into()
            } else {
                String::new()
            },
            relid: l.service_id,
            description: l.description.clone(),
            amount: (l.amount * qty).round_dp(2),
            taxed: false,
        });
    }
    let applied: HashMap<i64, i64> = b
        .transaction_applied
        .iter()
        .map(|a| (a.transaction_id, a.invoice_id))
        .collect();
    for t in &b.transactions {
        let (amountin, amountout) = match t.status.as_str() {
            "approved" => (t.amount, Decimal::ZERO),
            "refunded" => (Decimal::ZERO, t.amount),
            _ => continue, // pending, declined, void, error
        };
        w.transactions.push(Transaction {
            id: t.id,
            userid: t.client_id,
            currency: cur.id(&t.currency),
            gateway: t.gateway.clone(),
            date: parse_date(&t.date_added),
            amountin,
            amountout,
            transid: t.transaction_id.clone(),
            invoiceid: applied.get(&t.id).copied().unwrap_or(0),
        });
    }

    for d in &b.support_departments {
        w.ticket_departments.push(TicketDepartment {
            id: d.id,
            name: d.name.clone(),
            description: d.description.clone(),
            hidden: false,
            order: 0,
        });
    }
    let mut replies: HashMap<i64, Vec<&BReply>> = HashMap::new();
    for r in b
        .support_replies
        .iter()
        .filter(|r| r.r#type.is_empty() || r.r#type == "reply")
    {
        replies.entry(r.ticket_id).or_default().push(r);
    }
    for t in &b.support_tickets {
        let mut rs = replies.remove(&t.id).unwrap_or_default();
        rs.sort_by_key(|r| (r.date_added.clone(), r.id));
        let first = if rs.is_empty() {
            String::new()
        } else {
            rs.remove(0).details.clone()
        };
        w.tickets.push(Ticket {
            id: t.id,
            tid: t.code.clone(),
            did: t.department_id,
            userid: t.client_id,
            title: t.summary.clone(),
            message: first,
            status: ticket_status(&t.status).into(),
            urgency: t.priority.clone(),
            date: parse_date(&t.date_added),
            lastreply: rs.last().and_then(|r| parse_date(&r.date_added)),
        });
        for r in rs {
            w.ticket_replies.push(TicketReply {
                id: r.id,
                tid: t.id,
                admin: if r.staff_id > 0 {
                    format!("staff #{}", r.staff_id)
                } else {
                    String::new()
                },
                date: parse_date(&r.date_added),
                message: r.details.clone(),
            });
        }
    }

    if cur.rows.is_empty() {
        cur.id("EUR");
    }
    w.currencies = cur.rows;
    (w, warn)
}
