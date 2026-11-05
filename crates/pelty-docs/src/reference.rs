//! Events index, "cancel and modify" reference, widgets/slots index and
//! hook-points index. Everything here is derived from the event registry,
//! the emit sites in the source, the router and the frontend.
use crate::doc::{esc, slugify, Block, Page};
use crate::slots;
use pelty_extensions::events::{EventKind, EventSpec, REGISTRY};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn c(s: &str) -> String {
    format!("`{s}`")
}

fn example_value(ty: &str, name: &str) -> Value {
    match ty {
        "uuid" => json!("0190f7a2-6c1e-7b3a-9d2e-4f5a6b7c8d9e"),
        "decimal" => json!("12.50"),
        "int" => json!(3),
        "bool" => json!(true),
        "array" => json!([]),
        "object" => json!({}),
        _ => json!(match name {
            "currency" => "EUR",
            "email" => "jane@example.com",
            "ip" => "203.0.113.7",
            "gateway" => "stripe",
            _ => "…",
        }),
    }
}

pub fn example_payload(e: &EventSpec) -> Value {
    Value::Object(
        e.payload
            .iter()
            .map(|f| (f.name.to_string(), example_value(f.ty, f.name)))
            .collect(),
    )
}

/// Listener code for one event (Rust SDK).
fn listener(e: &EventSpec) -> (String, String) {
    let first = e.payload.first().map(|f| f.name).unwrap_or("id");
    match e.kind {
        EventKind::Domain => (
            serde_json::to_string_pretty(&json!({ "capabilities": { "events": [e.name], "hooks": [{ "event": format!("finished:{}", e.name), "priority": "monitor" }] } })).unwrap(),
            format!(
                "// on-event: delivered after the action was committed (cannot be cancelled).\nfn on_event(name: &str, payload: &Value) {{\n    if name == \"{n}\" {{\n        let {v} = payload[\"{first}\"].as_str().unwrap_or_default();\n        let _ = host::kv_set(\"last:{n}\", {v});\n    }}\n}}",
                n = e.name,
                v = first.replace('.', "_"),
            ),
        ),
        EventKind::Pre => {
            let modify = match e.modify.first() {
                Some((k, _)) if k.contains("[]") => {
                    let (arr, key) = k.split_once("[].").unwrap();
                    format!("    // Modify: rewrite `{key}` on every `{arr}` element.\n    if let Some(lines) = p[\"{arr}\"].as_array_mut() {{\n        for l in lines {{\n            l[\"{key}\"] = json!(l[\"{key}\"].as_str().unwrap_or(\"\").trim());\n        }}\n    }}\n    reply::modify(p)")
                }
                Some((k, _)) => format!("    // Modify: only the keys listed under \"Modifiable keys\" are read back.\n    p[\"{k}\"] = json!(p[\"{k}\"].as_str().unwrap_or(\"\").trim());\n    reply::modify(p)"),
                None => "    reply::proceed() // this event is read-only: a modify is ignored".into(),
            };
            (
                serde_json::to_string_pretty(&json!({ "capabilities": { "hooks": [{ "event": e.name, "priority": "normal", "fail_open": true, "timeout_ms": 500 }] } })).unwrap(),
                format!(
                    "// op `hook`, phase `before`: return proceed, modify or cancel.\nfn hook(input: &Value) -> Value {{\n    if input[\"event\"] != \"{n}\" || input[\"cancelled\"] == true {{\n        return reply::proceed();\n    }}\n    let mut p = input[\"payload\"].clone();\n    // Cancel: the caller gets HTTP 409 `error.addon_hook_cancelled`.\n    if p[\"{first}\"].as_str().is_some_and(|v| v.ends_with(\".invalid\")) {{\n        return reply::cancel(\"refused by my-addon\");\n    }}\n{modify}\n}}",
                    n = e.name,
                ),
            )
        }
    }
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn cat_title(c: &str) -> String {
    let mut s = c.replace('_', " ");
    if let Some(f) = s.get(..1) {
        s = f.to_uppercase() + &s[1..];
    }
    s
}

pub fn categories() -> BTreeMap<&'static str, Vec<&'static EventSpec>> {
    let mut m: BTreeMap<&str, Vec<&EventSpec>> = BTreeMap::new();
    for e in REGISTRY {
        m.entry(e.category()).or_default().push(e);
    }
    m
}

/// `events.html`: index (categories, filter, table) and one section per event.
pub fn events(sites: &BTreeMap<String, Vec<String>>) -> Page {
    let mut p = Page::new("events", "Events index", "Reference");
    let pre = REGISTRY.iter().filter(|e| e.kind == EventKind::Pre).count();
    let modi = REGISTRY.iter().filter(|e| e.modifiable).count();
    p.p(format!(
        "All {} core events: {} domain events (published after the fact, observe only) and {} pre-events (run before the action; {} can be cancelled, {} can be modified). On top of these, **every mutating API request** is a cancellable, modifiable event, see [Cancel and modify](cancellable.md). Rendered from `pelty_extensions::events::REGISTRY`; the test `crates/pelty-api/tests/integration/event_registry_coverage.rs` fails when the source emits an event or payload key missing here. Machine-readable: [events.json](events.json).",
        REGISTRY.len(), REGISTRY.len() - pre, pre, pre, modi
    ));
    p.h(2, "How events work");
    p.list(vec![
        "**Domain events** (`invoice.paid`, …) are published after the action committed. Addons receive them through `on-event` when listed in `capabilities.events`, and as `after:<event>` / `finished:<event>` hooks. Webhook plugins receive them as signed HTTPS POSTs when listed in the manifest `hooks`. They cannot be cancelled or changed.".into(),
        "**Pre-events** (`order.before_place`, …) run synchronously in the `before` phase, in priority order. Each listener may continue, modify the payload, or cancel. A cancel answers the API call with HTTP 409 `error.addon_hook_cancelled`.".into(),
        "**Request events** `<METHOD> <route>` exist for every POST, PUT, PATCH and DELETE route, with all four phases. See [Cancel and modify](cancellable.md).".into(),
        "**Custom events** `<addon-id>:<name>` are emitted by addons (`capabilities.emits`) and have the same phases; listeners can cancel or modify them in `before`.".into(),
        "Listeners run lowest priority first (`lowest` 0, `low` 50, `normal` 100, `high` 150, `highest` 200, or any integer -1000..1000), ties by addon id. `monitor` (1000) runs last and only observes. See [Hooks and mixins](hooks.md).".into(),
    ]);
    p.h(2, "Categories");
    let cats = categories();
    let mut html = String::from("<div class=\"cats\">");
    let mut md = String::new();
    for (k, v) in &cats {
        html.push_str(&format!("<a class=\"cat\" href=\"#cat-{k}\" data-cat=\"{k}\"><strong>{}</strong><span>{} event{}</span></a>", esc(&cat_title(k)), v.len(), if v.len() == 1 { "" } else { "s" }));
        md.push_str(&format!("- [{}](#cat-{k}): {}\n", cat_title(k), v.len()));
    }
    html.push_str("</div>");
    p.blocks.push(Block::Raw(html, md));
    p.h(2, "All events");
    // Filterable index table.
    let mut html = String::from("<div class=\"filters\"><input id=\"evq\" type=\"search\" placeholder=\"Filter events, e.g. invoice or customer_id\" aria-label=\"Filter events\"><label><input type=\"checkbox\" id=\"evcancel\"> Cancellable only</label><span id=\"evcount\" class=\"src\"></span></div><div class=\"tw\"><table id=\"evtable\"><thead><tr><th>Event</th><th>Category</th><th>Phases</th><th>Cancel</th><th>Modify</th><th>Payload</th></tr></thead><tbody>");
    let mut md = String::from(
        "| Event | Category | Phases | Cancel | Modify | Payload |\n|---|---|---|---|---|---|\n",
    );
    for e in REGISTRY {
        let keys: Vec<String> = e.payload.iter().map(|f| f.name.to_string()).collect();
        html.push_str(&format!(
            "<tr data-cancel=\"{}\"><td><a href=\"#{}\"><code>{}</code></a></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            e.cancellable,
            slugify(e.name),
            e.name,
            esc(&cat_title(e.category())),
            e.phases().join(", "),
            if e.cancellable { "<span class=\"pill ok\">yes</span>" } else { "<span class=\"pill\">no</span>" },
            if e.modifiable { "<span class=\"pill ok\">yes</span>" } else { "<span class=\"pill\">no</span>" },
            keys.iter().map(|k| format!("<code>{k}</code>")).collect::<Vec<_>>().join(" ")
        ));
        md.push_str(&format!(
            "| [`{}`](#{}) | {} | {} | {} | {} | {} |\n",
            e.name,
            slugify(e.name),
            cat_title(e.category()),
            e.phases().join(", "),
            yes(e.cancellable),
            yes(e.modifiable),
            keys.join(", ")
        ));
    }
    html.push_str("</tbody></table></div>");
    p.blocks.push(Block::Raw(html, md));
    for (k, v) in &cats {
        p.blocks.push(Block::Raw(
            format!(
                "<h2 id=\"cat-{k}\"><a class=\"anchor\" href=\"#cat-{k}\">#</a>{}</h2>\n",
                esc(&cat_title(k))
            ),
            format!("## {}\n", cat_title(k)),
        ));
        for e in v {
            p.h(3, e.name);
            p.p(e.description);
            p.table(
                &["Kind", "Phases", "Cancellable", "Modifiable", "Since"],
                vec![vec![
                    if e.kind == EventKind::Pre {
                        "pre-event".into()
                    } else {
                        "domain event".into()
                    },
                    e.phases()
                        .iter()
                        .map(|x| c(x))
                        .collect::<Vec<_>>()
                        .join(", "),
                    yes(e.cancellable).into(),
                    yes(e.modifiable).into(),
                    e.since.into(),
                ]],
            );
            p.p("**Payload**");
            p.table(
                &["Key", "Type", "Always present", "Description"],
                e.payload
                    .iter()
                    .map(|f| {
                        vec![
                            c(f.name),
                            c(f.ty),
                            yes(f.always).into(),
                            f.description.into(),
                        ]
                    })
                    .collect(),
            );
            if e.modifiable {
                p.p("**Modifiable keys** (everything else in a `modify` is ignored)");
                p.table(
                    &["Key", "Rule"],
                    e.modify
                        .iter()
                        .map(|(k, r)| vec![c(k), (*r).into()])
                        .collect(),
                );
            } else if e.kind == EventKind::Pre {
                p.p("**Modifiable keys:** none. A `modify` reply is accepted but the host does not read anything back.");
            }
            match sites.get(e.name) {
                Some(s) => p.p(format!(
                    "**Emitted from:** {}.",
                    s.iter().map(|x| c(x)).collect::<Vec<_>>().join(", ")
                )),
                None => p.p("**Emitted from:** not found by the source scanner."),
            };
            p.code(
                "json",
                serde_json::to_string_pretty(&example_payload(e)).unwrap(),
            );
            let (manifest, code) = listener(e);
            p.code("json", manifest);
            p.code("rust", code);
        }
    }
    p.h(2, "Custom addon events");
    p.p("Named `<addon-id>:<event>`, declared in `capabilities.emits` and emitted with `pelty_addon_sdk::emit`. They are not in the registry because addons define them. Other addons listen with `before:<id>:<event>` (cancel or modify), `after:` or `finished:` in `hooks`. An emit inside a listener nests; the depth is limited to 4. The emitter gets `{cancelled, reason, payload}` back. See [Events and hooks](events-guide.md) for a full example.");
    p.h(2, "Lifecycle phases");
    p.blocks.extend(crate::pages::phases_blocks());
    p.h(2, "Webhook plugins");
    p.p("A webhook plugin lists domain events in its manifest `hooks`. Each one is POSTed to `webhook_url`, signed, with the payload above. Delivery attempts are listed under Admin, Plugins, Deliveries.");
    crate::pages::fields_table(
        &mut p,
        "crates/pelty-extensions/src/plugins.rs",
        "PluginManifest",
    );
    p.code("json", serde_json::to_string_pretty(&json!({ "id": "my-plugin", "name": "My plugin", "version": "1.0.0", "api_version": 1, "hooks": ["invoice.paid", "order.created"], "webhook_url": "https://example.com/pelty" })).unwrap());
    p
}

/// `cancellable.html`: every action an addon can stop or change.
pub fn cancellable(spec: &Value) -> Page {
    let mut p = Page::new("cancellable", "Cancel and modify", "Reference");
    p.p("Everything an addon can stop or change before it happens, and exactly what it may change. There are three kinds: typed pre-events, request events on every mutating API route, and mixins.");
    p.h(2, "Rules");
    p.list(vec![
        "Only the `before` phase can cancel or modify. `during`, `after` and `finished` only observe (the reply is ignored).".into(),
        "Listeners run in priority order. Each sees the payload as changed by the ones before it, and `input.cancelled` tells it whether an earlier listener cancelled.".into(),
        "After a cancel, later listeners are skipped unless their hook sets `ignore_cancelled: true`. Only those may reply `uncancel`, which clears the cancel.".into(),
        "`monitor` listeners always run last and cannot change anything; a non-continue reply is logged and ignored.".into(),
        "A cancel answers the API call with HTTP 409 and `error.addon_hook_cancelled`. The reason (at most 200 characters) is logged, not shown to the user.".into(),
        "If a hook errors or times out (`timeout_ms`, default 500, capped by the sandbox), `fail_open: true` (default) continues and `fail_open: false` blocks the action with 409 `error.addon_hook_failed`.".into(),
        "A `modify` must be a JSON object. The host reads back only the keys listed for the event; other keys are ignored.".into(),
        format!("Money keys ({}) change only for addons with `capabilities.money.modify`, must be exact decimal strings (at most 4 places), and every change is audited as `addon.money_modified`. Without the capability the money keys are reverted and the rest of the change is kept.", pelty_api::addon_ext::MONEY_KEYS.iter().map(|x| c(x)).collect::<Vec<_>>().join(", ")),
        format!("Secrets never reach addons: {} and any key ending in `_password`, `_secret` or `_token` are replaced by `\"[redacted]\"` and restored afterwards, so they can be neither read nor changed.", pelty_api::addon_ext::REDACT_KEYS.iter().map(|x| c(x)).collect::<Vec<_>>().join(", ")),
    ]);
    p.h(2, "Reply format");
    p.code("json", "{\"action\":\"continue\"}\n{\"action\":\"modify\",\"payload\":{ \"coupon\": \"SUMMER\" }}\n{\"action\":\"cancel\",\"reason\":\"not allowed\"}\n{\"action\":\"uncancel\"}");
    p.p("The Rust SDK builds these with `reply::proceed()`, `reply::modify(p)`, `reply::cancel(reason)` and `reply::uncancel()`.");
    p.h(2, "Typed pre-events");
    p.table(
        &["Event", "Cancel", "Modifiable keys and rules", "Payload"],
        REGISTRY
            .iter()
            .filter(|e| e.kind == EventKind::Pre)
            .map(|e| {
                vec![
                    format!("[`{}`](events.md#{})", e.name, slugify(e.name)),
                    yes(e.cancellable).into(),
                    if e.modify.is_empty() {
                        "none (read-only)".into()
                    } else {
                        e.modify
                            .iter()
                            .map(|(k, r)| format!("`{k}`: {r}"))
                            .collect::<Vec<_>>()
                            .join("; ")
                    },
                    e.payload
                        .iter()
                        .map(|f| c(f.name))
                        .collect::<Vec<_>>()
                        .join(", "),
                ]
            })
            .collect(),
    );
    p.h(3, "Example: cancel, modify and uncancel");
    p.blocks.extend(crate::pages::snippet_blocks("hooks"));
    p.blocks.extend(crate::pages::snippet_blocks("uncancel"));
    p.h(2, "Request events (every mutating route)");
    p.p("Every POST, PUT, PATCH and DELETE under `/api/v1` (except addon routes) is an event named `<METHOD> <route template>`. A hook `before:<METHOD> <route>` gets `{method, path, route, query, body}` and may cancel the request or rewrite `body` (the JSON request body, at most 1 MiB, money and secret rules apply). Patterns: `before:*` (every request), `before:POST /api/v1/admin/*` (prefix), or an exact template. Because they see every request, these hooks are a dangerous capability (`hooks.any`) the admin must approve at install. `during`, `after` (with the JSON response) and `finished` (with `status` and `http_status`) observe.");
    p.code("json", serde_json::to_string_pretty(&json!({ "capabilities": { "hooks": [{ "event": "before:POST /api/v1/client/tickets", "priority": "high" }, { "event": "after:PUT /api/v1/client/profile", "priority": "monitor" }] } })).unwrap());
    let mut rows = vec![];
    for (path, item) in spec["paths"].as_object().unwrap() {
        if path.starts_with("/api/v1/addons/") || !path.starts_with("/api/v1/") {
            continue;
        }
        for (m, op) in item.as_object().unwrap() {
            if !["post", "put", "patch", "delete"].contains(&m.as_str()) || !op.is_object() {
                continue;
            }
            let summary = op["summary"]
                .as_str()
                .or(op["description"].as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            rows.push(vec![
                format!("`before:{} {path}`", m.to_uppercase()),
                summary,
            ]);
        }
    }
    p.p(format!("{} cancellable request events:", rows.len()));
    p.table(&["Hook event", "Action"], rows);
    p.h(2, "Mixins");
    p.p("A mixin wraps one core route under `/api/v1` (any method, `GET` too; not `/api/v1/auth/*` or addon routes): `before` rewrites the request body, `after` rewrites the JSON response (only values of keys it already has), `around` does both with shared `state`, and `return` replaces the response with one of the same shape. Mixins need admin approval (`mixins`, `mixins.return`) and can be switched off per addon. See [Hooks and mixins](hooks.md).");
    p.blocks.extend(crate::pages::snippet_blocks("mixin"));
    p
}

/// `widgets.html`: every place an addon can put UI.
pub fn widgets() -> Page {
    use pelty_extensions::ui_schema as u;
    let mut p = Page::new("widgets", "Widgets and slots", "Reference");
    let pages = slots::pages();
    let named = slots::named();
    let n_slots: usize = pages
        .iter()
        .map(|x| u::PAGE_POSITIONS.len() - usize::from(!x.detail))
        .sum::<usize>()
        + 2 * u::LAYOUT_POSITIONS.len()
        + named.len();
    p.p(format!(
        "Every place where an addon can inject UI: {} admin pages and {} client pages, each with its own page slots, {} layout slots and {} hand-placed named slots ({} slot names in total, wildcards not counted). Generated from `frontend/src/App.tsx`, `frontend/src/layouts/Shell.tsx` and the `<PluginSlot>` / `<PluginWidgets>` calls; a test fails when the frontend renders a position the docs do not list.",
        pages.iter().filter(|x| x.area == "admin").count(),
        pages.iter().filter(|x| x.area == "client").count(),
        2 * u::LAYOUT_POSITIONS.len(),
        named.len(),
        n_slots
    ));
    p.h(2, "Two ways to render");
    p.list(vec![
        "**Native** (default): `body` is a JSON node tree (cards, tables, stats, forms, …) rendered with the panel's own components and theme. No addon JavaScript runs. Node reference: [UI schema](ui.md).".into(),
        "**Iframe**: `frame: {src, height}` loads an HTML file from the addon's `assets/` in a sandboxed iframe (opaque origin, strict CSP). Needs `\"iframe\"` in `capabilities.ui`, approved by the admin at install. The frame talks to the panel through `postMessage`: `pelty:ready`, `pelty:resize`, `pelty:request` (own routes, plus GET on approved `bridge` prefixes), `pelty:navigate`, `pelty:toast`, plus the event bus (`pelty:subscribe`, `pelty:emit`; host events arrive as `pelty:event`). `pelty:init` carries the theme tokens, locale, page context and the viewer (`id`, `display_name`, `locale`, `staff`, `permissions`; never the email). The typed, dependency-free frame SDK in `extensions/sdk/addon-ui` (`connect()` → `api.get/post/…`, `on`, `emit`, `navigate`, `toast`, `autoResize`, `t`, `applyTheme`) wraps the protocol.".into(),
    ]);
    p.h(2, "Frontend event bus");
    p.p("The panel publishes host events on an in-page bus; frames subscribe with wildcards (`record.*`, `*`). An event a frame emits is published as `<addon-id>:<name>`, so addons can talk to each other and to their own slots on the same page.");
    p.table(
        &["Event", "Payload", "When"],
        vec![
            vec![
                c("route.changed"),
                c("{area, path, key}"),
                "Every navigation".into(),
            ],
            vec![
                c("record.viewed"),
                c("{area, kind, id}"),
                "A detail page opens (`kind` = route key)".into(),
            ],
            vec![
                c("ui.theme_changed"),
                c("{dark}"),
                "Light/dark switch".into(),
            ],
            vec![
                c("ui.locale_changed"),
                c("{locale}"),
                "Language switch".into(),
            ],
            vec![
                c("data.changed"),
                c("{key}"),
                "A panel mutation succeeded (`key` = first query key)".into(),
            ],
        ],
    );
    p.p("Frontend filters (`nav.items`, `page.title`) let in-page code rewrite the sidebar's addon entries and the document title; a throwing or wrongly typed filter is skipped. Server-side value filters (`ui.contributions`, `client.dashboard`, …) are on [Hooks](hooks.md#filters).");
    p.h(2, "Overriding core pages and the menu");
    p.p("With `\"override\"` in `capabilities.ui` (shown to the admin as a dangerous permission at install), `ui.overrides` replaces whole core pages with one of the addon's own pages and hides core sidebar entries. The replaced page receives the route context (`ctx.id` on detail pages). When two addons override the same route, the first by addon id wins; the API lists the others under `overrides.conflicts`. Opening a page with `?core=1` always shows the core page, so a broken override can't lock anyone out. Without the capability, `ui.overrides` is ignored.");
    p.code("json", serde_json::to_string_pretty(&json!({ "capabilities": { "ui": ["iframe", "override"] }, "ui": {
        "pages": [{ "id": "my-invoices", "area": "client", "title": "Invoices", "nav": false, "frame": { "src": "assets/invoices.html" } }],
        "overrides": { "pages": { "client.invoices": "my-invoices" }, "hide_nav": ["/billing/quotes"] }
    } })).unwrap());
    p.h(2, "Registering a slot contribution");
    p.p("List contributions in `addon.json` `ui.slots`. `order` sorts contributions in the same slot (lower first). Text that starts with `plugin.<addon-id>.` is translated from the manifest `i18n`. Page context is available as `{{ctx.*}}`.");
    p.code("json", serde_json::to_string_pretty(&json!({ "ui": { "slots": [
        { "slot": "admin.invoices.toolbar", "order": 5, "body": { "type": "badge", "text": "{{t:plugin.my-addon.badge}}" } },
        { "slot": "admin.customers.detail-panel", "body": { "type": "card", "children": [{ "type": "stat", "label": "plugin.my-addon.score", "value": "{{data.score.value}}" }] }, "data": { "score": { "get": "/api/v1/addons/my-addon/score?customer={{ctx.id}}" } } },
        { "slot": "client.layout.banner", "frame": { "src": "assets/banner.html", "height": 64 } }
    ] } })).unwrap());
    p.p("The example addon's real manifest is on [Addon UI](ui-guide.md).");
    p.h(2, "Slot names");
    p.p(format!("Page slots are `<area>.<route-key>.<position>`, with `area` = `admin` or `client`. Use `*` as route key for every page (`admin.*.toolbar`). Positions: {}. Layout slots are `<area>.layout.<position>` with {}.", u::PAGE_POSITIONS.iter().map(|x| c(x)).collect::<Vec<_>>().join(", "), u::LAYOUT_POSITIONS.iter().map(|x| c(x)).collect::<Vec<_>>().join(", ")));
    p.table(&["Position", "Where it renders", "Context"], vec![
        vec![c("before"), "Above the page content, below the layout banner".into(), "`ctx.route`, `ctx.id` on detail pages".into()],
        vec![c("toolbar"), "In the page header, next to the page's own buttons".into(), "same".into()],
        vec![c("tabs"), "Extra tabs after the page; `body.label` is the tab title".into(), "same".into()],
        vec![c("detail-panel"), "Cards below the content, only on pages with an id".into(), "same".into()],
        vec![c("after"), "Below the page content".into(), "same".into()],
        vec![c("row-actions"), "An extra actions column on the page's tables".into(), "`ctx.row_id`, `row`".into()],
        vec![c("replace:<section>"), "Replaces a section wrapped in `<Replaceable section>` (the first contribution by `order` wins)".into(), "same".into()],
        vec![c("before:<section>"), "Above a `<Replaceable section>` (kept when the section is replaced)".into(), "same".into()],
        vec![c("after:<section>"), "Below a `<Replaceable section>`".into(), "same".into()],
        vec![c("layout.header"), "Top bar, before the search (desktop)".into(), "`ctx.route`".into()],
        vec![c("layout.sidebar"), "Bottom of the sidebar".into(), "`ctx.route`".into()],
        vec![c("layout.banner"), "Above every page".into(), "`ctx.route`".into()],
        vec![c("layout.footer"), "Below every page".into(), "`ctx.route`".into()],
    ]);
    let repl = regex::Regex::new(r#"<Replaceable section="([a-z0-9-]+)""#).unwrap();
    let mut sections = vec![];
    for (rel, s) in crate::extract::frontend_sources() {
        for c in repl.captures_iter(&s) {
            sections.push(format!("`{}` ({rel})", &c[1]));
        }
    }
    p.p(if sections.is_empty() {
        "Replaceable sections: none are wrapped yet, so section slots currently render nothing."
            .to_string()
    } else {
        format!(
            "Replaceable sections (`replace:`, `before:`, `after:`): {}.",
            sections.join(", ")
        )
    });
    p.h(2, "Named slots");
    p.p("Fixed slots placed on specific pages. `render` slots take a node tree or frame (`ui.slots`). `widget` and `nav` slots take stat cards and menu links from webhook plugins (`ui.widgets`, `ui.menu`).");
    p.table(
        &["Slot", "Kind", "Context", "Rendered in"],
        named
            .iter()
            .map(|n| {
                vec![
                    c(&n.slot),
                    n.kind.into(),
                    n.ctx
                        .iter()
                        .map(|k| c(&format!("ctx.{k}")))
                        .collect::<Vec<_>>()
                        .join(", "),
                    n.files.iter().map(|f| c(f)).collect::<Vec<_>>().join(", "),
                ]
            })
            .collect(),
    );
    for (area, title) in [("admin", "Admin pages"), ("client", "Client area pages")] {
        p.h(2, title);
        let rows = pages
            .iter()
            .filter(|x| x.area == area)
            .map(|x| {
                let pos: Vec<String> = u::PAGE_POSITIONS
                    .iter()
                    .filter(|q| x.detail || **q != "detail-panel")
                    .map(|q| format!("`{}.{}.{q}`", x.area, x.key))
                    .collect();
                vec![c(&x.path), c(&x.key), x.component.clone(), pos.join(" ")]
            })
            .collect();
        p.table(&["Page", "Route key", "Component", "Slots"], rows);
    }
    p.p("A key with `<param>` contains the actual value of that URL segment (`/admin/hub/system` → `hub-system`). Ids (numbers, UUIDs, long hex, `ABC-123`) are always dropped.");
    p.h(2, "Example: native widget and iframe widget");
    p.code("json", serde_json::to_string_pretty(&json!({ "slot": "client.dashboard", "body": { "type": "alert", "text": "{{t:plugin.integration-guide.hello}}" } })).unwrap());
    p.code("html", "<!-- assets/banner.html -->\n<!doctype html><meta charset=\"utf-8\">\n<p id=\"msg\">Loading…</p>\n<script>\n  const send = (m) => parent.postMessage(m, \"*\");\n  addEventListener(\"message\", (e) => {\n    if (e.data?.type === \"pelty:response\" && e.data.id === 1) document.getElementById(\"msg\").textContent = e.data.body.message;\n  });\n  send({ type: \"pelty:ready\" });\n  send({ type: \"pelty:request\", id: 1, method: \"GET\", path: \"/api/v1/addons/my-addon/hello\" });\n  send({ type: \"pelty:resize\", height: document.body.scrollHeight });\n</script>");
    p
}

/// `hook-points.html`: every target a hook or mixin can name.
pub fn hook_points(spec: &Value) -> Page {
    let mut p = Page::new("hook-points", "Hook points index", "Reference");
    p.p("Every string you can put in `capabilities.hooks[].event`, `capabilities.events` or `capabilities.mixins[].target`, with what it can do.");
    let (mut reads, mut writes) = (0, 0);
    for item in spec["paths"].as_object().unwrap().values() {
        for (m, op) in item.as_object().unwrap() {
            if op.is_object() {
                if ["post", "put", "patch", "delete"].contains(&m.as_str()) {
                    writes += 1
                } else {
                    reads += 1
                }
            }
        }
    }
    let pre = REGISTRY.iter().filter(|e| e.kind == EventKind::Pre).count();
    p.table(
        &["Target", "Where", "Count", "Cancel", "Modify", "Docs"],
        vec![
            vec![
                "`<pre-event>`, e.g. `order.before_place`".into(),
                "`hooks`".into(),
                pre.to_string(),
                "yes".into(),
                "listed keys".into(),
                "[Cancel and modify](cancellable.md)".into(),
            ],
            vec![
                "`<domain-event>`".into(),
                "`events` (on-event)".into(),
                (REGISTRY.len() - pre).to_string(),
                "no".into(),
                "no".into(),
                "[Events index](events.md)".into(),
            ],
            vec![
                "`after:<domain-event>`, `finished:<domain-event>`".into(),
                "`hooks`".into(),
                (2 * (REGISTRY.len() - pre)).to_string(),
                "no".into(),
                "no".into(),
                "[Events index](events.md)".into(),
            ],
            vec![
                "`before:<METHOD> <route>`, `before:*`".into(),
                "`hooks`".into(),
                writes.to_string(),
                "yes".into(),
                "request body".into(),
                "[Cancel and modify](cancellable.md)".into(),
            ],
            vec![
                "`during:` / `after:` / `finished:<METHOD> <route>`".into(),
                "`hooks`".into(),
                (3 * writes).to_string(),
                "no".into(),
                "no".into(),
                "[Hooks and mixins](hooks.md)".into(),
            ],
            vec![
                "`before:<addon>:<event>`".into(),
                "`hooks`".into(),
                "addon-defined".into(),
                "yes".into(),
                "whole payload".into(),
                "[Events and hooks](events-guide.md)".into(),
            ],
            vec![
                "`<METHOD> <route>` mixin (`before`, `after`, `around`, `return`)".into(),
                "`mixins`".into(),
                (reads + writes).to_string(),
                "`return` replaces".into(),
                "body / response".into(),
                "[Hooks and mixins](hooks.md)".into(),
            ],
            vec![
                "UI slot".into(),
                "`ui.slots`".into(),
                "see page".into(),
                "-".into(),
                "-".into(),
                "[Widgets and slots](widgets.md)".into(),
            ],
        ],
    );
    p.h(2, "Phases");
    p.table(&["Phase", "When", "Can change it", "Payload"], vec![
        vec![c("before"), "Before the action, synchronously, in priority order".into(), "yes: modify, cancel, uncancel".into(), "event payload (requests: `{method, path, route, query, body}`)".into()],
        vec![c("during"), "While the request runs, in the background (requests only)".into(), "no".into(), "`{request}`".into()],
        vec![c("after"), "After success".into(), "no".into(), "domain: the event payload; requests: `{request, http_status, response}`; custom: `{source, payload}`".into()],
        vec![c("finished"), "Always last".into(), "no".into(), "`{status: ok|failed|cancelled, …}`".into()],
    ]);
    p.h(2, "Priorities");
    p.table(
        &["Name", "Value", "Note"],
        vec![
            vec![c("lowest"), "0".into(), "runs first".into()],
            vec![c("low"), "50".into(), "".into()],
            vec![c("normal"), "100".into(), "default".into()],
            vec![c("high"), "150".into(), "".into()],
            vec![c("highest"), "200".into(), "".into()],
            vec![
                c("monitor"),
                pelty_wasm::PRIORITY_MONITOR.to_string(),
                "runs last, observe only".into(),
            ],
        ],
    );
    p.p("Integers from -1000 to 1000 are accepted too.");
    p
}

/// Does the name belong to the registry?
pub fn is_registered(n: &str) -> bool {
    REGISTRY.iter().any(|e| e.name == n)
}
