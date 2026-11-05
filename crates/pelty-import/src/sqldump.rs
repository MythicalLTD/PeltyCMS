//! Reads a WHMCS database from a plain SQL export (`mysqldump whmcs > whmcs.sql`,
//! phpMyAdmin "Export > SQL", or a hosting panel backup), so an import needs
//! no running MySQL. Only `CREATE TABLE` (for column order) and
//! `INSERT|REPLACE [IGNORE] INTO ... VALUES (...), (...)` statements are read;
//! everything else (`SET`, `LOCK TABLES`, comments, indexes) is skipped.
//! Values keep MySQL's text form; `NULL` is `None`. `--hex-blob` values
//! (`0x...`) are decoded to text.
use std::collections::HashMap;

/// One row: column name -> value (`None` for SQL NULL).
pub type Row = HashMap<String, Option<String>>;

/// Table name -> rows, in file order.
#[derive(Debug, Default, Clone)]
pub struct Tables(pub HashMap<String, Vec<Row>>);

impl Tables {
    pub fn rows(&self, table: &str) -> &[Row] {
        self.0.get(table).map(Vec::as_slice).unwrap_or(&[])
    }
    pub fn has(&self, table: &str) -> bool {
        self.0.contains_key(table)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Word(String),
    Punct(char),
}

/// Split the dump into statements (on `;` outside strings and comments) and
/// tokenize each one.
fn statements(sql: &str) -> Vec<Vec<Tok>> {
    let mut out = vec![];
    let mut cur: Vec<Tok> = vec![];
    let b: Vec<char> = sql.chars().collect();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            '-' if b.get(i + 1) == Some(&'-') && b.get(i + 2).is_none_or(|x| x.is_whitespace()) => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '#' => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' if b.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
                continue;
            }
            '\'' | '"' => {
                let q = c;
                let mut s = String::new();
                i += 1;
                while i < b.len() {
                    let x = b[i];
                    if x == '\\' && i + 1 < b.len() {
                        i += 1;
                        s.push(match b[i] {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            '0' => '\0',
                            'Z' => '\x1a',
                            'b' => '\x08',
                            o => o,
                        });
                    } else if x == q && b.get(i + 1) == Some(&q) {
                        s.push(q);
                        i += 1;
                    } else if x == q {
                        break;
                    } else {
                        s.push(x);
                    }
                    i += 1;
                }
                cur.push(Tok::Str(s));
            }
            '`' => {
                let mut s = String::new();
                i += 1;
                while i < b.len() && b[i] != '`' {
                    s.push(b[i]);
                    i += 1;
                }
                cur.push(Tok::Ident(s));
            }
            ';' => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c if c.is_whitespace() => {}
            '(' | ')' | ',' | '=' => cur.push(Tok::Punct(c)),
            _ => {
                let mut s = String::new();
                while i < b.len() && !b[i].is_whitespace() && !"(),;=`'\"".contains(b[i]) {
                    s.push(b[i]);
                    i += 1;
                }
                if s.is_empty() {
                    s.push(c);
                    i += 1;
                }
                cur.push(Tok::Word(s));
                continue;
            }
        }
        i += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn word(t: Option<&Tok>, w: &str) -> bool {
    matches!(t, Some(Tok::Word(x)) if x.eq_ignore_ascii_case(w))
}

fn name(t: Option<&Tok>) -> Option<String> {
    match t? {
        Tok::Ident(s) | Tok::Word(s) => Some(s.rsplit('.').next().unwrap_or(s).trim_matches('`').to_string()),
        _ => None,
    }
}

fn value(t: &Tok) -> Option<String> {
    match t {
        Tok::Str(s) => Some(s.clone()),
        Tok::Word(w) if w.eq_ignore_ascii_case("NULL") => None,
        Tok::Word(w) if w.len() > 2 && (w.starts_with("0x") || w.starts_with("0X")) => {
            let bytes: Option<Vec<u8>> = (2..w.len()).step_by(2).map(|i| w.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok())).collect();
            Some(bytes.map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_else(|| w.clone()))
        }
        Tok::Word(w) | Tok::Ident(w) => Some(w.clone()),
        Tok::Punct(c) => Some(c.to_string()),
    }
}

