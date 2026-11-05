//! Pure WHMCS -> PeltyCMS mapping rules.
use rust_decimal::Decimal;
use time::{Date, Month};

pub fn client_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "inactive" => "inactive",
        "closed" => "closed",
        _ => "active",
    }
}

/// `tblhosting.domainstatus` -> `services.status`.
pub fn service_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "active" => "active",
        "suspended" => "suspended",
        "terminated" | "completed" => "terminated",
        "cancelled" => "cancelled",
        "fraud" => "fraud",
        _ => "pending",
    }
}

/// WHMCS billing cycle label -> PeltyCMS cycle.
pub fn cycle(s: &str) -> Option<&'static str> {
    Some(
        match s.to_ascii_lowercase().replace(['-', ' '], "").as_str() {
            "monthly" => "monthly",
            "quarterly" => "quarterly",
            "semiannually" => "semiannual",
            "annually" => "annual",
            "biennially" => "biennial",
            "triennially" => "triennial",
            "onetime" | "freeaccount" | "free" => "one_time",
            _ => return None,
        },
    )
}

/// `tblinvoices.status` -> `invoices.status`.
pub fn invoice_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "paid" => "paid",
        "cancelled" => "void",
        "refunded" => "refunded",
        "draft" => "draft",
        _ => "unpaid", // Unpaid, Payment Pending, Collections
    }
}

/// Invoice number preserved verbatim: custom `invoicenum` if set, else the
/// WHMCS invoice id (which is what WHMCS shows when numbering is off).
pub fn invoice_number(id: i64, invoicenum: &str) -> String {
    let n = invoicenum.trim();
    if n.is_empty() {
        id.to_string()
    } else {
        n.to_string()
    }
}

/// WHMCS uses `-1.00` for "cycle disabled".
pub fn price_enabled(p: Decimal) -> bool {
    p >= Decimal::ZERO
}

/// Unique, constraint-safe slug: `^[a-z0-9][a-z0-9-]{0,63}$`.
pub fn slug(name: &str, prefix: &str, id: i64) -> String {
    let mut s = String::new();
    for c in name.to_ascii_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') && !s.is_empty() {
            s.push('-');
        }
    }
    let suffix = format!("{prefix}{id}");
    s.truncate(63 - suffix.len().min(60));
    let s = s.trim_end_matches('-');
    if s.is_empty() {
        suffix
    } else {
        format!("{s}-{suffix}")
    }
}

/// Parse `YYYY-MM-DD` (optionally followed by time). WHMCS zero dates -> None.
pub fn parse_date(v: &str) -> Option<Date> {
    let v = v.get(..10)?;
    let mut it = v.split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    if y == 0 {
        return None;
    }
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// Country must be ISO-3166 alpha-2 upper-case or empty.
pub fn country(c: &str) -> String {
    let c = c.trim().to_ascii_uppercase();
    if c.len() == 2 && c.chars().all(|x| x.is_ascii_uppercase()) {
        c
    } else {
        String::new()
    }
}

/// Truncate to a column's char limit without splitting a code point.
pub fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// `tbltickets.status` -> `tickets.status`.
pub fn ticket_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().replace([' ', '_'], "-").as_str() {
        "answered" => "answered",
        "customer-reply" => "customer_reply",
        "on-hold" => "on_hold",
        "closed" => "closed",
        _ => "open", // Open, In Progress, custom statuses
    }
}

pub fn ticket_priority(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        "urgent" | "critical" => "urgent",
        _ => "medium",
    }
}

/// `tblhostingaddons.status` -> `service_addons.status`.
pub fn addon_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().as_str() {
        "active" | "pending" => "active",
        "suspended" => "suspended",
        _ => "cancelled",
    }
}

/// `tblcustomfields.fieldtype` -> (kind, choices). Dropdown options are
/// comma separated in WHMCS. Link/password fields degrade to text.
pub fn field_kind(fieldtype: &str, options: &str) -> (&'static str, Vec<String>) {
    match fieldtype.to_ascii_lowercase().as_str() {
        "dropdown" => (
            "select",
            options
                .split(',')
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect(),
        ),
        "tickbox" => ("checkbox", vec![]),
        "textarea" => ("textarea", vec![]),
        _ => ("text", vec![]),
    }
}

