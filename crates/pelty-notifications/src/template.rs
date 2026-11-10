//! `{{var}}` substitution. Deliberately logic-less: operators edit
//! templates in the admin UI, so there is nothing to execute.
use std::collections::BTreeMap;

/// A known template and the merge fields it may use.
pub struct TemplateDef {
    pub name: &'static str,
    pub vars: &'static [&'static str],
}

pub const TEMPLATES: &[TemplateDef] = &[
    TemplateDef {
        name: "admin_account_locked",
        vars: &["email", "ip", "attempts", "minutes", "company"],
    },
    TemplateDef {
        name: "admin_login_alert",
        vars: &[
            "email",
            "ip",
            "location",
            "city",
            "region",
            "country",
            "country_name",
            "user_agent",
            "time",
            "company",
        ],
    },
    TemplateDef {
        name: "password_reset",
        vars: &["name", "link", "expires_minutes", "company"],
    },
    TemplateDef {
        name: "client_invite",
        vars: &["name", "link", "expires_days", "company"],
    },
    TemplateDef {
        name: "staff_invite",
        vars: &["name", "link", "expires_days", "company"],
    },
    TemplateDef {
        name: "client_welcome",
        vars: &["name", "link", "company"],
    },
    TemplateDef {
        name: "contact_invite",
        vars: &["name", "inviter", "link", "expires_days", "company"],
    },
    TemplateDef {
        name: "email_verify",
        vars: &["name", "link", "company"],
    },
    TemplateDef {
        name: "cart_abandoned",
        vars: &["items", "link", "company"],
    },
    TemplateDef {
        name: "invoice_created",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "due_date",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "invoice_reminder",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "due_date",
            "days_overdue",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "payment_failed",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "reason",
            "next_retry",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "invoice_due_soon",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "due_date",
            "days_left",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "service_suspended",
        vars: &["name", "service", "reason", "link", "company"],
    },
    TemplateDef {
        name: "registrant_verify",
        vars: &["name", "domain", "email", "deadline", "link", "company"],
    },
    TemplateDef {
        name: "domain_expiry",
        vars: &["name", "domain", "expires_on", "days", "link", "company"],
    },
    TemplateDef {
        name: "ticket_reply",
        vars: &["name", "ticket_number", "subject", "link", "company"],
    },
    TemplateDef {
        name: "ticket_auto_closed",
        vars: &["name", "ticket_number", "subject", "link", "company"],
    },
    TemplateDef {
        name: "ticket_autoreply",
        vars: &[
            "name",
            "ticket_number",
            "subject",
            "department",
            "message",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "ticket_staff_notify",
        vars: &[
            "name",
            "ticket_number",
            "subject",
            "customer",
            "department",
            "message",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "quote_sent",
        vars: &[
            "name",
            "quote_number",
            "subject",
            "total",
            "valid_until",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "creator_payout_paid",
        vars: &["name", "amount", "method", "link", "company"],
    },
    TemplateDef {
        name: "creator_payout_failed",
        vars: &["name", "amount", "reason", "link", "company"],
    },
    TemplateDef {
        name: "digital_version_released",
        vars: &["name", "product", "version", "link", "company"],
    },
    TemplateDef {
        name: "affiliate_payout_requested",
        vars: &["affiliate", "amount", "link", "company"],
    },
    TemplateDef {
        name: "client_signup",
        vars: &["name", "email", "link", "company"],
    },
    TemplateDef {
        name: "password_changed",
        vars: &["name", "ip", "link", "company"],
    },
    TemplateDef {
        name: "order_confirmation",
        vars: &["name", "order_number", "total", "link", "company"],
    },
    TemplateDef {
        name: "quote_accepted",
        vars: &["name", "quote_number", "total", "link", "company"],
    },
    TemplateDef {
        name: "credit_card_expiring",
        vars: &["name", "card", "expires", "link", "company"],
    },
    TemplateDef {
        name: "new_service_info",
        vars: &["name", "service", "username", "server", "link", "company"],
    },
    TemplateDef {
        name: "service_unsuspended",
        vars: &["name", "service", "link", "company"],
    },
    TemplateDef {
        name: "service_terminated",
        vars: &["name", "service", "link", "company"],
    },
    TemplateDef {
        name: "cancellation_confirmation",
        vars: &["name", "service", "cancel_date", "link", "company"],
    },
    TemplateDef {
        name: "upgrade_order_confirmation",
        vars: &["name", "service", "new_plan", "total", "link", "company"],
    },
    TemplateDef {
        name: "domain_registration_confirmation",
        vars: &["name", "domain", "expires_on", "link", "company"],
    },
    TemplateDef {
        name: "domain_renewal_confirmation",
        vars: &["name", "domain", "expires_on", "link", "company"],
    },
    TemplateDef {
        name: "domain_transfer_initiated",
        vars: &["name", "domain", "link", "company"],
    },
    TemplateDef {
        name: "domain_transfer_completed",
        vars: &["name", "domain", "expires_on", "link", "company"],
    },
    TemplateDef {
        name: "domain_transfer_failed",
        vars: &["name", "domain", "reason", "link", "company"],
    },
    TemplateDef {
        name: "domain_expired",
        vars: &["name", "domain", "expires_on", "link", "company"],
    },
    TemplateDef {
        name: "invoice_payment_confirmation",
        vars: &["name", "invoice_number", "total", "link", "company"],
    },
    TemplateDef {
        name: "invoice_second_overdue",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "due_date",
            "days_overdue",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "invoice_third_overdue",
        vars: &[
            "name",
            "invoice_number",
            "total",
            "due_date",
            "days_overdue",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "invoice_refund_confirmation",
        vars: &["name", "invoice_number", "total", "link", "company"],
    },
    TemplateDef {
        name: "credit_added",
        vars: &["name", "amount", "balance", "link", "company"],
    },
    TemplateDef {
        name: "ticket_opened",
        vars: &["name", "ticket_number", "subject", "link", "company"],
    },
    TemplateDef {
        name: "ticket_auto_close",
        vars: &["name", "ticket_number", "subject", "link", "company"],
    },
    TemplateDef {
        name: "ticket_feedback_request",
        vars: &["name", "ticket_number", "subject", "link", "company"],
    },
    TemplateDef {
        name: "affiliate_commission_earned",
        vars: &["affiliate", "amount", "balance", "link", "company"],
    },
    TemplateDef {
        name: "affiliate_monthly_referrals",
        vars: &[
            "affiliate",
            "signups",
            "conversions",
            "balance",
            "link",
            "company",
        ],
    },
    TemplateDef {
        name: "admin_new_order",
        vars: &["order_number", "customer", "total", "link", "company"],
    },
    TemplateDef {
        name: "admin_password_reset",
        vars: &["name", "link", "company"],
    },
    TemplateDef {
        name: "mass_mail",
        vars: &[
            "name",
            "first_name",
            "last_name",
            "email",
            "company_name",
            "subject",
            "message",
            "company",
        ],
    },
    TemplateDef {
        name: "provisioning_failed",
        vars: &["service", "action", "error", "link", "company"],
    },
    TemplateDef {
        name: "automation_task_tripped",
        vars: &["task", "failures", "error", "link", "company"],
    },
];

/// WHMCS-style template category, used to group the admin list.
pub fn category(name: &str) -> &'static str {
    match name {
        "admin_account_locked"
        | "admin_login_alert"
        | "ticket_staff_notify"
        | "admin_new_order"
        | "admin_password_reset"
        | "automation_task_tripped" => "admin",
        n if n.starts_with("affiliate_") => "affiliate",
        n if n.starts_with("ticket_") => "support",
        n if n.starts_with("domain_") => "domain",
        n if n.starts_with("invoice_")
            || n == "payment_failed"
            || n == "credit_added"
            || n == "credit_card_expiring" =>
        {
            "invoice"
        }
        "new_service_info"
        | "service_suspended"
        | "service_unsuspended"
        | "service_terminated"
        | "cancellation_confirmation"
        | "upgrade_order_confirmation" => "product",
        _ => "general",
    }
}

pub fn def(name: &str) -> Option<&'static TemplateDef> {
    TEMPLATES.iter().find(|t| t.name == name)
}

pub fn escape_html(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

/// Operator-authored HTML: markup is trusted; merge field *values* are escaped.
pub fn render_html(tpl: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(tpl.len() + 64);
    let mut rest = tpl;
    while let Some(start) = rest.find("{{") {
        let (lit, after) = rest.split_at(start);
        out.push_str(lit);
        match after[2..].find("}}") {
            Some(end) => {
                let key = after[2..2 + end].trim();
                let v = vars.get(key).map(String::as_str).unwrap_or("");
                out.push_str(&escape_html(v));
                rest = &after[2 + end + 2..];
            }
            None => {
                out.push_str(after);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Replace `{{ name }}` with values. Unknown fields render empty. With
/// `html`, values are escaped (the template itself is operator-trusted text
/// and is escaped too, since we only ever produce HTML from plain text).
pub fn render(tpl: &str, vars: &BTreeMap<String, String>, html: bool) -> String {
    let mut out = String::with_capacity(tpl.len() + 64);
    let mut rest = tpl;
    while let Some(start) = rest.find("{{") {
        let (lit, after) = rest.split_at(start);
        out.push_str(&if html {
            escape_html(lit)
        } else {
            lit.to_string()
        });
        match after[2..].find("}}") {
            Some(end) => {
                let key = after[2..2 + end].trim();
                let v = vars.get(key).map(String::as_str).unwrap_or("");
                out.push_str(&if html { escape_html(v) } else { v.to_string() });
                rest = &after[2 + end + 2..];
            }
            None => {
                out.push_str(&if html {
                    escape_html(after)
                } else {
                    after.to_string()
                });
                rest = "";
            }
        }
    }
    out.push_str(&if html {
        escape_html(rest)
    } else {
        rest.to_string()
    });
    out
}

/// Plain text body -> minimal HTML (escaped, paragraphs, autolinked URLs).
pub fn text_to_html(text: &str) -> String {
    let mut body = String::new();
    for para in text.split("\n\n") {
        let mut p = String::new();
        for (i, line) in para.lines().enumerate() {
            if i > 0 {
                p.push_str("<br>");
            }
            for (j, word) in line.split(' ').enumerate() {
                if j > 0 {
                    p.push(' ');
                }
                if word.starts_with("https://") || word.starts_with("http://") {
                    let w = escape_html(word);
                    p.push_str(&format!("<a href=\"{w}\" style=\"color:#1f6f8b\">{w}</a>"));
                } else {
                    p.push_str(&escape_html(word));
                }
            }
        }
        body.push_str(&format!("<p>{p}</p>"));
    }
    // Pelty brand (peltycms.com): flat card, fine border, Manrope, cyan accent.
    format!(
        "<!doctype html><html><body style=\"margin:0;padding:24px 12px;background:#f8fafc;color:#0b1220;font-family:Manrope,system-ui,sans-serif;line-height:1.5\">\
<div style=\"max-width:600px;margin:0 auto;background:#ffffff;border:1px solid #e2e8f0;border-radius:8px;padding:24px\">\
<p style=\"margin:0 0 16px;font-weight:700;font-size:18px;color:#080e16\">Pelty<span style=\"color:#1f6f8b\">CMS</span></p>\
{body}</div></body></html>"
    )
}

/// Merge fields a template uses that its definition doesn't allow.
pub fn unknown_vars(tpl: &str, def: &TemplateDef) -> Vec<String> {
    let mut bad = Vec::new();
    let mut rest = tpl;
    while let Some(s) = rest.find("{{") {
        let after = &rest[s + 2..];
        let Some(e) = after.find("}}") else { break };
        let k = after[..e].trim();
        if !def.vars.contains(&k) && !bad.iter().any(|b| b == k) {
            bad.push(k.to_string());
        }
        rest = &after[e + 2..];
    }
    bad
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_template_has_a_name_in_both_locales() {
        for loc in ["en", "ro"] {
            let path = format!("{}/../../locales/{loc}.json", env!("CARGO_MANIFEST_DIR"));
            let raw = std::fs::read_to_string(&path).expect("locale file");
            let map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(&raw).expect("locale json");
            for t in TEMPLATES {
                let key = format!("email.name.{}", t.name);
                assert!(map.contains_key(&key), "{loc}.json is missing {key}");
            }
        }
    }
    fn v(p: &[(&str, &str)]) -> BTreeMap<String, String> {
        p.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }
    #[test]
    fn renders_and_escapes() {
        let vars = v(&[("name", "<Ana>"), ("link", "https://x/r?t=1&a=2")]);
        assert_eq!(
            render("Hi {{name}}, go {{ link }} {{missing}}.", &vars, false),
            "Hi <Ana>, go https://x/r?t=1&a=2 ."
        );
        assert_eq!(
            render("<b>{{name}}</b>", &vars, true),
            "&lt;b&gt;&lt;Ana&gt;&lt;/b&gt;"
        );
        assert_eq!(render("open {{name", &vars, false), "open {{name");
        assert_eq!(
            render_html("<p>Hi <b>{{name}}</b></p>", &vars),
            "<p>Hi <b>&lt;Ana&gt;</b></p>"
        );
    }
    #[test]
    fn html_and_validation() {
        let h = text_to_html("Hi <x>\n\nClick https://a.b/c?d=1&e=2 now");
        assert!(h.contains("<p>Hi &lt;x&gt;</p>"));
        assert!(h.contains("<a href=\"https://a.b/c?d=1&amp;e=2\""));
        assert!(h.contains("Pelty<span"));
        let d = def("password_reset").unwrap();
        assert_eq!(
            unknown_vars("{{name}} {{link}} {{evil}} {{evil}}", d),
            vec!["evil"]
        );
    }
}