/// Parse a SQL export. Errors only on INSERTs whose value count does not
/// match the columns (a truncated or foreign file).
pub fn parse(sql: &str) -> Result<Tables, String> {
    let mut cols: HashMap<String, Vec<String>> = HashMap::new();
    let mut t = Tables::default();
    for st in statements(sql) {
        let mut i = 0;
        if word(st.first(), "CREATE") {
            while i < st.len() && !word(st.get(i), "TABLE") {
                i += 1;
            }
            i += 1;
            while word(st.get(i), "IF") || word(st.get(i), "NOT") || word(st.get(i), "EXISTS") {
                i += 1;
            }
            let Some(tbl) = name(st.get(i)) else { continue };
            // Column definitions: an identifier right after `(` or `,` at depth 1.
            let mut depth = 0;
            let mut list = vec![];
            for k in i + 1..st.len() {
                match &st[k] {
                    Tok::Punct('(') => depth += 1,
                    Tok::Punct(')') => depth -= 1,
                    Tok::Ident(c) if depth == 1 && matches!(st.get(k - 1), Some(Tok::Punct('(' | ','))) => list.push(c.clone()),
                    _ => {}
                }
            }
            cols.insert(tbl, list);
            continue;
        }
        if !(word(st.first(), "INSERT") || word(st.first(), "REPLACE")) {
            continue;
        }
        i = 1;
        while word(st.get(i), "IGNORE") || word(st.get(i), "INTO") || word(st.get(i), "LOW_PRIORITY") || word(st.get(i), "DELAYED") {
            i += 1;
        }
        let Some(tbl) = name(st.get(i)) else { continue };
        i += 1;
        let mut names = cols.get(&tbl).cloned().unwrap_or_default();
        if st.get(i) == Some(&Tok::Punct('(')) {
            names.clear();
            i += 1;
            while i < st.len() && st[i] != Tok::Punct(')') {
                if let Some(n) = name(st.get(i)).filter(|_| st[i] != Tok::Punct(',')) {
                    names.push(n);
                }
                i += 1;
            }
            i += 1;
        }
        if !(word(st.get(i), "VALUES") || word(st.get(i), "VALUE")) {
            continue;
        }
        i += 1;
        let rows = t.0.entry(tbl.clone()).or_default();
        while i < st.len() {
            if st[i] != Tok::Punct('(') {
                i += 1;
                continue;
            }
            i += 1;
            let mut vals = vec![];
            let mut neg = false;
            while i < st.len() && st[i] != Tok::Punct(')') {
                match &st[i] {
                    Tok::Punct(',') => {}
                    Tok::Word(w) if w == "-" => neg = true,
                    tok => {
                        let v = value(tok).map(|v| if neg { format!("-{v}") } else { v });
                        neg = false;
                        vals.push(v);
                    }
                }
                i += 1;
            }
            i += 1;
            if names.is_empty() {
                return Err(format!("{tbl}: INSERT without a column list and no CREATE TABLE before it"));
            }
            if vals.len() != names.len() {
                return Err(format!("{tbl}: a row has {} values for {} columns", vals.len(), names.len()));
            }
            rows.push(names.iter().cloned().zip(vals).collect());
        }
    }
    Ok(t)
}

use crate::whmcs::*;
use rust_decimal::Decimal;
use std::str::FromStr;

fn s(r: &Row, c: &str) -> String {
    r.get(c).cloned().flatten().unwrap_or_default()
}
fn i(r: &Row, c: &str) -> i64 {
    let v = s(r, c);
    v.trim().parse().ok().or_else(|| v.trim().parse::<f64>().ok().map(|f| f as i64)).unwrap_or_default()
}
fn d(r: &Row, c: &str) -> Decimal {
    Decimal::from_str(s(r, c).trim()).unwrap_or_default()
}
fn dt(r: &Row, c: &str) -> Option<time::Date> {
    // DATETIME columns: keep the date part; '0000-00-00' -> None.
    crate::map::parse_date(s(r, c).get(..10).unwrap_or(""))
}
fn on(r: &Row, c: &str) -> bool {
    matches!(s(r, c).trim(), "on" | "1" | "yes")
}

fn pricing(t: &Tables, kind: &str) -> Vec<Pricing> {
    t.rows("tblpricing").iter().filter(|r| s(r, "type") == kind).map(|r| Pricing {
        relid: i(r, "relid"), currency: i(r, "currency"),
        msetupfee: d(r, "msetupfee"), qsetupfee: d(r, "qsetupfee"), ssetupfee: d(r, "ssetupfee"),
        asetupfee: d(r, "asetupfee"), bsetupfee: d(r, "bsetupfee"), tsetupfee: d(r, "tsetupfee"),
        monthly: d(r, "monthly"), quarterly: d(r, "quarterly"), semiannually: d(r, "semiannually"),
        annually: d(r, "annually"), biennially: d(r, "biennially"), triennially: d(r, "triennially"),
    }).collect()
}

