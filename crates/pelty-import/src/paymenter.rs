//! Paymenter (v1, Laravel) -> PeltyCMS. Reads a JSON export whose top-level
//! keys are Paymenter table names (`users`, `categories`, `products`, `plans`,
//! `prices`, `services`, `invoices`, `invoice_items`, `invoice_transactions`,
//! `gateways`, `tickets`, `ticket_messages`, `credits`) and converts it to the
//! WHMCS-shaped model consumed by [`crate::import::run_source`].
use crate::foreign::{self, dec, flag, int, text, Currencies};
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

row!(PCurrency { code: String = "text" });
row!(PUser {
    id: i64 = "int", first_name: String = "text", last_name: String = "text",
    email: String = "text", role_id: i64 = "int",
});
row!(PCredit { user_id: i64 = "int", currency_code: String = "text", amount: Decimal = "dec" });
row!(PCategory { id: i64 = "int", name: String = "text", sort: i64 = "int" });
row!(PProduct {
    id: i64 = "int", category_id: i64 = "int", name: String = "text", description: String = "text",
    hidden: bool = "flag", sort: i64 = "int",
});
row!(PPlan {
    id: i64 = "int", priceable_id: i64 = "int", priceable_type: String = "text", r#type: String = "text",
    billing_period: i64 = "int", billing_unit: String = "text",
});
row!(PPrice { plan_id: i64 = "int", currency_code: String = "text", price: Decimal = "dec", setup_fee: Decimal = "dec" });
row!(PService {
    id: i64 = "int", user_id: i64 = "int", product_id: i64 = "int", plan_id: i64 = "int",
    status: String = "text", price: Decimal = "dec", currency_code: String = "text",
    expires_at: String = "text", created_at: String = "text",
});
row!(PInvoice {
    id: i64 = "int", number: String = "text", user_id: i64 = "int", status: String = "text",
    currency_code: String = "text", due_at: String = "text", created_at: String = "text",
});
row!(PInvoiceItem {
    id: i64 = "int", invoice_id: i64 = "int", description: String = "text", price: Decimal = "dec",
    quantity: Decimal = "dec", reference_id: i64 = "int", reference_type: String = "text",
});
row!(PTransaction {
    id: i64 = "int", invoice_id: i64 = "int", gateway_id: i64 = "int", amount: Decimal = "dec",
    fee: Decimal = "dec", transaction_id: String = "text", status: String = "text", created_at: String = "text",
});
row!(PGateway { id: i64 = "int", name: String = "text", extension: String = "text" });
row!(PTicket {
    id: i64 = "int", user_id: i64 = "int", subject: String = "text", status: String = "text",
    priority: String = "text", department: String = "text", created_at: String = "text",
});
row!(PMessage { id: i64 = "int", ticket_id: i64 = "int", user_id: i64 = "int", message: String = "text", created_at: String = "text" });

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PaymenterExport {
    pub currencies: Vec<PCurrency>,
    pub users: Vec<PUser>,
    pub credits: Vec<PCredit>,
    pub categories: Vec<PCategory>,
    pub products: Vec<PProduct>,
    pub plans: Vec<PPlan>,
    pub prices: Vec<PPrice>,
    pub services: Vec<PService>,
    pub invoices: Vec<PInvoice>,
    pub invoice_items: Vec<PInvoiceItem>,
    pub invoice_transactions: Vec<PTransaction>,
    pub gateways: Vec<PGateway>,
    pub tickets: Vec<PTicket>,
    pub ticket_messages: Vec<PMessage>,
}

pub fn parse(json: &str) -> Result<PaymenterExport, serde_json::Error> {
    serde_json::from_str(json)
}

fn plan_cycle(p: &PPlan) -> Option<&'static str> {
    match p.r#type.as_str() {
        "free" => Some("Free Account"),
        "one-time" => Some("One Time"),
        _ => foreign::cycle(&p.billing_unit, p.billing_period),
    }
}

