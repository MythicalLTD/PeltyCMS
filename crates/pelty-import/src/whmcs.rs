//! Row structs for the subset of the WHMCS schema the importer reads.
//! Column names follow the WHMCS database schema (tblclients, tblcurrencies,
//! tblproductgroups, tblproducts, tblpricing, tblhosting, tblinvoices,
//! tblinvoiceitems, tblaccounts). Money columns are `decimal(16,2)` in WHMCS
//! and are carried as exact `Decimal`, never floats.
use rust_decimal::Decimal;
use time::Date;

#[derive(Debug, Clone, Default)]
pub struct Currency {
    pub id: i64,
    pub code: String,
    pub default: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Client {
    pub id: i64,
    pub firstname: String,
    pub lastname: String,
    pub companyname: String,
    pub email: String,
    pub address1: String,
    pub address2: String,
    pub city: String,
    pub state: String,
    pub postcode: String,
    pub country: String,
    pub phonenumber: String,
    pub tax_id: String,
    pub currency: i64,
    pub status: String,
    pub groupid: i64,
    /// `tblclients.credit`, account credit balance in the client's currency.
    pub credit: Decimal,
    /// Login hash: `tblusers.password` (WHMCS 8+, matched by email) or
    /// `tblclients.password` (bcrypt `$2y$...`, or legacy `md5(salt.pw):salt`).
    pub password: String,
}

/// `tblcontacts`: extra people on a client account.
#[derive(Debug, Clone, Default)]
pub struct Contact {
    pub id: i64,
    pub userid: i64,
    pub firstname: String,
    pub lastname: String,
    pub email: String,
    pub phonenumber: String,
    /// `invoiceemails = 1`: receives invoice mail.
    pub billing: bool,
}

/// `tblservers`. `password` is WHMCS-encrypted (cc_encryption_hash) until
/// [`WhmcsData::decrypt_server_passwords`] runs; `accesshash` is plain.
#[derive(Debug, Clone, Default)]
pub struct Server {
    pub id: i64,
    pub name: String,
    pub ipaddress: String,
    pub hostname: String,
    pub type_: String,
    pub username: String,
    pub password: String,
    pub password_plain: bool,
    pub accesshash: String,
    pub secure: bool,
    pub port: i64,
    pub disabled: bool,
    pub maxaccounts: i64,
}

/// `tblpromotions`.
#[derive(Debug, Clone, Default)]
pub struct Promotion {
    pub id: i64,
    pub code: String,
    /// `Percentage` | `Fixed Amount` | `Price Override` | `Free Setup`
    pub type_: String,
    pub recurring: bool,
    pub value: Decimal,
    /// Comma-separated product ids (empty = all).
    pub appliesto: String,
    pub maxuses: i64,
    pub uses: i64,
    pub startdate: Option<Date>,
    pub expirationdate: Option<Date>,
    pub notes: String,
}

/// `tbltax`: `level` 1 or 2 (WHMCS' second tax line).
#[derive(Debug, Clone, Default)]
pub struct TaxRule {
    pub id: i64,
    pub level: i32,
    pub name: String,
    pub state: String,
    pub country: String,
    pub taxrate: Decimal,
}

/// `tbldomains` row.
#[derive(Debug, Clone, Default)]
pub struct Domain {
    pub id: i64,
    pub userid: i64,
    /// `Register` | `Transfer`
    pub type_: String,
    pub domain: String,
    pub registrar: String,
    pub registrationperiod: i32,
    pub registrationdate: Option<Date>,
    pub expirydate: Option<Date>,
    pub status: String,
    pub donotrenew: bool,
    pub dnsmanagement: bool,
    pub idprotection: bool,
}

/// `tblproductconfiggroups` + `tblproductconfiglinks` (gid → product ids).
#[derive(Debug, Clone, Default)]
pub struct ConfigGroup {
    pub id: i64,
    pub name: String,
    pub product_ids: Vec<i64>,
}

/// `tblproductconfigoptions`: optiontype 1 dropdown, 2 radio, 3 yes/no, 4 quantity.
#[derive(Debug, Clone, Default)]
pub struct ConfigOption {
    pub id: i64,
    pub gid: i64,
    pub optionname: String,
    pub optiontype: i32,
    pub qtyminimum: i32,
    pub qtymaximum: i32,
    pub order: i32,
    pub hidden: bool,
}

/// `tblproductconfigoptionssub`; prices are `tblpricing` rows with
/// `type = 'configoptions'` and `relid` = this id.
#[derive(Debug, Clone, Default)]
pub struct ConfigSub {
    pub id: i64,
    pub configid: i64,
    pub optionname: String,
    pub sortorder: i32,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ProductGroup {
    pub id: i64,
    pub name: String,
    pub hidden: bool,
    pub order: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Product {
    pub id: i64,
    pub gid: i64,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub retired: bool,
    pub servertype: String,
    pub order: i32,
}

/// One `tblpricing` row with `type = 'product'`. A price of `-1` means the
/// cycle is disabled in WHMCS.
#[derive(Debug, Clone, Default)]
pub struct Pricing {
    pub relid: i64,
    pub currency: i64,
    pub msetupfee: Decimal,
    pub qsetupfee: Decimal,
    pub ssetupfee: Decimal,
    pub asetupfee: Decimal,
    pub bsetupfee: Decimal,
    pub tsetupfee: Decimal,
    pub monthly: Decimal,
    pub quarterly: Decimal,
    pub semiannually: Decimal,
    pub annually: Decimal,
    pub biennially: Decimal,
    pub triennially: Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct Hosting {
    pub id: i64,
    pub userid: i64,
    pub packageid: i64,
    pub regdate: Option<Date>,
    pub domain: String,
    pub billingcycle: String,
    pub nextduedate: Option<Date>,
    pub amount: Decimal,
    pub domainstatus: String,
    /// `tblhosting.server` (tblservers id, 0 = none) and the panel username.
    pub server: i64,
    pub username: String,
}

#[derive(Debug, Clone, Default)]
pub struct Invoice {
    pub id: i64,
    pub userid: i64,
    pub invoicenum: String,
    pub date: Option<Date>,
    pub duedate: Option<Date>,
    pub datepaid: Option<Date>,
    pub subtotal: Decimal,
    pub credit: Decimal,
    pub tax: Decimal,
    pub tax2: Decimal,
    pub total: Decimal,
    pub taxrate: Decimal,
    pub status: String,
}

#[derive(Debug, Clone, Default)]
pub struct InvoiceItem {
    pub id: i64,
    pub invoiceid: i64,
    pub type_: String,
    pub relid: i64,
    pub description: String,
    pub amount: Decimal,
    pub taxed: bool,
}

/// `tblaccounts`, WHMCS transactions.
#[derive(Debug, Clone, Default)]
pub struct Transaction {
    pub id: i64,
    pub userid: i64,
    pub currency: i64,
    pub gateway: String,
    pub date: Option<Date>,
    pub amountin: Decimal,
    pub amountout: Decimal,
    pub transid: String,
    pub invoiceid: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ClientGroup {
    pub id: i64,
    pub groupname: String,
    pub discountpercent: Decimal,
    pub susptermexempt: bool,
}

/// `tbladdons`; `packages` is WHMCS' comma-separated product id list.
#[derive(Debug, Clone, Default)]
pub struct Addon {
    pub id: i64,
    pub packages: String,
    pub name: String,
    pub description: String,
    pub billingcycle: String,
    pub hidden: bool,
    pub retired: bool,
    pub weight: i32,
}

#[derive(Debug, Clone, Default)]
pub struct HostingAddon {
    pub id: i64,
    pub hostingid: i64,
    pub addonid: i64,
    pub name: String,
    pub recurring: Decimal,
    pub billingcycle: String,
    pub status: String,
    pub regdate: Option<Date>,
}

/// `tblcustomfields` (type `product` or `client`).
#[derive(Debug, Clone, Default)]
pub struct CustomField {
    pub id: i64,
    pub type_: String,
    pub relid: i64,
    pub fieldname: String,
    pub fieldtype: String,
    pub description: String,
    pub fieldoptions: String,
    pub regexpr: String,
    pub required: bool,
    pub adminonly: bool,
    pub sortorder: i32,
}

/// `tblcustomfieldsvalues`: `relid` is a tblhosting id for product fields.
#[derive(Debug, Clone, Default)]
pub struct CustomFieldValue {
    pub fieldid: i64,
    pub relid: i64,
    pub value: String,
}

#[derive(Debug, Clone, Default)]
pub struct TicketDepartment {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub hidden: bool,
    pub order: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Ticket {
    pub id: i64,
    pub tid: String,
    pub did: i64,
    pub userid: i64,
    pub title: String,
    pub message: String,
    pub status: String,
    pub urgency: String,
    pub date: Option<Date>,
    pub lastreply: Option<Date>,
}

/// `tblticketreplies`: `admin` holds the staff name, empty for client replies.
#[derive(Debug, Clone, Default)]
pub struct TicketReply {
    pub id: i64,
    pub tid: i64,
    pub admin: String,
    pub date: Option<Date>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct Announcement {
    pub id: i64,
    pub date: Option<Date>,
    pub title: String,
    pub announcement: String,
    pub published: bool,
}

#[derive(Debug, Clone, Default)]
pub struct KbCategory {
    pub id: i64,
    pub name: String,
    pub description: String,
}

/// `tblknowledgebase` joined with `tblknowledgebaselinks` (first category).
#[derive(Debug, Clone, Default)]
pub struct KbArticle {
    pub id: i64,
    pub categoryid: i64,
    pub title: String,
    pub article: String,
    pub views: i64,
    pub private: bool,
}

/// Everything read from WHMCS. Tests build this directly (in-memory fixture);
/// production fills it via [`load_mysql`] behind the `mysql` feature.
#[derive(Debug, Clone, Default)]
pub struct WhmcsData {
    pub currencies: Vec<Currency>,
    pub clients: Vec<Client>,
    pub product_groups: Vec<ProductGroup>,
    pub products: Vec<Product>,
    pub pricing: Vec<Pricing>,
    pub hosting: Vec<Hosting>,
    pub invoices: Vec<Invoice>,
    pub invoice_items: Vec<InvoiceItem>,
    pub transactions: Vec<Transaction>,
    pub client_groups: Vec<ClientGroup>,
    pub addons: Vec<Addon>,
    /// `tblpricing` rows with `type = 'addon'` (relid = addon id).
    pub addon_pricing: Vec<Pricing>,
    pub hosting_addons: Vec<HostingAddon>,
    pub custom_fields: Vec<CustomField>,
    pub custom_field_values: Vec<CustomFieldValue>,
    pub ticket_departments: Vec<TicketDepartment>,
    pub tickets: Vec<Ticket>,
    pub ticket_replies: Vec<TicketReply>,
    pub announcements: Vec<Announcement>,
    pub kb_categories: Vec<KbCategory>,
    pub kb_articles: Vec<KbArticle>,
    pub domains: Vec<Domain>,
    pub config_groups: Vec<ConfigGroup>,
    pub config_options: Vec<ConfigOption>,
    pub config_subs: Vec<ConfigSub>,
    pub config_pricing: Vec<Pricing>,
    pub contacts: Vec<Contact>,
    pub servers: Vec<Server>,
    pub promotions: Vec<Promotion>,
    pub tax_rules: Vec<TaxRule>,
}

impl WhmcsData {
    /// Decrypt `tblservers.password` with WHMCS' `$cc_encryption_hash`
    /// (from its configuration.php). Returns one warning per server whose
    /// password could not be decrypted (wrong key or not encrypted); those
    /// keep an empty password and fall back to the access hash.
    pub fn decrypt_server_passwords(&mut self, cc_encryption_hash: &str) -> Vec<String> {
        let mut warn = vec![];
        for s in &mut self.servers {
            if s.password.is_empty() || s.password_plain {
                continue;
            }
            match crate::whmcs_crypt::decrypt(cc_encryption_hash, &s.password) {
                Some(p) => {
                    s.password = p;
                    s.password_plain = true;
                }
                None => {
                    warn.push(format!("server {} ({}): password could not be decrypted with the given cc_encryption_hash", s.id, s.name));
                    s.password.clear();
                }
            }
        }
        warn
    }
}

#[cfg(feature = "mysql")]
pub use mysql_loader::load_mysql;

#[cfg(feature = "mysql")]
mod mysql_loader {
    use super::*;
    use sqlx::mysql::MySqlPoolOptions;
    use sqlx::Row;

    fn s(r: &sqlx::mysql::MySqlRow, c: &str) -> String {
        r.try_get::<Option<String>, _>(c)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    fn d(r: &sqlx::mysql::MySqlRow, c: &str) -> Decimal {
        r.try_get::<Option<Decimal>, _>(c)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    fn i(r: &sqlx::mysql::MySqlRow, c: &str) -> i64 {
        r.try_get::<Option<i64>, _>(c)
            .ok()
            .flatten()
            .unwrap_or_default()
    }
    fn dt(r: &sqlx::mysql::MySqlRow, c: &str) -> Option<Date> {
        // WHMCS stores '0000-00-00' for "no date"; we select dates as text.
        let v = s(r, c);
        crate::map::parse_date(&v)
    }

    /// Read the WHMCS tables (read-only `SELECT`s). Integer columns are
    /// CAST to SIGNED and dates to CHAR so zero-dates never error.
    pub async fn load_mysql(dsn: &str) -> Result<WhmcsData, sqlx::Error> {
        let pool = MySqlPoolOptions::new()
            .max_connections(2)
            .connect(dsn)
            .await?;
        let mut out = WhmcsData::default();
        for r in sqlx::query(
            "SELECT CAST(id AS SIGNED) id, code, CAST(`default` AS SIGNED) dflt FROM tblcurrencies",
        )
        .fetch_all(&pool)
        .await?
        {
            out.currencies.push(Currency {
                id: i(&r, "id"),
                code: s(&r, "code"),
                default: i(&r, "dflt") == 1,
            });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, firstname, lastname, companyname, email, address1, address2, city, state, postcode, country, phonenumber, tax_id, CAST(currency AS SIGNED) currency, status, CAST(groupid AS SIGNED) groupid, credit, password FROM tblclients").fetch_all(&pool).await? {
            out.clients.push(Client { id: i(&r,"id"), firstname: s(&r,"firstname"), lastname: s(&r,"lastname"), companyname: s(&r,"companyname"), email: s(&r,"email"), address1: s(&r,"address1"), address2: s(&r,"address2"), city: s(&r,"city"), state: s(&r,"state"), postcode: s(&r,"postcode"), country: s(&r,"country"), phonenumber: s(&r,"phonenumber"), tax_id: s(&r,"tax_id"), currency: i(&r,"currency"), status: s(&r,"status"), groupid: i(&r,"groupid"), credit: d(&r,"credit"), password: s(&r,"password") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, name, CAST(hidden AS SIGNED) hidden, CAST(`order` AS SIGNED) ord FROM tblproductgroups").fetch_all(&pool).await? {
            out.product_groups.push(ProductGroup { id: i(&r,"id"), name: s(&r,"name"), hidden: i(&r,"hidden") == 1, order: i(&r,"ord") as i32 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(gid AS SIGNED) gid, name, description, CAST(hidden AS SIGNED) hidden, CAST(retired AS SIGNED) retired, servertype, CAST(`order` AS SIGNED) ord FROM tblproducts").fetch_all(&pool).await? {
            out.products.push(Product { id: i(&r,"id"), gid: i(&r,"gid"), name: s(&r,"name"), description: s(&r,"description"), hidden: i(&r,"hidden") == 1, retired: i(&r,"retired") == 1, servertype: s(&r,"servertype"), order: i(&r,"ord") as i32 });
        }
        for r in sqlx::query("SELECT CAST(relid AS SIGNED) relid, CAST(currency AS SIGNED) currency, msetupfee, qsetupfee, ssetupfee, asetupfee, bsetupfee, tsetupfee, monthly, quarterly, semiannually, annually, biennially, triennially FROM tblpricing WHERE type = 'product'").fetch_all(&pool).await? {
            out.pricing.push(Pricing { relid: i(&r,"relid"), currency: i(&r,"currency"), msetupfee: d(&r,"msetupfee"), qsetupfee: d(&r,"qsetupfee"), ssetupfee: d(&r,"ssetupfee"), asetupfee: d(&r,"asetupfee"), bsetupfee: d(&r,"bsetupfee"), tsetupfee: d(&r,"tsetupfee"), monthly: d(&r,"monthly"), quarterly: d(&r,"quarterly"), semiannually: d(&r,"semiannually"), annually: d(&r,"annually"), biennially: d(&r,"biennially"), triennially: d(&r,"triennially") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(userid AS SIGNED) userid, CAST(packageid AS SIGNED) packageid, CAST(regdate AS CHAR) regdate, domain, billingcycle, CAST(nextduedate AS CHAR) nextduedate, amount, domainstatus, CAST(server AS SIGNED) server, username FROM tblhosting").fetch_all(&pool).await? {
            out.hosting.push(Hosting { id: i(&r,"id"), userid: i(&r,"userid"), packageid: i(&r,"packageid"), regdate: dt(&r,"regdate"), domain: s(&r,"domain"), billingcycle: s(&r,"billingcycle"), nextduedate: dt(&r,"nextduedate"), amount: d(&r,"amount"), domainstatus: s(&r,"domainstatus"), server: i(&r,"server"), username: s(&r,"username") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(userid AS SIGNED) userid, invoicenum, CAST(date AS CHAR) date, CAST(duedate AS CHAR) duedate, CAST(DATE(datepaid) AS CHAR) datepaid, subtotal, credit, tax, tax2, total, taxrate, status FROM tblinvoices").fetch_all(&pool).await? {
            out.invoices.push(Invoice { id: i(&r,"id"), userid: i(&r,"userid"), invoicenum: s(&r,"invoicenum"), date: dt(&r,"date"), duedate: dt(&r,"duedate"), datepaid: dt(&r,"datepaid"), subtotal: d(&r,"subtotal"), credit: d(&r,"credit"), tax: d(&r,"tax"), tax2: d(&r,"tax2"), total: d(&r,"total"), taxrate: d(&r,"taxrate"), status: s(&r,"status") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(invoiceid AS SIGNED) invoiceid, type, CAST(relid AS SIGNED) relid, description, amount, CAST(taxed AS SIGNED) taxed FROM tblinvoiceitems").fetch_all(&pool).await? {
            out.invoice_items.push(InvoiceItem { id: i(&r,"id"), invoiceid: i(&r,"invoiceid"), type_: s(&r,"type"), relid: i(&r,"relid"), description: s(&r,"description"), amount: d(&r,"amount"), taxed: i(&r,"taxed") == 1 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(userid AS SIGNED) userid, CAST(currency AS SIGNED) currency, gateway, CAST(DATE(date) AS CHAR) date, amountin, amountout, transid, CAST(invoiceid AS SIGNED) invoiceid FROM tblaccounts").fetch_all(&pool).await? {
            out.transactions.push(Transaction { id: i(&r,"id"), userid: i(&r,"userid"), currency: i(&r,"currency"), gateway: s(&r,"gateway"), date: dt(&r,"date"), amountin: d(&r,"amountin"), amountout: d(&r,"amountout"), transid: s(&r,"transid"), invoiceid: i(&r,"invoiceid") });
        }
        let pr = |r: &sqlx::mysql::MySqlRow| Pricing {
            relid: i(r, "relid"),
            currency: i(r, "currency"),
            msetupfee: d(r, "msetupfee"),
            qsetupfee: d(r, "qsetupfee"),
            ssetupfee: d(r, "ssetupfee"),
            asetupfee: d(r, "asetupfee"),
            bsetupfee: d(r, "bsetupfee"),
            tsetupfee: d(r, "tsetupfee"),
            monthly: d(r, "monthly"),
            quarterly: d(r, "quarterly"),
            semiannually: d(r, "semiannually"),
            annually: d(r, "annually"),
            biennially: d(r, "biennially"),
            triennially: d(r, "triennially"),
        };
        for r in sqlx::query("SELECT CAST(relid AS SIGNED) relid, CAST(currency AS SIGNED) currency, msetupfee, qsetupfee, ssetupfee, asetupfee, bsetupfee, tsetupfee, monthly, quarterly, semiannually, annually, biennially, triennially FROM tblpricing WHERE type = 'addon'").fetch_all(&pool).await? {
            out.addon_pricing.push(pr(&r));
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, groupname, discountpercent, CAST(susptermexempt AS CHAR) ste FROM tblclientgroups").fetch_all(&pool).await? {
            let ste = s(&r, "ste");
            out.client_groups.push(ClientGroup { id: i(&r,"id"), groupname: s(&r,"groupname"), discountpercent: d(&r,"discountpercent"), susptermexempt: ste == "on" || ste == "1" });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(packages AS CHAR) packages, name, description, billingcycle, CAST(hidden AS SIGNED) hidden, CAST(retired AS SIGNED) retired, CAST(weight AS SIGNED) weight FROM tbladdons").fetch_all(&pool).await? {
            out.addons.push(Addon { id: i(&r,"id"), packages: s(&r,"packages"), name: s(&r,"name"), description: s(&r,"description"), billingcycle: s(&r,"billingcycle"), hidden: i(&r,"hidden") == 1, retired: i(&r,"retired") == 1, weight: i(&r,"weight") as i32 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(hostingid AS SIGNED) hostingid, CAST(addonid AS SIGNED) addonid, name, recurring, billingcycle, status, CAST(regdate AS CHAR) regdate FROM tblhostingaddons").fetch_all(&pool).await? {
            out.hosting_addons.push(HostingAddon { id: i(&r,"id"), hostingid: i(&r,"hostingid"), addonid: i(&r,"addonid"), name: s(&r,"name"), recurring: d(&r,"recurring"), billingcycle: s(&r,"billingcycle"), status: s(&r,"status"), regdate: dt(&r,"regdate") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, type, CAST(relid AS SIGNED) relid, fieldname, fieldtype, description, fieldoptions, regexpr, CAST(required AS CHAR) req, CAST(adminonly AS CHAR) adm, CAST(sortorder AS SIGNED) so FROM tblcustomfields").fetch_all(&pool).await? {
            let on = |c: &str| { let v = s(&r, c); v == "on" || v == "1" };
            out.custom_fields.push(CustomField { id: i(&r,"id"), type_: s(&r,"type"), relid: i(&r,"relid"), fieldname: s(&r,"fieldname"), fieldtype: s(&r,"fieldtype"), description: s(&r,"description"), fieldoptions: s(&r,"fieldoptions"), regexpr: s(&r,"regexpr"), required: on("req"), adminonly: on("adm"), sortorder: i(&r,"so") as i32 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(userid AS SIGNED) userid, type, domain, registrar, CAST(registrationperiod AS SIGNED) rp, CAST(registrationdate AS CHAR) rd, CAST(expirydate AS CHAR) ed, status, CAST(donotrenew AS SIGNED) dnr, CAST(dnsmanagement AS SIGNED) dns, CAST(idprotection AS SIGNED) idp FROM tbldomains").fetch_all(&pool).await? {
            out.domains.push(Domain { id: i(&r,"id"), userid: i(&r,"userid"), type_: s(&r,"type"), domain: s(&r,"domain"), registrar: s(&r,"registrar"), registrationperiod: i(&r,"rp") as i32, registrationdate: dt(&r,"rd"), expirydate: dt(&r,"ed"), status: s(&r,"status"), donotrenew: i(&r,"dnr") == 1, dnsmanagement: i(&r,"dns") == 1, idprotection: i(&r,"idp") == 1 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, name FROM tblproductconfiggroups").fetch_all(&pool).await? {
            out.config_groups.push(ConfigGroup { id: i(&r,"id"), name: s(&r,"name"), product_ids: vec![] });
        }
        for r in sqlx::query("SELECT CAST(gid AS SIGNED) gid, CAST(pid AS SIGNED) pid FROM tblproductconfiglinks").fetch_all(&pool).await? {
            let (g, p) = (i(&r,"gid"), i(&r,"pid"));
            if let Some(cg) = out.config_groups.iter_mut().find(|x| x.id == g) { cg.product_ids.push(p); }
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(gid AS SIGNED) gid, optionname, CAST(optiontype AS SIGNED) ot, CAST(qtyminimum AS SIGNED) qmin, CAST(qtymaximum AS SIGNED) qmax, CAST(`order` AS SIGNED) ord, CAST(hidden AS SIGNED) hidden FROM tblproductconfigoptions").fetch_all(&pool).await? {
            out.config_options.push(ConfigOption { id: i(&r,"id"), gid: i(&r,"gid"), optionname: s(&r,"optionname"), optiontype: i(&r,"ot") as i32, qtyminimum: i(&r,"qmin") as i32, qtymaximum: i(&r,"qmax") as i32, order: i(&r,"ord") as i32, hidden: i(&r,"hidden") == 1 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(configid AS SIGNED) configid, optionname, CAST(sortorder AS SIGNED) so, CAST(hidden AS SIGNED) hidden FROM tblproductconfigoptionssub").fetch_all(&pool).await? {
            out.config_subs.push(ConfigSub { id: i(&r,"id"), configid: i(&r,"configid"), optionname: s(&r,"optionname"), sortorder: i(&r,"so") as i32, hidden: i(&r,"hidden") == 1 });
        }
        for r in sqlx::query("SELECT CAST(relid AS SIGNED) relid, CAST(currency AS SIGNED) currency, msetupfee, qsetupfee, ssetupfee, asetupfee, bsetupfee, tsetupfee, monthly, quarterly, semiannually, annually, biennially, triennially FROM tblpricing WHERE type = 'configoptions'").fetch_all(&pool).await? {
            out.config_pricing.push(Pricing { relid: i(&r,"relid"), currency: i(&r,"currency"), msetupfee: d(&r,"msetupfee"), qsetupfee: d(&r,"qsetupfee"), ssetupfee: d(&r,"ssetupfee"), asetupfee: d(&r,"asetupfee"), bsetupfee: d(&r,"bsetupfee"), tsetupfee: d(&r,"tsetupfee"), monthly: d(&r,"monthly"), quarterly: d(&r,"quarterly"), semiannually: d(&r,"semiannually"), annually: d(&r,"annually"), biennially: d(&r,"biennially"), triennially: d(&r,"triennially") });
        }
        for r in sqlx::query("SELECT CAST(fieldid AS SIGNED) fieldid, CAST(relid AS SIGNED) relid, CAST(value AS CHAR) value FROM tblcustomfieldsvalues").fetch_all(&pool).await? {
            out.custom_field_values.push(CustomFieldValue { fieldid: i(&r,"fieldid"), relid: i(&r,"relid"), value: s(&r,"value") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, name, description, CAST(hidden AS CHAR) hidden, CAST(`order` AS SIGNED) ord FROM tblticketdepartments").fetch_all(&pool).await? {
            let h = s(&r, "hidden");
            out.ticket_departments.push(TicketDepartment { id: i(&r,"id"), name: s(&r,"name"), description: s(&r,"description"), hidden: h == "on" || h == "1", order: i(&r,"ord") as i32 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, tid, CAST(did AS SIGNED) did, CAST(userid AS SIGNED) userid, title, CAST(message AS CHAR) message, status, urgency, CAST(DATE(date) AS CHAR) date, CAST(DATE(lastreply) AS CHAR) lastreply FROM tbltickets").fetch_all(&pool).await? {
            out.tickets.push(Ticket { id: i(&r,"id"), tid: s(&r,"tid"), did: i(&r,"did"), userid: i(&r,"userid"), title: s(&r,"title"), message: s(&r,"message"), status: s(&r,"status"), urgency: s(&r,"urgency"), date: dt(&r,"date"), lastreply: dt(&r,"lastreply") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(tid AS SIGNED) tid, admin, CAST(DATE(date) AS CHAR) date, CAST(message AS CHAR) message FROM tblticketreplies ORDER BY id").fetch_all(&pool).await? {
            out.ticket_replies.push(TicketReply { id: i(&r,"id"), tid: i(&r,"tid"), admin: s(&r,"admin"), date: dt(&r,"date"), message: s(&r,"message") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(DATE(date) AS CHAR) date, title, CAST(announcement AS CHAR) announcement, CAST(published AS SIGNED) published FROM tblannouncements WHERE parentid = 0").fetch_all(&pool).await? {
            out.announcements.push(Announcement { id: i(&r,"id"), date: dt(&r,"date"), title: s(&r,"title"), announcement: s(&r,"announcement"), published: i(&r,"published") == 1 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, name, description FROM tblknowledgebasecats WHERE catid = 0").fetch_all(&pool).await? {
            out.kb_categories.push(KbCategory { id: i(&r,"id"), name: s(&r,"name"), description: s(&r,"description") });
        }
        for r in sqlx::query("SELECT CAST(k.id AS SIGNED) id, CAST(MIN(l.categoryid) AS SIGNED) categoryid, k.title, CAST(k.article AS CHAR) article, CAST(k.views AS SIGNED) views, CAST(k.private AS CHAR) priv FROM tblknowledgebase k LEFT JOIN tblknowledgebaselinks l ON l.articleid = k.id WHERE k.parentid = 0 GROUP BY k.id, k.title, k.article, k.views, k.private").fetch_all(&pool).await? {
            let p = s(&r, "priv");
            out.kb_articles.push(KbArticle { id: i(&r,"id"), categoryid: i(&r,"categoryid"), title: s(&r,"title"), article: s(&r,"article"), views: i(&r,"views"), private: p == "on" || p == "1" });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(userid AS SIGNED) userid, firstname, lastname, email, phonenumber, CAST(invoiceemails AS SIGNED) ie FROM tblcontacts").fetch_all(&pool).await? {
            out.contacts.push(Contact { id: i(&r,"id"), userid: i(&r,"userid"), firstname: s(&r,"firstname"), lastname: s(&r,"lastname"), email: s(&r,"email"), phonenumber: s(&r,"phonenumber"), billing: i(&r,"ie") == 1 });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, name, ipaddress, hostname, type, username, password, accesshash, CAST(secure AS CHAR) secure, CAST(port AS SIGNED) port, CAST(disabled AS SIGNED) disabled, CAST(maxaccounts AS SIGNED) maxaccounts FROM tblservers").fetch_all(&pool).await? {
            let sec = s(&r, "secure");
            out.servers.push(Server { id: i(&r,"id"), name: s(&r,"name"), ipaddress: s(&r,"ipaddress"), hostname: s(&r,"hostname"), type_: s(&r,"type"), username: s(&r,"username"), password: s(&r,"password"), password_plain: false, accesshash: s(&r,"accesshash"), secure: sec == "on" || sec == "1", port: i(&r,"port"), disabled: i(&r,"disabled") == 1, maxaccounts: i(&r,"maxaccounts") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, code, type, CAST(recurring AS SIGNED) rec, value, CAST(appliesto AS CHAR) appliesto, CAST(maxuses AS SIGNED) maxuses, CAST(uses AS SIGNED) uses, CAST(startdate AS CHAR) sd, CAST(expirationdate AS CHAR) ed, CAST(notes AS CHAR) notes FROM tblpromotions").fetch_all(&pool).await? {
            out.promotions.push(Promotion { id: i(&r,"id"), code: s(&r,"code"), type_: s(&r,"type"), recurring: i(&r,"rec") == 1, value: d(&r,"value"), appliesto: s(&r,"appliesto"), maxuses: i(&r,"maxuses"), uses: i(&r,"uses"), startdate: dt(&r,"sd"), expirationdate: dt(&r,"ed"), notes: s(&r,"notes") });
        }
        for r in sqlx::query("SELECT CAST(id AS SIGNED) id, CAST(level AS SIGNED) level, name, state, country, taxrate FROM tbltax").fetch_all(&pool).await? {
            out.tax_rules.push(TaxRule { id: i(&r,"id"), level: i(&r,"level") as i32, name: s(&r,"name"), state: s(&r,"state"), country: s(&r,"country"), taxrate: d(&r,"taxrate") });
        }
        // WHMCS 8+: logins live in tblusers (matched to clients by email).
        if let Ok(rows) = sqlx::query("SELECT email, password FROM tblusers").fetch_all(&pool).await {
            let hashes: std::collections::HashMap<String, String> = rows.iter().map(|r| (s(r, "email").to_lowercase(), s(r, "password"))).collect();
            for c in &mut out.clients {
                if let Some(h) = hashes.get(&c.email.trim().to_lowercase()).filter(|h| !h.is_empty()) {
                    c.password = h.clone();
                }
            }
        }
        pool.close().await;
        Ok(out)
    }
}
