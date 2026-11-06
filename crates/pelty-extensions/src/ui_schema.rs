//! Declarative plugin UI: plugins describe pages and slot contributions as a
//! JSON node tree that the frontend renders natively with shadcn components
//! and the active theme. No plugin JavaScript ever runs in the app; `embed`
//! (a sandboxed iframe) is a documented last resort behind the `ui.embed`
//! capability. See `make docs` for the node reference.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Every node type the renderer understands. Unknown types are rejected at
/// manifest load so a plugin never renders half-broken.
pub const NODE_TYPES: &[&str] = &[
    // layout
    "stack",
    "row",
    "grid",
    "card",
    "section",
    "separator",
    "tabs",
    "tab",
    "accordion",
    "accordion_item",
    "dialog",
    // content
    "heading",
    "text",
    "badge",
    "alert",
    "stat",
    "kv",
    "code",
    "image",
    "avatar",
    "icon",
    "progress",
    "skeleton",
    "empty",
    "tooltip",
    "money",
    "date",
    "link",
    // data
    "table",
    "list",
    "each",
    "if",
    "chart",
    // interaction
    "button",
    "form",
    "input",
    "textarea",
    "select",
    "switch",
    "checkbox",
    "radio",
    // overlays / navigation
    "collapsible",
    "popover",
    "sheet",
    "dropdown",
    "breadcrumb",
    "pagination",
    "scroll_area",
    // typography
    "label",
    "kbd",
    "blockquote",
    "list_item",
    // addon iframe widget (needs the `ui.iframe` capability)
    "frame",
    // last resort
    "embed",
];

/// Slots a plugin can fill with a node tree. Context keys available as
/// `{{ctx.*}}` are listed next to each slot.
pub const UI_RENDER_SLOTS: &[(&str, &[&str])] = &[
    ("admin.dashboard", &[]),
    ("client.dashboard", &[]),
    ("admin.service.detail", &["service_id"]),
    ("admin.customer.detail", &["customer_id"]),
    ("admin.order.detail", &["order_id"]),
    ("admin.invoice.detail", &["invoice_id"]),
    ("client.home", &[]),
    // Above the reply box of a ticket (staff and client side).
    ("admin.ticket.reply", &["ticket_id"]),
    ("client.ticket.reply", &["ticket_id"]),
    // Extra checkout step, rendered before the payment method card.
    ("client.checkout.step", &["currency", "gateway"]),
    // Client invoice / service detail, under the main content.
    ("client.invoice.detail", &["invoice_id"]),
    ("client.service.detail", &["service_id"]),
];

/// Positions an addon can fill on every page. A slot name is
/// `<area>.<route>.<position>`: `area` is `admin` or `client`, `route` is the
/// page's route key (path segments without ids joined by `-`, e.g.
/// `/admin/invoices/42` → `invoices`, `/admin/settings/tax` → `settings-tax`)
/// or `*` for every page. `replace:<section>` swaps out a named section,
/// `before:<section>` / `after:<section>` render next to it.
pub const SECTION_POSITIONS: &[&str] = &["replace", "before", "after"];
pub const PAGE_POSITIONS: &[&str] = &[
    "before",
    "after",
    "toolbar",
    "row-actions",
    "tabs",
    "detail-panel",
];
/// Layout slots: `<area>.layout.<position>` (`*` not needed: always global).
pub const LAYOUT_POSITIONS: &[&str] = &["header", "sidebar", "footer", "banner"];

fn key_ok(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= 64
        && k.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Accepts the named legacy slots and the generic page/layout slots.
pub fn valid_slot(name: &str) -> bool {
    if UI_RENDER_SLOTS.iter().any(|(n, _)| *n == name) {
        return true;
    }
    let mut it = name.splitn(3, '.');
    let (Some(area), Some(route), Some(pos)) = (it.next(), it.next(), it.next()) else {
        return false;
    };
    if area != "admin" && area != "client" {
        return false;
    }
    if route == "layout" {
        return LAYOUT_POSITIONS.contains(&pos);
    }
    (route == "*" || key_ok(route))
        && (PAGE_POSITIONS.contains(&pos)
            || pos
                .split_once(':')
                .is_some_and(|(p, s)| SECTION_POSITIONS.contains(&p) && key_ok(s)))
}

/// A sandboxed iframe served from the addon's own `assets/` folder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    /// Package path, e.g. `assets/ui/index.html` (served at
    /// `/api/v1/addons/<id>/assets/ui/index.html`).
    pub src: String,
    /// Initial height in px (the frame auto-resizes via the bridge).
    #[serde(default)]
    pub height: Option<u32>,
}