impl WhmcsData {
    /// Build the import model from a parsed SQL export (same columns the
    /// MySQL loader selects). Missing tables are simply empty.
    pub fn from_tables(t: &Tables) -> WhmcsData {
        let mut o = WhmcsData {
            currencies: t.rows("tblcurrencies").iter().map(|r| Currency { id: i(r, "id"), code: s(r, "code"), default: i(r, "default") == 1 }).collect(),
            ..WhmcsData::default()
        };
        o.clients = t.rows("tblclients").iter().map(|r| Client {
            id: i(r, "id"), firstname: s(r, "firstname"), lastname: s(r, "lastname"), companyname: s(r, "companyname"),
            email: s(r, "email"), address1: s(r, "address1"), address2: s(r, "address2"), city: s(r, "city"), state: s(r, "state"),
            postcode: s(r, "postcode"), country: s(r, "country"), phonenumber: s(r, "phonenumber"), tax_id: s(r, "tax_id"),
            currency: i(r, "currency"), status: s(r, "status"), groupid: i(r, "groupid"), credit: d(r, "credit"), password: s(r, "password"),
        }).collect();
        // WHMCS 8+: logins live in tblusers, matched to clients by email.
        let users: HashMap<String, String> = t.rows("tblusers").iter().map(|r| (s(r, "email").trim().to_lowercase(), s(r, "password"))).collect();
        for c in &mut o.clients {
            if let Some(h) = users.get(&c.email.trim().to_lowercase()).filter(|h| !h.is_empty()) {
                c.password = h.clone();
            }
        }
        o.contacts = t.rows("tblcontacts").iter().map(|r| Contact {
            id: i(r, "id"), userid: i(r, "userid"), firstname: s(r, "firstname"), lastname: s(r, "lastname"),
            email: s(r, "email"), phonenumber: s(r, "phonenumber"), billing: i(r, "invoiceemails") == 1,
        }).collect();
        o.client_groups = t.rows("tblclientgroups").iter().map(|r| ClientGroup { id: i(r, "id"), groupname: s(r, "groupname"), discountpercent: d(r, "discountpercent"), susptermexempt: on(r, "susptermexempt") }).collect();
        o.product_groups = t.rows("tblproductgroups").iter().map(|r| ProductGroup { id: i(r, "id"), name: s(r, "name"), hidden: on(r, "hidden"), order: i(r, "order") as i32 }).collect();
        o.products = t.rows("tblproducts").iter().map(|r| Product {
            id: i(r, "id"), gid: i(r, "gid"), name: s(r, "name"), description: s(r, "description"), hidden: on(r, "hidden"),
            retired: on(r, "retired"), servertype: s(r, "servertype"), order: i(r, "order") as i32,
        }).collect();
        o.pricing = pricing(t, "product");
        o.addon_pricing = pricing(t, "addon");
        o.config_pricing = pricing(t, "configoptions");
        o.servers = t.rows("tblservers").iter().map(|r| Server {
            id: i(r, "id"), name: s(r, "name"), ipaddress: s(r, "ipaddress"), hostname: s(r, "hostname"), type_: s(r, "type"),
            username: s(r, "username"), password: s(r, "password"), password_plain: false, accesshash: s(r, "accesshash"),
            secure: on(r, "secure"), port: i(r, "port"), disabled: on(r, "disabled"), maxaccounts: i(r, "maxaccounts"),
        }).collect();
        o.hosting = t.rows("tblhosting").iter().map(|r| Hosting {
            id: i(r, "id"), userid: i(r, "userid"), packageid: i(r, "packageid"), regdate: dt(r, "regdate"), domain: s(r, "domain"),
            billingcycle: s(r, "billingcycle"), nextduedate: dt(r, "nextduedate"), amount: d(r, "amount"), domainstatus: s(r, "domainstatus"),
            server: i(r, "server"), username: s(r, "username"),
        }).collect();
        o.invoices = t.rows("tblinvoices").iter().map(|r| Invoice {
            id: i(r, "id"), userid: i(r, "userid"), invoicenum: s(r, "invoicenum"), date: dt(r, "date"), duedate: dt(r, "duedate"),
            datepaid: dt(r, "datepaid"), subtotal: d(r, "subtotal"), credit: d(r, "credit"), tax: d(r, "tax"), tax2: d(r, "tax2"),
            total: d(r, "total"), taxrate: d(r, "taxrate"), status: s(r, "status"),
        }).collect();
        o.invoice_items = t.rows("tblinvoiceitems").iter().map(|r| InvoiceItem {
            id: i(r, "id"), invoiceid: i(r, "invoiceid"), type_: s(r, "type"), relid: i(r, "relid"), description: s(r, "description"),
            amount: d(r, "amount"), taxed: on(r, "taxed"),
        }).collect();
        o.transactions = t.rows("tblaccounts").iter().map(|r| Transaction {
            id: i(r, "id"), userid: i(r, "userid"), currency: i(r, "currency"), gateway: s(r, "gateway"), date: dt(r, "date"),
            amountin: d(r, "amountin"), amountout: d(r, "amountout"), transid: s(r, "transid"), invoiceid: i(r, "invoiceid"),
        }).collect();
        o.addons = t.rows("tbladdons").iter().map(|r| Addon {
            id: i(r, "id"), packages: s(r, "packages"), name: s(r, "name"), description: s(r, "description"),
            billingcycle: s(r, "billingcycle"), hidden: on(r, "hidden"), retired: on(r, "retired"), weight: i(r, "weight") as i32,
        }).collect();
        o.hosting_addons = t.rows("tblhostingaddons").iter().map(|r| HostingAddon {
            id: i(r, "id"), hostingid: i(r, "hostingid"), addonid: i(r, "addonid"), name: s(r, "name"), recurring: d(r, "recurring"),
            billingcycle: s(r, "billingcycle"), status: s(r, "status"), regdate: dt(r, "regdate"),
        }).collect();
        o.custom_fields = t.rows("tblcustomfields").iter().map(|r| CustomField {
            id: i(r, "id"), type_: s(r, "type"), relid: i(r, "relid"), fieldname: s(r, "fieldname"), fieldtype: s(r, "fieldtype"),
            description: s(r, "description"), fieldoptions: s(r, "fieldoptions"), regexpr: s(r, "regexpr"), required: on(r, "required"),
            adminonly: on(r, "adminonly"), sortorder: i(r, "sortorder") as i32,
        }).collect();
        o.custom_field_values = t.rows("tblcustomfieldsvalues").iter().map(|r| CustomFieldValue { fieldid: i(r, "fieldid"), relid: i(r, "relid"), value: s(r, "value") }).collect();
        o.ticket_departments = t.rows("tblticketdepartments").iter().map(|r| TicketDepartment { id: i(r, "id"), name: s(r, "name"), description: s(r, "description"), hidden: on(r, "hidden"), order: i(r, "order") as i32 }).collect();
        o.tickets = t.rows("tbltickets").iter().map(|r| Ticket {
            id: i(r, "id"), tid: s(r, "tid"), did: i(r, "did"), userid: i(r, "userid"), title: s(r, "title"), message: s(r, "message"),
            status: s(r, "status"), urgency: s(r, "urgency"), date: dt(r, "date"), lastreply: dt(r, "lastreply"),
        }).collect();
        let mut replies: Vec<&Row> = t.rows("tblticketreplies").iter().collect();
        replies.sort_by_key(|r| i(r, "id"));
        o.ticket_replies = replies.into_iter().map(|r| TicketReply { id: i(r, "id"), tid: i(r, "tid"), admin: s(r, "admin"), date: dt(r, "date"), message: s(r, "message") }).collect();
        o.announcements = t.rows("tblannouncements").iter().filter(|r| i(r, "parentid") == 0).map(|r| Announcement {
            id: i(r, "id"), date: dt(r, "date"), title: s(r, "title"), announcement: s(r, "announcement"), published: on(r, "published"),
        }).collect();
        o.kb_categories = t.rows("tblknowledgebasecats").iter().filter(|r| i(r, "catid") == 0).map(|r| KbCategory { id: i(r, "id"), name: s(r, "name"), description: s(r, "description") }).collect();
        let first_cat = |a: i64| t.rows("tblknowledgebaselinks").iter().filter(|l| i(l, "articleid") == a).map(|l| i(l, "categoryid")).min().unwrap_or_default();
        o.kb_articles = t.rows("tblknowledgebase").iter().filter(|r| i(r, "parentid") == 0).map(|r| KbArticle {
            id: i(r, "id"), categoryid: first_cat(i(r, "id")), title: s(r, "title"), article: s(r, "article"), views: i(r, "views"), private: on(r, "private"),
        }).collect();
        o.domains = t.rows("tbldomains").iter().map(|r| Domain {
            id: i(r, "id"), userid: i(r, "userid"), type_: s(r, "type"), domain: s(r, "domain"), registrar: s(r, "registrar"),
            registrationperiod: i(r, "registrationperiod") as i32, registrationdate: dt(r, "registrationdate"), expirydate: dt(r, "expirydate"),
            status: s(r, "status"), donotrenew: on(r, "donotrenew"), dnsmanagement: on(r, "dnsmanagement"), idprotection: on(r, "idprotection"),
        }).collect();
        o.config_groups = t.rows("tblproductconfiggroups").iter().map(|r| {
            let id = i(r, "id");
            ConfigGroup { id, name: s(r, "name"), product_ids: t.rows("tblproductconfiglinks").iter().filter(|l| i(l, "gid") == id).map(|l| i(l, "pid")).collect() }
        }).collect();
        o.config_options = t.rows("tblproductconfigoptions").iter().map(|r| ConfigOption {
            id: i(r, "id"), gid: i(r, "gid"), optionname: s(r, "optionname"), optiontype: i(r, "optiontype") as i32,
            qtyminimum: i(r, "qtyminimum") as i32, qtymaximum: i(r, "qtymaximum") as i32, order: i(r, "order") as i32, hidden: on(r, "hidden"),
        }).collect();
        o.config_subs = t.rows("tblproductconfigoptionssub").iter().map(|r| ConfigSub { id: i(r, "id"), configid: i(r, "configid"), optionname: s(r, "optionname"), sortorder: i(r, "sortorder") as i32, hidden: on(r, "hidden") }).collect();
        o.promotions = t.rows("tblpromotions").iter().map(|r| Promotion {
            id: i(r, "id"), code: s(r, "code"), type_: s(r, "type"), recurring: on(r, "recurring"), value: d(r, "value"),
            appliesto: s(r, "appliesto"), maxuses: i(r, "maxuses"), uses: i(r, "uses"), startdate: dt(r, "startdate"),
            expirationdate: dt(r, "expirationdate"), notes: s(r, "notes"),
        }).collect();
        o.tax_rules = t.rows("tbltax").iter().map(|r| TaxRule { id: i(r, "id"), level: i(r, "level") as i32, name: s(r, "name"), state: s(r, "state"), country: s(r, "country"), taxrate: d(r, "taxrate") }).collect();
        o
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_mysqldump_inserts() {
        let sql = "-- MySQL dump\n/*!40101 SET NAMES utf8mb4 */;\nDROP TABLE IF EXISTS `tblx`;\n\
            CREATE TABLE `tblx` (\n  `id` int(10) NOT NULL AUTO_INCREMENT,\n  `name` text,\n  `amount` decimal(16,2) NOT NULL DEFAULT '0.00',\n  `note` varchar(5) DEFAULT NULL,\n  PRIMARY KEY (`id`),\n  KEY `name` (`name`(32))\n) ENGINE=InnoDB;\n\
            LOCK TABLES `tblx` WRITE;\nINSERT INTO `tblx` VALUES (1,'O\\'Neil; \\\"x\\\"',-1.00,NULL),(2,'it''s\\nok',12.50,0x6869);\nUNLOCK TABLES;\n\
            INSERT IGNORE INTO tbly (`a`, b) VALUES ('1', 'two');";
        let t = parse(sql).unwrap();
        let x = t.rows("tblx");
        assert_eq!(x.len(), 2);
        assert_eq!(x[0]["name"].as_deref(), Some("O'Neil; \"x\""));
        assert_eq!(x[0]["amount"].as_deref(), Some("-1.00"));
        assert_eq!(x[0]["note"], None);
        assert_eq!(x[1]["name"].as_deref(), Some("it's\nok"));
        assert_eq!(x[1]["note"].as_deref(), Some("hi"));
        assert_eq!(t.rows("tbly")[0]["b"].as_deref(), Some("two"));
        assert!(t.rows("nope").is_empty());
        assert!(parse("INSERT INTO t VALUES (1);").is_err());
    }
}