pub fn convert(x: &PaymenterExport) -> (WhmcsData, Vec<String>) {
    let mut w = WhmcsData::default();
    let mut warn = Vec::new();
    let mut cur = Currencies::default();
    for c in &x.currencies {
        cur.id(&c.code);
    }
    let mut credit: HashMap<i64, &PCredit> = HashMap::new();
    for c in &x.credits {
        credit.entry(c.user_id).or_insert(c);
    }
    let mut user_cur: HashMap<i64, String> = HashMap::new();
    for s in &x.services {
        user_cur.entry(s.user_id).or_insert_with(|| s.currency_code.clone());
    }
    for i in &x.invoices {
        user_cur.entry(i.user_id).or_insert_with(|| i.currency_code.clone());
    }
    for u in &x.users {
        if u.role_id > 0 {
            continue; // staff/admin accounts are not clients
        }
        let cr = credit.get(&u.id);
        let code = cr.map(|c| c.currency_code.as_str()).or(user_cur.get(&u.id).map(String::as_str)).unwrap_or("");
        w.clients.push(Client {
            id: u.id,
            firstname: u.first_name.clone(),
            lastname: u.last_name.clone(),
            email: u.email.clone(),
            currency: cur.id(code),
            status: "Active".into(),
            credit: cr.map(|c| c.amount).unwrap_or_default(),
            ..Default::default()
        });
    }
    let staff: std::collections::HashSet<i64> = x.users.iter().filter(|u| u.role_id > 0).map(|u| u.id).collect();

    for c in &x.categories {
        w.product_groups.push(ProductGroup { id: c.id, name: c.name.clone(), hidden: false, order: c.sort as i32 });
    }
    for p in &x.products {
        w.products.push(Product {
            id: p.id,
            gid: p.category_id,
            name: p.name.clone(),
            description: p.description.clone(),
            hidden: p.hidden,
            retired: false,
            servertype: String::new(),
            order: p.sort as i32,
        });
    }
    let plans: HashMap<i64, &PPlan> = x
        .plans
        .iter()
        .filter(|p| p.priceable_type.is_empty() || p.priceable_type.ends_with("Product"))
        .map(|p| (p.id, p))
        .collect();
    let mut rows: HashMap<(i64, i64), Pricing> = HashMap::new();
    for pr in &x.prices {
        let Some(plan) = plans.get(&pr.plan_id) else { continue };
        let Some(cy) = plan_cycle(plan) else {
            warn.push(format!("paymenter plan {}: unsupported period {} {}, skipped", plan.id, plan.billing_period, plan.billing_unit));
            continue;
        };
        let cid = cur.id(&pr.currency_code);
        let row = rows.entry((plan.priceable_id, cid)).or_insert_with(|| foreign::disabled_pricing(plan.priceable_id, cid));
        foreign::set_price(row, cy, pr.price, pr.setup_fee);
    }
    let mut rows: Vec<_> = rows.into_values().collect();
    rows.sort_by_key(|r| (r.relid, r.currency));
    w.pricing = rows;

    for s in &x.services {
        let cycle = plans.get(&s.plan_id).and_then(|p| plan_cycle(p)).unwrap_or("Monthly");
        w.hosting.push(Hosting {
            id: s.id,
            userid: s.user_id,
            packageid: s.product_id,
            regdate: parse_date(&s.created_at),
            domain: String::new(),
            billingcycle: cycle.into(),
            nextduedate: parse_date(&s.expires_at),
            amount: s.price,
            domainstatus: match s.status.as_str() {
                "active" => "Active",
                "suspended" => "Suspended",
                "cancelled" => "Cancelled",
                _ => "Pending",
            }
            .into(),
            ..Default::default()
        });
    }

    let mut totals: HashMap<i64, Decimal> = HashMap::new();
    for l in &x.invoice_items {
        let qty = if l.quantity.is_zero() { Decimal::ONE } else { l.quantity };
        let amount = (l.price * qty).round_dp(2);
        *totals.entry(l.invoice_id).or_default() += amount;
        let svc = l.reference_type.ends_with("Service");
        w.invoice_items.push(InvoiceItem {
            id: l.id,
            invoiceid: l.invoice_id,
            type_: if svc { "Hosting".into() } else { String::new() },
            relid: if svc { l.reference_id } else { 0 },
            description: l.description.clone(),
            amount,
            taxed: false,
        });
    }
    let mut paid_at: HashMap<i64, String> = HashMap::new();
    let gateways: HashMap<i64, &PGateway> = x.gateways.iter().map(|g| (g.id, g)).collect();
    let inv_user: HashMap<i64, (i64, &str)> = x.invoices.iter().map(|i| (i.id, (i.user_id, i.currency_code.as_str()))).collect();
    for t in &x.invoice_transactions {
        if !(t.status.is_empty() || t.status == "succeeded") {
            continue;
        }
        let Some((uid, code)) = inv_user.get(&t.invoice_id) else {
            warn.push(format!("paymenter transaction {}: unknown invoice {}, skipped", t.id, t.invoice_id));
            continue;
        };
        paid_at.entry(t.invoice_id).or_insert_with(|| t.created_at.clone());
        let g = gateways.get(&t.gateway_id).map(|g| if g.extension.is_empty() { g.name.clone() } else { g.extension.clone() });
        w.transactions.push(Transaction {
            id: t.id,
            userid: *uid,
            currency: cur.id(code),
            gateway: g.unwrap_or_default(),
            date: parse_date(&t.created_at),
            amountin: t.amount,
            amountout: Decimal::ZERO,
            transid: t.transaction_id.clone(),
            invoiceid: t.invoice_id,
        });
    }
    for i in &x.invoices {
        let total = totals.get(&i.id).copied().unwrap_or_default();
        let status = match i.status.as_str() {
            "paid" => "Paid",
            "cancelled" => "Cancelled",
            _ => "Unpaid",
        };
        w.invoices.push(Invoice {
            id: i.id,
            userid: i.user_id,
            invoicenum: i.number.clone(),
            date: parse_date(&i.created_at),
            duedate: parse_date(&i.due_at),
            datepaid: if status == "Paid" { paid_at.get(&i.id).and_then(|d| parse_date(d)).or(parse_date(&i.created_at)) } else { None },
            subtotal: total,
            total,
            status: status.into(),
            ..Default::default()
        });
    }

    // Paymenter departments are free-text strings on the ticket.
    let mut depts: HashMap<String, i64> = HashMap::new();
    let mut msgs: HashMap<i64, Vec<&PMessage>> = HashMap::new();
    for m in &x.ticket_messages {
        msgs.entry(m.ticket_id).or_default().push(m);
    }
    for t in &x.tickets {
        let dname = if t.department.trim().is_empty() { "Support".to_string() } else { t.department.trim().to_string() };
        let next = depts.len() as i64 + 1;
        let did = *depts.entry(dname.clone()).or_insert_with(|| {
            w.ticket_departments.push(TicketDepartment { id: next, name: dname.clone(), ..Default::default() });
            next
        });
        let mut ms = msgs.remove(&t.id).unwrap_or_default();
        ms.sort_by_key(|m| (m.created_at.clone(), m.id));
        let first = if ms.is_empty() { String::new() } else { ms.remove(0).message.clone() };
        w.tickets.push(Ticket {
            id: t.id,
            tid: format!("P{}", t.id),
            did,
            userid: t.user_id,
            title: t.subject.clone(),
            message: first,
            status: match t.status.as_str() {
                "closed" => "Closed",
                "replied" => "Answered",
                _ => "Open",
            }
            .into(),
            urgency: t.priority.clone(),
            date: parse_date(&t.created_at),
            lastreply: ms.last().and_then(|m| parse_date(&m.created_at)),
        });
        for m in ms {
            w.ticket_replies.push(TicketReply {
                id: m.id,
                tid: t.id,
                admin: if m.user_id != t.user_id || staff.contains(&m.user_id) { format!("staff #{}", m.user_id) } else { String::new() },
                date: parse_date(&m.created_at),
                message: m.message.clone(),
            });
        }
    }

    if cur.rows.is_empty() {
        cur.id("EUR");
    }
    w.currencies = cur.rows;
    (w, warn)
}