/// Safe `assets/...` package path of a static file.
pub fn asset_path_ok(p: &str) -> bool {
    let Some(rest) = p.strip_prefix("assets/") else {
        return false;
    };
    !rest.is_empty()
        && p.len() <= 200
        && rest.split('/').all(|seg| {
            !seg.is_empty()
                && seg != "."
                && seg != ".."
                && seg
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}

fn validate_frame(rules: &UiRules, f: &Frame) -> Result<(), String> {
    if !rules.allow_iframe {
        return Err("frames need the ui.iframe capability".into());
    }
    if !asset_path_ok(&f.src) || !f.src.ends_with(".html") {
        return Err(format!("frame src {} must be an assets/*.html file", f.src));
    }
    if f.height.is_some_and(|h| !(40..=4000).contains(&h)) {
        return Err("frame height must be 40..=4000".into());
    }
    Ok(())
}

/// Exactly one of `body` (declarative) and `frame` (iframe).
fn validate_content(rules: &UiRules, body: &Value, frame: &Option<Frame>) -> Result<(), String> {
    match (body.is_null(), frame) {
        (true, Some(f)) => validate_frame(rules, f),
        (false, None) => validate_node(rules, body),
        _ => Err("give either body or frame".into()),
    }
}

pub const MAX_NODES: usize = 2000;
pub const MAX_DEPTH: usize = 32;
const MAX_STR: usize = 8000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DataSource {
    /// GET path under `/api/v1/`; may contain `{{ctx.*}}` / `{{params.*}}`.
    pub get: String,
    /// Auto-refresh interval (5..=3600 s).
    #[serde(default)]
    pub refresh_secs: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UiPage {
    /// `[a-z0-9-]`, unique per plugin; route `/admin/p/<plugin>/<id>` or `/p/<plugin>/<id>`.
    pub id: String,
    /// `admin` or `client`.
    pub area: String,
    /// Translation key in `plugin.<id>.*`.
    pub title: String,
    /// Phosphor icon name (PascalCase without `Icon`), e.g. `HardDrives`.
    #[serde(default)]
    pub icon: Option<String>,
    /// Show in the sidebar.
    #[serde(default = "yes")]
    pub nav: bool,
    /// Nav category whose hub lists the page (`billing`, `addons`, …).
    #[serde(default)]
    pub hub: Option<String>,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub data: BTreeMap<String, DataSource>,
    /// Declarative node tree (or `null` when `frame` is set).
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub frame: Option<Frame>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UiSlotContribution {
    pub slot: String,
    #[serde(default)]
    pub permission: Option<String>,
    #[serde(default)]
    pub data: BTreeMap<String, DataSource>,
    #[serde(default)]
    pub body: Value,
    #[serde(default)]
    pub frame: Option<Frame>,
    /// Order among contributions to the same slot (lower first).
    #[serde(default)]
    pub order: i32,
}

fn yes() -> bool {
    true
}

/// What a plugin may do from its UI.
pub struct UiRules<'a> {
    pub plugin_id: &'a str,
    /// `ui.embed`: allow `embed` nodes (https iframes, sandboxed).
    pub allow_embed: bool,
    /// `ui.core_actions`: allow request actions to core endpoints (not only
    /// `/api/v1/plugins/<id>/`). Requests always carry the viewer's session,
    /// so this never grants more than the viewer already has.
    pub allow_core_actions: bool,
    /// The extension's own API prefix; `None` = `/api/v1/plugins/<id>/`
    /// (WASM addons use `/api/v1/addons/<id>/`).
    pub own_api: Option<&'a str>,
    /// `ui.iframe` (WASM addons, approved at install): sandboxed frames
    /// served from the addon's `assets/`.
    pub allow_iframe: bool,
}

/// Path with optional `{{...}}` placeholders: same-origin, no traversal.
pub fn safe_templated_path(p: &str) -> bool {
    if !p.starts_with('/') || p.starts_with("//") || p.len() > 400 || p.contains('\\') {
        return false;
    }
    // Placeholders become URL-encoded segments at runtime; check the rest.
    let mut rest = String::new();
    let mut s = p;
    while let Some(i) = s.find("{{") {
        rest.push_str(&s[..i]);
        let Some(j) = s[i..].find("}}") else {
            return false;
        };
        let inner = &s[i + 2..i + j];
        if !inner
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._".contains(c))
        {
            return false;
        }
        rest.push('x');
        s = &s[i + j + 2..];
    }
    rest.push_str(s);
    let (path, query) = rest.split_once('?').unwrap_or((&rest, ""));
    !path.split('/').any(|seg| seg == ".." || seg == ".")
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_/.".contains(c))
        && query
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.=&%".contains(c))
}

fn api_path(p: &str) -> bool {
    safe_templated_path(p) && p.starts_with("/api/v1/")
}

pub fn validate_data(rules: &UiRules, data: &BTreeMap<String, DataSource>) -> Result<(), String> {
    if data.len() > 20 {
        return Err("too many data sources".into());
    }
    for (k, d) in data {
        if !crate::valid_id(k) && !(k.len() == 1 && k.chars().all(|c| c.is_ascii_lowercase())) {
            return Err(format!("bad data source name {k}"));
        }
        if !api_path(&d.get) || d.get.starts_with("/api/v1/auth/") {
            return Err(format!("unsafe data path {}", d.get));
        }
        if d.refresh_secs.is_some_and(|s| !(5..=3600).contains(&s)) {
            return Err("refresh_secs must be 5..=3600".into());
        }
    }
    let _ = rules;
    Ok(())
}

fn validate_action(rules: &UiRules, a: &Value) -> Result<(), String> {
    let o = a.as_object().ok_or("action must be an object")?;
    match o.get("type").and_then(Value::as_str) {
        Some("refresh") => Ok(()),
        Some("navigate") => {
            let to = o
                .get("to")
                .and_then(Value::as_str)
                .ok_or("navigate needs to")?;
            if !safe_templated_path(to) || to.starts_with("/api/") {
                return Err(format!("unsafe navigate target {to}"));
            }
            Ok(())
        }
        Some("request") => {
            let m = o.get("method").and_then(Value::as_str).unwrap_or("POST");
            if !["POST", "PUT", "PATCH", "DELETE"].contains(&m) {
                return Err(format!("bad method {m}"));
            }
            let p = o
                .get("path")
                .and_then(Value::as_str)
                .ok_or("request needs path")?;
            let own = rules
                .own_api
                .map(String::from)
                .unwrap_or_else(|| format!("/api/v1/plugins/{}/", rules.plugin_id));
            if !api_path(p)
                || p.starts_with("/api/v1/auth/")
                || (!p.starts_with(&own) && !rules.allow_core_actions)
            {
                return Err(format!(
                    "request path {p} not allowed (needs ui.core_actions outside {own})"
                ));
            }
            if let Some(then) = o.get("then") {
                validate_action(rules, then)?;
            }
            Ok(())
        }
        other => Err(format!("unknown action type {other:?}")),
    }
}

/// Keys whose values are nodes / node lists / actions anywhere in the tree.
const NODE_KEYS: &[&str] = &["template", "empty", "else", "trigger", "fallback"];
const NODE_LIST_KEYS: &[&str] = &["children", "footer", "actions"];

pub fn validate_node(rules: &UiRules, node: &Value) -> Result<(), String> {
    let mut count = 0;
    walk(rules, node, 0, &mut count)
}

fn check_strings(v: &Value) -> Result<(), String> {
    match v {
        Value::String(s) if s.len() > MAX_STR => Err("string too long".into()),
        Value::Array(a) => a.iter().try_for_each(check_strings),
        Value::Object(o) => o.values().try_for_each(check_strings),
        _ => Ok(()),
    }
}

fn walk(rules: &UiRules, node: &Value, depth: usize, count: &mut usize) -> Result<(), String> {
    *count += 1;
    if *count > MAX_NODES {
        return Err(format!("more than {MAX_NODES} nodes"));
    }
    if depth > MAX_DEPTH {
        return Err(format!("deeper than {MAX_DEPTH}"));
    }
    let o = node.as_object().ok_or("node must be an object")?;
    let ty = o
        .get("type")
        .and_then(Value::as_str)
        .ok_or("node needs a type")?;
    if !NODE_TYPES.contains(&ty) {
        return Err(format!("unknown node type {ty}"));
    }
    for (k, v) in o {
        match k.as_str() {
            "type" => {}
            k if NODE_LIST_KEYS.contains(&k) => {
                for c in v.as_array().ok_or(format!("{k} must be an array"))? {
                    walk(rules, c, depth + 1, count)?;
                }
            }
            k if NODE_KEYS.contains(&k) => walk(rules, v, depth + 1, count)?,
            "columns" => {
                for c in v.as_array().ok_or("columns must be an array")? {
                    let c = c.as_object().ok_or("column must be an object")?;
                    if let Some(cell) = c.get("cell") {
                        walk(rules, cell, depth + 1, count)?;
                    }
                    check_strings(&Value::Object(c.clone()))?;
                }
            }
            "action" | "submit" => validate_action(rules, v)?,
            "href" | "src" => {
                let s = v.as_str().ok_or(format!("{k} must be a string"))?;
                let ok = if ty == "frame" {
                    asset_path_ok(s)
                } else if ty == "embed" {
                    s.starts_with("https://") && s.len() <= 500 && !s.contains(char::is_whitespace)
                } else {
                    safe_templated_path(s)
                };
                if !ok {
                    return Err(format!("unsafe {k} {s}"));
                }
            }
            _ => check_strings(v)?,
        }
    }
    if ty == "frame" {
        let src = o.get("src").and_then(Value::as_str).unwrap_or("");
        let height = o.get("height").and_then(Value::as_u64).map(|h| h as u32);
        validate_frame(
            rules,
            &Frame {
                src: src.into(),
                height,
            },
        )?;
    }
    if ty == "embed" && !rules.allow_embed {
        return Err("embed nodes need the ui.embed capability".into());
    }
    if ty == "tabs"
        && o.get("children")
            .and_then(Value::as_array)
            .is_some_and(|c| c.iter().any(|t| t["type"] != "tab"))
    {
        return Err("tabs children must be tab nodes".into());
    }
    Ok(())
}

pub fn validate_page(rules: &UiRules, p: &UiPage) -> Result<(), String> {
    let ns = format!("plugin.{}.", rules.plugin_id);
    if !crate::valid_id(&p.id) {
        return Err(format!("bad page id {}", p.id));
    }
    if p.area != "admin" && p.area != "client" {
        return Err(format!("page area must be admin or client, got {}", p.area));
    }
    if !p.title.starts_with(&ns) {
        return Err(format!("page title {} must be in {ns}*", p.title));
    }
    if p.icon
        .as_ref()
        .is_some_and(|i| i.len() > 40 || !i.chars().all(|c| c.is_ascii_alphanumeric()))
    {
        return Err("bad icon name".into());
    }
    if p.hub.as_deref().is_some_and(|h| !key_ok(h)) {
        return Err("bad hub id".into());
    }
    validate_data(rules, &p.data)?;
    validate_content(rules, &p.body, &p.frame)
}

pub fn validate_slot(rules: &UiRules, s: &UiSlotContribution) -> Result<(), String> {
    if !valid_slot(&s.slot) {
        return Err(format!("unknown render slot {}", s.slot));
    }
    validate_data(rules, &s.data)?;
    validate_content(rules, &s.body, &s.frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(embed: bool, core: bool) -> UiRules<'static> {
        UiRules {
            plugin_id: "demo",
            allow_embed: embed,
            allow_core_actions: core,
            own_api: None,
            allow_iframe: false,
        }
    }

    #[test]
    fn named_reply_checkout_slots() {
        for n in [
            "admin.ticket.reply",
            "client.ticket.reply",
            "client.checkout.step",
            "client.invoice.detail",
            "client.service.detail",
        ] {
            assert!(valid_slot(n), "{n}");
        }
    }

    #[test]
    fn generic_slots() {
        for ok in [
            "admin.dashboard",
            "admin.invoices.toolbar",
            "client.*.before",
            "admin.settings-tax.replace:summary",
            "admin.layout.banner",
            "client.layout.sidebar",
            "admin.services.row-actions",
            "admin.customers.tabs",
            "admin.orders.detail-panel",
            "client.invoices.before:summary",
            "admin.dashboard.after:stats",
        ] {
            assert!(valid_slot(ok), "{ok}");
        }
        for bad in [
            "root.x.before",
            "admin.layout.before",
            "admin.Invoices.after",
            "admin.x.sideways",
            "admin.x.replace:",
            "admin.x",
            "admin..after",
            "admin.x.wrap:y",
            "admin.x.before:",
        ] {
            assert!(!valid_slot(bad), "{bad}");
        }
    }

    #[test]
    fn frames_need_capability_and_assets() {
        let mut rules = r(false, false);
        let page = |frame: Value| -> UiPage {
            serde_json::from_value(
                json!({"id": "page", "area": "admin", "title": "plugin.demo.t", "frame": frame}),
            )
            .unwrap()
        };
        assert!(validate_page(&rules, &page(json!({"src": "assets/ui.html"}))).is_err());
        rules.allow_iframe = true;
        validate_page(
            &rules,
            &page(json!({"src": "assets/ui/index.html", "height": 300})),
        )
        .unwrap();
        for bad in [
            json!({"src": "addon.wasm"}),
            json!({"src": "assets/../addon.json"}),
            json!({"src": "assets/x.js"}),
            json!({"src": "https://x/y.html"}),
        ] {
            assert!(validate_page(&rules, &page(bad.clone())).is_err(), "{bad}");
        }
        // body and frame are exclusive; one is required.
        let both: UiPage = serde_json::from_value(json!({"id": "page", "area": "admin", "title": "plugin.demo.t", "body": {"type": "text"}, "frame": {"src": "assets/a.html"}})).unwrap();
        assert!(validate_page(&rules, &both).is_err());
        let none: UiPage = serde_json::from_value(
            json!({"id": "page", "area": "admin", "title": "plugin.demo.t"}),
        )
        .unwrap();
        assert!(validate_page(&rules, &none).is_err());
        validate_node(
            &rules,
            &json!({"type": "card", "children": [{"type": "frame", "src": "assets/w.html"}]}),
        )
        .unwrap();
        assert!(validate_node(
            &r(false, false),
            &json!({"type": "frame", "src": "assets/w.html"})
        )
        .is_err());
    }

    #[test]
    fn accepts_rich_tree() {
        let n = json!({"type": "stack", "children": [
            {"type": "heading", "text": "{{t:plugin.demo.title}}"},
            {"type": "table", "rows": "{{data.items.items}}", "columns": [
                {"header": "plugin.demo.name", "value": "{{row.name}}"},
                {"header": "", "cell": {"type": "button", "label": "plugin.demo.go", "action": {"type": "request", "path": "/api/v1/plugins/demo/x/{{row.id}}", "then": {"type": "refresh"}}}}
            ], "empty": {"type": "empty", "title": "plugin.demo.none"}},
            {"type": "tabs", "children": [{"type": "tab", "label": "a", "children": [{"type": "text", "text": "x"}]}]},
            {"type": "link", "href": "/admin/p/demo/other", "label": "plugin.demo.more"}
        ]});
        validate_node(&r(false, false), &n).unwrap();
    }

    #[test]
    fn rejects_unsafe() {
        let bad = [
            json!({"type": "script"}),
            json!({"type": "link", "href": "https://evil.example"}),
            json!({"type": "link", "href": "/a/../../x"}),
            json!({"type": "embed", "src": "https://x.example"}),
            json!({"type": "button", "action": {"type": "request", "path": "/api/v1/admin/users/1"}}),
            json!({"type": "button", "action": {"type": "request", "path": "/api/v1/plugins/other/x"}}),
            json!({"type": "button", "action": {"type": "navigate", "to": "javascript:alert(1)"}}),
            json!({"type": "tabs", "children": [{"type": "text"}]}),
            json!({"type": "stack", "children": [{"type": "text", "text": "x".repeat(9000)}]}),
        ];
        for b in bad {
            assert!(validate_node(&r(false, false), &b).is_err(), "{b}");
        }
        // Capabilities unlock embed and core actions.
        validate_node(
            &r(true, false),
            &json!({"type": "embed", "src": "https://x.example/p"}),
        )
        .unwrap();
        validate_node(&r(false, true), &json!({"type": "button", "action": {"type": "request", "path": "/api/v1/admin/services/{{ctx.service_id}}/module"}})).unwrap();
        assert!(validate_node(
            &r(true, true),
            &json!({"type": "embed", "src": "http://x.example"})
        )
        .is_err());
    }

    #[test]
    fn limits_size_and_depth() {
        let mut n = json!({"type": "text"});
        for _ in 0..40 {
            n = json!({"type": "stack", "children": [n]});
        }
        assert!(validate_node(&r(false, false), &n)
            .unwrap_err()
            .contains("deeper"));
        let wide =
            json!({"type": "stack", "children": vec![json!({"type": "text"}); MAX_NODES + 1]});
        assert!(validate_node(&r(false, false), &wide).is_err());
    }

    #[test]
    fn templated_paths() {
        assert!(safe_templated_path(
            "/api/v1/admin/services/{{ctx.service_id}}/provisioning"
        ));
        assert!(safe_templated_path(
            "/api/v1/plugins/demo/list?page=1&per_page=20"
        ));
        assert!(!safe_templated_path("/api/v1/x/{{ctx.a b}}"));
        assert!(!safe_templated_path("//evil.example"));
        assert!(!safe_templated_path("/x/{{unclosed"));
    }
}