/// WHMCS tickbox values are "on" / "", PeltyCMS stores "true".
pub fn field_value(kind: &str, v: &str) -> Option<String> {
    let v = v.trim();
    if kind == "checkbox" {
        return (v == "on" || v == "1" || v.eq_ignore_ascii_case("yes")).then(|| "true".into());
    }
    (!v.is_empty()).then(|| v.to_string())
}

/// `tbldomains.status` -> `domains.status`.
pub fn domain_status(s: &str) -> &'static str {
    match s.to_ascii_lowercase().replace(['-', '_'], " ").trim() {
        "active" => "active",
        "grace" => "grace",
        "redemption" => "redemption",
        "expired" => "expired",
        "pending transfer" => "transferring",
        "transferred away" => "transferred_away",
        "cancelled" => "cancelled",
        "fraud" => "fraud",
        _ => "pending",
    }
}

/// `tblproductconfigoptions.optiontype` -> `product_config_options.option_type`.
pub fn config_option_type(t: i32) -> &'static str {
    match t {
        2 => "radio",
        3 => "yesno",
        4 => "quantity",
        _ => "dropdown",
    }
}

/// Split a domain into (label-less name, tld): "shop.example.co.uk" -> tld "co.uk"
/// is not knowable without a suffix list, so the TLD is everything after the
/// first dot ("example.co.uk" -> "co.uk"), matching how WHMCS stores TLD pricing.
pub fn domain_tld(name: &str) -> Option<String> {
    let n = name.trim().trim_end_matches('.').to_ascii_lowercase();
    let (label, tld) = n.split_once('.')?;
    if label.is_empty() || tld.is_empty() {
        return None;
    }
    Some(tld.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn statuses_and_cycles() {
        assert_eq!(service_status("Completed"), "terminated");
        assert_eq!(service_status("Pending"), "pending");
        assert_eq!(invoice_status("Cancelled"), "void");
        assert_eq!(invoice_status("Payment Pending"), "unpaid");
        assert_eq!(invoice_status("Collections"), "unpaid");
        assert_eq!(cycle("Semi-Annually"), Some("semiannual"));
        assert_eq!(cycle("One Time"), Some("one_time"));
        assert_eq!(cycle("Free Account"), Some("one_time"));
        assert_eq!(cycle("Weekly"), None);
        assert_eq!(client_status("Closed"), "closed");
        assert_eq!(ticket_status("Customer-Reply"), "customer_reply");
        assert_eq!(ticket_status("On Hold"), "on_hold");
        assert_eq!(ticket_status("In Progress"), "open");
        assert_eq!(ticket_priority("High"), "high");
        assert_eq!(addon_status("Terminated"), "cancelled");
        assert_eq!(field_kind("dropdown", "a, b,c").1, vec!["a", "b", "c"]);
        assert_eq!(field_value("checkbox", "on").as_deref(), Some("true"));
        assert_eq!(field_value("checkbox", ""), None);
    }

    #[test]
    fn numbers_dates_slugs() {
        assert_eq!(invoice_number(42, ""), "42");
        assert_eq!(invoice_number(42, " 2024-0042 "), "2024-0042");
        assert_eq!(parse_date("0000-00-00"), None);
        assert_eq!(parse_date("2024-02-29 10:00:00").map(|d| d.day()), Some(29));
        assert_eq!(slug("Shared Hosting!!", "w", 3), "shared-hosting-w3");
        assert_eq!(slug("ŞŢĂ", "w", 3), "w3");
        assert!(slug(&"x".repeat(200), "w", 12345).len() <= 64);
        assert!(!price_enabled(Decimal::from_str("-1.00").unwrap()));
        assert!(price_enabled(Decimal::ZERO));
        assert_eq!(country("ro"), "RO");
        assert_eq!(country("Romania"), "");
    }
}
