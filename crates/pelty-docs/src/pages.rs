//! Builds every page. Reference pages are assembled from code; guide pages
//! come from `templates/*.md` with directives that pull code in.
use crate::doc::{esc, inline, parse_template, Block, Page};
use crate::extract::{self, read};
use pelty_extensions::events::{EventKind, REGISTRY};
use serde_json::Value;

pub const GUIDE_SRC: &str = "extensions/examples/addon-guide/src/lib.rs";

fn c(s: &str) -> String {
    format!("`{s}`")
}

pub(crate) fn fields_table(p: &mut Page, file: &str, name: &str) {
    let src = read(file);
    let doc = extract::item_doc(&src, name);
    if !doc.is_empty() {
        p.p(doc);
    }
    let rows = extract::struct_fields(&src, name)
        .into_iter()
        .map(|f| {
            vec![
                c(&f.name),
                c(&f.ty),
                if f.optional {
                    "no".into()
                } else {
                    "yes".into()
                },
                f.doc,
            ]
        })
        .collect();
    p.table(&["Field", "Type", "Required", "Description"], rows);
    p.p(format!("Source: `{file}` (`{name}`)."));
}

fn fields_blocks(file: &str, name: &str) -> Vec<Block> {
    let mut p = Page::new("", "", "");
    fields_table(&mut p, file, name);
    p.blocks
}

/// Guide snippet, with a pointer to its source.
pub(crate) fn snippet_blocks(name: &str) -> Vec<Block> {
    let src = read(GUIDE_SRC);
    let s = extract::snippet(&src, name)
        .unwrap_or_else(|| panic!("snippet {name} missing in {GUIDE_SRC}"));
    vec![Block::Code(
        format!("rust title={GUIDE_SRC} · snippet {name}"),
        s,
    )]
}

fn raw_const(file: &str, name: &str) -> String {
    let s = read(file);
    let i = s
        .find(&format!("const {name}: &str = r#\""))
        .unwrap_or_else(|| panic!("{name} in {file}"));
    let body = &s[i..];
    let body = &body[body.find("r#\"").unwrap() + 3..];
    body[..body.find("\"#").unwrap()].to_string()
}

pub(crate) fn phases_blocks() -> Vec<Block> {
    let src = read("crates/pelty-api/src/addon_host/addon_events.rs");
    let doc = extract::module_doc(&src);
    let mut items = vec![];
    for l in doc.lines() {
        if let Some(i) = l.strip_prefix("- ") {
            items.push(i.to_string());
        } else if l.starts_with("  ") {
            if let Some(last) = items.last_mut() {
                last.push(' ');
                last.push_str(l.trim());
            }
        }
    }
    vec![
        Block::P(format!(
            "Phases in order: {}. Source: `crates/pelty-api/src/addon_host/addon_events.rs`.",
            pelty_wasm::PHASES
                .iter()
                .map(|x| c(x))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        Block::List(items),
    ]
}

fn slots_blocks() -> Vec<Block> {
    use pelty_extensions::ui_schema as u;
    let src = read("crates/pelty-extensions/src/ui_schema.rs");
    vec![
        Block::P(extract::item_doc(&src, "PAGE_POSITIONS")),
        Block::Table(
            vec!["Kind".into(), "Values".into()],
            vec![
                vec![
                    "Page positions".into(),
                    u::PAGE_POSITIONS
                        .iter()
                        .map(|x| c(x))
                        .collect::<Vec<_>>()
                        .join(", "),
                ],
                vec![
                    "Layout positions".into(),
                    u::LAYOUT_POSITIONS
                        .iter()
                        .map(|x| c(x))
                        .collect::<Vec<_>>()
                        .join(", "),
                ],
            ],
        ),
        Block::P(extract::item_doc(&src, "UI_RENDER_SLOTS")),
        Block::Table(
            vec!["Slot".into(), "Context keys".into()],
            u::UI_RENDER_SLOTS
                .iter()
                .map(|(s, k)| {
                    vec![
                        c(s),
                        k.iter()
                            .map(|x| c(&format!("ctx.{x}")))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ]
                })
                .collect(),
        ),
    ]
}

fn capabilities_blocks() -> Vec<Block> {
    let mut p = Page::new("", "", "");
    let rt = "crates/pelty-wasm/src/runtime.rs";
    fields_table(&mut p, rt, "Capabilities");
    for s in ["Hook", "Mixin", "Route", "Job", "MoneyCaps", "DbCaps"] {
        p.h(3, s);
        fields_table(&mut p, rt, s);
    }
    p.h(3, "Dangerous capabilities");
    p.code("rust", extract::item_source(&read(rt), "pub fn dangerous"));
    p.blocks
}

fn permissions_blocks() -> Vec<Block> {
    vec![
        Block::P(
            "Coarse package permissions in `addon.json` `permissions` (shown at install):".into(),
        ),
        Block::List(
            pelty_api::addons::PERMISSIONS
                .iter()
                .map(|x| c(x))
                .collect(),
        ),
        Block::P(format!(
            "Addon kinds: {}.",
            pelty_api::addons::KINDS
                .iter()
                .map(|x| c(x))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    ]
}

pub struct Ctx {
    pub spec: Value,
    pub routes: usize,
    pub undocumented: usize,
    pub json_files: Vec<(String, String)>,
}

/// Shared directives for other template sets.
pub(crate) fn expand_pub(dir: &str, ctx: &Ctx) -> Vec<Block> {
    expand(dir, ctx)
}

fn expand(dir: &str, ctx: &Ctx) -> Vec<Block> {
    let mut parts = dir.splitn(4, ':');
    let kind = parts.next().unwrap();
    let a = parts.next().unwrap_or("");
    let b = parts.next().unwrap_or("");
    match kind {
        "snippet" => snippet_blocks(a),
        "file" => vec![Block::Code(
            format!("{b} title={a}"),
            read(a).trim_end().into(),
        )],
        "item" => vec![Block::Code(
            format!("rust title={a} · {b}"),
            extract::item_source(&read(a), b),
        )],
        "rawconst" => vec![Block::Code(
            format!("{} title={a} · {b}", parts.next().unwrap_or("")),
            raw_const(a, b),
        )],
        "fields" => fields_blocks(a, b),
        "phases" => phases_blocks(),
        "slots" => slots_blocks(),
        "capabilities" => capabilities_blocks(),
        "permissions" => permissions_blocks(),
        "stats" => vec![Block::Table(
            vec!["What".into(), "Count".into()],
            vec![
                vec!["HTTP routes".into(), ctx.routes.to_string()],
                vec![
                    "…with a hand-written schema".into(),
                    (ctx.routes - ctx.undocumented).to_string(),
                ],
                vec!["Events (domain + pre)".into(), REGISTRY.len().to_string()],
                vec![
                    "Environment variables".into(),
                    extract::env_vars().len().to_string(),
                ],
                vec!["Migrations".into(), extract::migrations().len().to_string()],
                vec!["Version".into(), env!("CARGO_PKG_VERSION").into()],
            ],
        )],
        "json_index" => vec![Block::List(
            ctx.json_files
                .iter()
                .map(|(f, d)| format!("[{f}]({f}): {d}"))
                .collect(),
        )],
        _ => panic!("unknown directive {dir}"),
    }
}

const TEMPLATES: &[(&str, &str, &str)] = &[
    ("index", "Guides", include_str!("../templates/index.md")),
    (
        "getting-started",
        "Guides",
        include_str!("../templates/getting-started.md"),
    ),
    (
        "plugin-rust",
        "Guides",
        include_str!("../templates/plugin-rust.md"),
    ),
    (
        "events-guide",
        "Guides",
        include_str!("../templates/events-guide.md"),
    ),
    (
        "ui-guide",
        "Guides",
        include_str!("../templates/ui-guide.md"),
    ),
    (
        "other-languages",
        "Guides",
        include_str!("../templates/other-languages.md"),
    ),
    ("kinds", "Guides", include_str!("../templates/kinds.md")),
    ("themes", "Guides", include_str!("../templates/themes.md")),
    (
        "security",
        "Guides",
        include_str!("../templates/security.md"),
    ),
    (
        "addon-signing",
        "Guides",
        include_str!("../templates/developers/addon-signing.md"),
    ),
    (
        "addon-upgrades",
        "Guides",
        include_str!("../templates/developers/addon-upgrades.md"),
    ),
    (
        "addon-testing",
        "Guides",
        include_str!("../templates/developers/addon-testing.md"),
    ),
    (
        "addon-compatibility",
        "Guides",
        include_str!("../templates/developers/addon-compatibility.md"),
    ),
    (
        "addon-security-review",
        "Guides",
        include_str!("../templates/developers/addon-security-review.md"),
    ),
];

pub fn guides(ctx: &Ctx) -> Vec<Page> {
    TEMPLATES
        .iter()
        .map(|(slug, section, src)| {
            let section: &'static str = section;
            let (title, blocks) = parse_template(src, &mut |d| expand(d, ctx));
            Page {
                slug: slug.to_string(),
                title,
                section,
                blocks,
            }
        })
        .collect()
}

pub fn api(ctx: &Ctx) -> Page {
    let mut p = Page::new("api", "HTTP API", "Reference");
    p.p(format!(
        "{} operations on {} paths. Every route registered in the router is listed: {} have a hand-written schema (utoipa) and {} are marked *registered route* with handler and source only. The OpenAPI 3 document is [api.json](api.json). Paths are absolute.",
        ctx.routes,
        ctx.spec["paths"].as_object().map(|m| m.len()).unwrap_or(0),
        ctx.routes - ctx.undocumented,
        ctx.undocumented
    ));
    p.p("Authentication: the browser session cookie `pelty_session` (with the `X-CSRF-Token` header on writes) or `Authorization: Bearer pelty_…` customer API keys for headless routes. Routes under `/api/v1/admin` need staff, `/api/v1/client` a customer.");
    let mut by_tag: std::collections::BTreeMap<String, Vec<(String, String, Value)>> =
        Default::default();
    for (path, item) in ctx.spec["paths"].as_object().unwrap() {
        for (m, op) in item.as_object().unwrap() {
            if !op.is_object() {
                continue;
            }
            // utoipa defaults the tag to the module path (`crate::duo`); show `duo`.
            let tag = op["tags"][0]
                .as_str()
                .map(|t| t.trim_start_matches("crate::").replace("_", "-"))
                .unwrap_or_else(|| {
                    path.trim_start_matches("/api/v1/")
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .to_string()
                });
            by_tag
                .entry(if tag.is_empty() { "other".into() } else { tag })
                .or_default()
                .push((path.clone(), m.clone(), op.clone()));
        }
    }
    p.h(2, "Groups");
    p.list(
        by_tag
            .iter()
            .map(|(t, v)| format!("[{t}](#{}) ({})", crate::doc::slugify(t), v.len()))
            .collect(),
    );
    for (tag, ops) in &by_tag {
        p.h(2, tag.clone());
        let mut html = String::new();
        let mut md = String::from("| Method | Path | Summary |\n|---|---|---|\n");
        for (path, m, op) in ops {
            let summary = op["summary"]
                .as_str()
                .or(op["description"].as_str())
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            let und = op["x-undocumented"] == true;
            md.push_str(&format!(
                "| {} | `{}` | {}{} |\n",
                m.to_uppercase(),
                path,
                summary.replace('|', "\\|"),
                if und { " (registered route)" } else { "" }
            ));
            html.push_str(&format!(
                "<details class=\"op\"><summary><span class=\"m m-{m}\">{}</span><code>{}</code><span class=\"sum\">{}</span>{}</summary><div class=\"opb\">",
                m.to_uppercase(),
                esc(path),
                esc(&summary),
                if und { "<span class=\"tag\">registered route</span>" } else { "" }
            ));
            if let Some(d) = op["description"].as_str() {
                html.push_str(&format!("<p>{}</p>", inline(d)));
            }
            if let Some(params) = op["parameters"].as_array() {
                html.push_str("<table><thead><tr><th>Parameter</th><th>In</th><th>Required</th><th>Description</th></tr></thead><tbody>");
                for pa in params {
                    html.push_str(&format!(
                        "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td></tr>",
                        esc(pa["name"].as_str().unwrap_or("")),
                        esc(pa["in"].as_str().unwrap_or("")),
                        pa["required"] == true,
                        inline(pa["description"].as_str().unwrap_or(""))
                    ));
                }
                html.push_str("</tbody></table>");
            }
            if let Some(rb) = op["requestBody"]["content"].as_object() {
                for (ct, v) in rb {
                    html.push_str(&format!(
                        "<p>Request body <code>{}</code>: <code>{}</code></p>",
                        esc(ct),
                        esc(&schema_name(&v["schema"]))
                    ));
                }
            }
            if let Some(rs) = op["responses"].as_object() {
                let r: Vec<String> = rs
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "<code>{}</code> {}",
                            esc(k),
                            esc(v["description"].as_str().unwrap_or(""))
                        )
                    })
                    .collect();
                html.push_str(&format!("<p>Responses: {}</p>", r.join(" · ")));
            }
            if let Some(h) = op["x-handler"].as_str() {
                html.push_str(&format!(
                    "<p class=\"src\">Handler <code>{}</code> in <code>{}</code></p>",
                    esc(h),
                    esc(op["x-source"].as_str().unwrap_or(""))
                ));
            }
            html.push_str("</div></details>\n");
        }
        p.blocks.push(Block::Raw(html, md));
    }
    p
}

fn schema_name(s: &Value) -> String {
    if let Some(r) = s["$ref"].as_str() {
        return r.rsplit('/').next().unwrap_or(r).to_string();
    }
    if s["type"] == "array" {
        return format!("{}[]", schema_name(&s["items"]));
    }
    s["type"].as_str().unwrap_or("object").to_string()
}

pub fn hooks() -> Page {
    let mut p = Page::new("hooks", "Hooks and mixins", "Reference");
    let rt = read("crates/pelty-wasm/src/runtime.rs");
    p.h(2, "Hook declaration");
    fields_table(&mut p, "crates/pelty-wasm/src/runtime.rs", "Hook");
    p.h(2, "Priorities");
    let re = regex::Regex::new(r#""([a-z]+)" => Ok\(([A-Z_0-9]+)\)"#).unwrap();
    p.table(
        &["Name", "Value"],
        re.captures_iter(&rt)
            .map(|c| {
                vec![
                    format!("`{}`", &c[1]),
                    if &c[2] == "PRIORITY_MONITOR" {
                        pelty_wasm::PRIORITY_MONITOR.to_string()
                    } else {
                        c[2].to_string()
                    },
                ]
            })
            .collect(),
    );
    p.p("Integers from -1000 to 1000 are also accepted. Lower values run first, and ties are ordered by addon id.");
    p.h(2, "Event names a hook may use");
    p.list(vec![
        format!("Pre-events: {}.", REGISTRY.iter().filter(|e| e.kind == EventKind::Pre).map(|e| c(e.name)).collect::<Vec<_>>().join(", ")),
        "Request lifecycle: `<phase>:<METHOD> <route template>`, e.g. `before:PUT /api/v1/client/profile`; `before:*` matches every request.".into(),
        "Domain events with a phase: `after:invoice.paid`, `finished:order.created`.".into(),
        "Custom events: `before:<addon-id>:<event>`.".into(),
    ]);
    p.h(2, "Phases");
    p.blocks.extend(phases_blocks());
    p.h(2, "Mixins");
    fields_table(&mut p, "crates/pelty-wasm/src/runtime.rs", "Mixin");
    p.p(format!("`at` values: {}. Mixins target `<METHOD> <route template>` of a core route. An `after` or `return` mixin may only change values of keys the response already has. Admins can turn an addon's mixins off.", pelty_api::addon_events::MIXIN_AT.iter().map(|x| c(x)).collect::<Vec<_>>().join(", ")));
    p.h(2, "Core-function mixins");
    p.p("A mixin whose `target` is `fn <name>` wraps a core host function instead of a route (`at`: `before`, `after` or `around`; no `return`). Before the function runs the host calls op `mixin` with `{function, at, phase: \"before\", context, input}`; reply `{input?, state?}` to rewrite the input. After it ran: `{function, at, phase: \"after\", context, input, output, state}`; reply `{output?}`. `context` (who, which record) is read-only. A rewritten input or output must keep the JSON schema and must not change money values; otherwise it is dropped. Function mixins run in priority order, fail open and honour the admin kill switch. In the Rust SDK use `op::Mixin::{function, context, input, output}` and `reply::{fn_input, fn_output}`.");
    p.table(
        &["Function", "Input", "Output", "Context", "Description"],
        pelty_extensions::functions::FUNCTIONS
            .iter()
            .map(|f| {
                vec![
                    c(&format!("fn {}", f.name)),
                    f.input.to_string(),
                    f.output.to_string(),
                    f.context
                        .iter()
                        .map(|x| c(x))
                        .collect::<Vec<_>>()
                        .join(", "),
                    f.description.to_string(),
                ]
            })
            .collect(),
    );
    p.h(2, "Filters");
    p.p("A filter rewrites a value the host is about to use or show (WordPress-style `apply_filters`). Declare it in `capabilities.filters`; the host calls op `filter` with `{filter, value, context}` and expects `{value}`. Filters run in priority order, each seeing the previous output. A reply is kept only when it has the same JSON schema as the value (arrays may shrink or be reordered). Money keys may change only with `money.modify` and exact decimals (audited); otherwise that addon's output is dropped. Filters always fail open: an error, timeout or invalid reply leaves the value unchanged. Filters are a dangerous capability (`filters`) approved at install. Addons can define their own filter points too (ABI 0.4): declare `capabilities.filter_points: [\"<own-id>:<point>\"]` and call `filters.apply(name, value, context)` (SDK `apply_filter`); other addons subscribe with `filters: [{name: \"<id>:<point>\"}]`. The applying addon is never called for its own point, nesting is capped at 4 levels and money values never change.");
    fields_table(&mut p, "crates/pelty-wasm/src/runtime.rs", "Filter");
    p.table(
        &["Filter", "Value", "Context", "Rules", "Description"],
        pelty_extensions::filters::FILTERS
            .iter()
            .map(|f| {
                vec![
                    c(f.name),
                    f.value.to_string(),
                    if f.context.is_empty() {
                        "none".into()
                    } else {
                        f.context
                            .iter()
                            .map(|x| c(x))
                            .collect::<Vec<_>>()
                            .join(", ")
                    },
                    f.rules.to_string(),
                    f.description.to_string(),
                ]
            })
            .collect(),
    );
    p.h(2, "Example");
    p.blocks.extend(snippet_blocks("hooks"));
    p.blocks.extend(snippet_blocks("mixin"));
    p
}

pub fn capabilities() -> Page {
    let mut p = Page::new("capabilities", "Capabilities and permissions", "Reference");
    p.p("`addon.json` `capabilities` is parsed into the struct below. The admin approves it at install, and every host call is checked against it.");
    p.blocks.extend(capabilities_blocks());
    p.h(2, "Permissions");
    p.blocks.extend(permissions_blocks());
    p
}

pub fn manifest() -> Page {
    let mut p = Page::new("manifest", "addon.json manifest", "Reference");
    fields_table(
        &mut p,
        "crates/pelty-api/src/addon_host/addons.rs",
        "Manifest",
    );
    p.h(2, "Settings");
    fields_table(
        &mut p,
        "crates/pelty-api/src/addon_host/addons.rs",
        "SettingSpec",
    );
    p.h(2, "capabilities");
    p.p("See [Capabilities](capabilities.md).");
    p.h(2, "ui");
    p.p("See [UI schema](ui.md).");
    p.h(2, "Full example");
    p.code(
        "json",
        read("extensions/examples/addon-guide/addon.json").trim_end(),
    );
    p
}

pub fn wit() -> Page {
    let mut p = Page::new("wit", "WIT world and ABI", "Reference");
    p.p(format!("Current generic addon ABI: `{}`. The host strictly parses complete semver versions: stable `0.2.x`, `0.3.x` and `0.4.x` are accepted; the current `0.5` line accepts only `0.5.0`. Prereleases, future patches on the current line, incomplete or malformed versions, surrounding whitespace and unsupported major/minor lines are rejected. Build metadata is allowed. These generations share the `pelty:addon@0.2.0` WIT package; ABI 0.5 adds the optional scheduled export. ABI acceptance does not guarantee component linking or capability approval. Addons are WASI preview 2 components for the world below. See [SDK and ABI compatibility](addon-compatibility.md) for the full contract and deprecation policy.", pelty_wasm::runtime::ABI));
    p.p("Validate representative wire behavior with [local addon testing](addon-testing.md), and review [signed releases](addon-signing.md), [upgrades and migrations](addon-upgrades.md), and the [security checklist](addon-security-review.md) before deployment.");
    let w = read("crates/pelty-wasm/wit/pelty.wit");
    let (mut iface, mut doc, mut rows): (String, Vec<String>, Vec<Vec<String>>) =
        (String::new(), vec![], vec![]);
    for l in w.lines() {
        let t = l.trim();
        if let Some(d) = t.strip_prefix("///") {
            doc.push(d.trim().into());
        } else if let Some(i) = t.strip_prefix("interface ") {
            iface = i.trim_end_matches('{').trim().into();
            doc.clear();
        } else if let Some((name, sig)) = t.split_once(": func") {
            rows.push(vec![
                c(&iface),
                c(name),
                c(&format!("func{}", sig.trim_end_matches(';'))),
                doc.join(" "),
            ]);
            doc.clear();
        } else {
            doc.clear();
        }
    }
    p.h(2, "Functions");
    p.table(&["Interface", "Name", "Signature", "Description"], rows);
    p.h(2, "pelty.wit");
    p.code("wit", w.trim_end());
    if let Ok(cw) =
        std::fs::read_to_string(extract::repo().join("crates/pelty-wasm/wit/captcha.wit"))
    {
        p.h(2, "captcha.wit");
        p.code("wit", cw.trim_end());
    }
    p.h(2, "Rust SDK");
    p.p("`extensions/addons/sdk` (`pelty-addon-sdk`) wraps the bindings: `host`, `power` and `events` modules, `emit`, `fetch`, `setting`, `context` (ABI 0.4: the hook, filter or mixin being served), `reply::{proceed, modify, cancel, uncancel, filter, http, mixin_before, mixin_response, mixin_keep, fn_input, fn_output}`, and the `export!`, `gateway_addon!`, `provisioning_addon!`, `registrar_addon!` and `integration_addon!` macros.");
    p.p("For integration addons, implement the `Addon` trait and export it with `integration_addon!`. Inputs arrive parsed (`op::Hook`, `op::Filter`, `op::Http`, `op::Mixin`, each keeping the full input in `raw`), and every method has a safe default: hooks proceed, filters keep the value, routes return 404 and mixins change nothing. Other ops (jobs, `auth.verify`, custom ops) go to `op(name, input)`. Addons written against the raw `Guest` trait keep working unchanged.");
    p.code("rust", "use pelty_addon_sdk::{op, reply, serde_json::{json, Value}, Addon};\n\nstruct Mine;\nimpl Addon for Mine {\n    fn describe() -> Value {\n        json!({ \"id\": \"mine\", \"name\": \"Mine\" })\n    }\n    fn hook(h: &op::Hook) -> Result<Value, String> {\n        if h.event == \"ticket.before_open\" && h.str(\"subject\").is_some_and(|s| s.contains(\"viagra\")) {\n            return Ok(reply::cancel(\"spam\"));\n        }\n        Ok(reply::proceed())\n    }\n    fn filter(f: &op::Filter) -> Result<Value, String> {\n        let mut v = f.value.clone();\n        if f.name == \"client.dashboard\" {\n            v[\"upcoming\"] = json!([]);\n        }\n        Ok(reply::filter(v))\n    }\n    fn http(r: &op::Http) -> Result<Value, String> {\n        Ok(match (r.method.as_str(), r.path.as_str()) {\n            (\"GET\", \"/hello\") => reply::http(200, json!({ \"message\": \"hi\" })),\n            _ => reply::http(404, Value::Null),\n        })\n    }\n}\npelty_addon_sdk::integration_addon!(Mine);");
    p
}

pub fn ui() -> Page {
    use pelty_extensions::ui_schema as u;
    let mut p = Page::new("ui", "UI schema", "Reference");
    let src = read("crates/pelty-extensions/src/ui_schema.rs");
    p.p(extract::module_doc(&src)
        .replace('\n', " ")
        .replace("See `make docs` for the node reference.", ""));
    p.h(2, "Node types");
    p.p(extract::item_doc(&src, "NODE_TYPES"));
    p.list(u::NODE_TYPES.iter().map(|x| c(x)).collect());
    p.p(format!(
        "Limits: at most {} nodes and depth {} per tree.",
        u::MAX_NODES,
        u::MAX_DEPTH
    ));
    p.h(2, "Slots");
    p.blocks.extend(slots_blocks());
    for s in ["UiPage", "UiSlotContribution", "Frame", "DataSource"] {
        p.h(2, s);
        fields_table(&mut p, "crates/pelty-extensions/src/ui_schema.rs", s);
    }
    p.h(2, "Example");
    p.p("See [Addon UI](ui-guide.md).");
    p
}

pub fn configuration() -> Page {
    let mut p = Page::new("configuration", "Configuration", "Reference");
    p.p("Every environment variable read by the server crates. Defaults come from `pelty-core/src/config.rs`. Variables read elsewhere show the file that reads them.");
    p.table(
        &["Variable", "Default", "Description", "Read in"],
        extract::env_vars()
            .into_iter()
            .map(|e| {
                vec![
                    c(&e.name),
                    if e.default.is_empty() {
                        "".into()
                    } else {
                        c(&e.default)
                    },
                    e.doc,
                    e.used_in.iter().map(|x| c(x)).collect::<Vec<_>>().join(" "),
                ]
            })
            .collect(),
    );
    p
}

pub fn cli() -> Page {
    let mut p = Page::new("cli", "CLI", "Reference");
    p.h(2, "pelty-server");
    p.code(
        "text",
        extract::module_doc(&read("crates/pelty-server/src/main.rs")),
    );
    p.h(2, "pelty-cli");
    p.code(
        "text",
        extract::module_doc(&read("crates/pelty-cli/src/main.rs"))
            .replace("```text\n", "")
            .replace("\n```", ""),
    );
    p.h(2, "cargo xtask");
    p.p("Developer commands are plain Rust in the `xtask` crate, so they work the same on Windows, macOS and Linux without make or bash. `cargo dev` is a shortcut for `cargo xtask dev` (see `.cargo/config.toml`). With GNU make installed, `make <command>` runs the same thing.");
    // One `("name", "help"),` line per command in xtask's COMMANDS table.
    let src = read("xtask/src/main.rs");
    let rows = src
        .lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("(\"")?
                .strip_suffix("\"),")?
                .split_once("\", \"")
                .map(|(n, d)| vec![c(&format!("cargo xtask {n}")), d.replace("\\\"", "\"")])
        })
        .collect();
    p.table(&["Command", "Description"], rows);
    p
}

pub fn architecture() -> Page {
    let mut p = Page::new("architecture", "Architecture / codebase map", "Reference");
    p.p("Every crate and module of the Rust workspace with the summary from its `//!` doc, generated from source. Big crates group modules by domain folder (for example `pelty-database/src/billing/`, `pelty-api/src/routes/finance/`); old flat paths still work through `pub use` re-exports. A test fails when a module has no `//!` doc.");
    let mods = extract::modules();
    let mut krates: Vec<&str> = mods.iter().map(|m| m.krate.as_str()).collect();
    krates.dedup();
    for k in krates {
        p.h(2, k);
        let lib = mods.iter().find(|m| {
            m.krate == k && (m.path.ends_with("src/lib.rs") || m.path.ends_with("src/main.rs"))
        });
        if let Some(l) = lib {
            p.p(l.summary.clone());
        }
        p.table(
            &["Module", "What it does"],
            mods.iter()
                .filter(|m| m.krate == k)
                .map(|m| {
                    vec![
                        c(m.path.split_once("/src/").map(|x| x.1).unwrap_or(&m.path)),
                        m.summary.clone(),
                    ]
                })
                .collect(),
        );
    }
    p
}

pub fn migrations() -> Page {
    let mut p = Page::new("migrations", "Database migrations", "Reference");
    let m = extract::migrations();
    p.p(format!("{} migrations in `migrations/`, applied in order by `pelty-server migrate` (and on `serve`). The summary is the file's leading comment.", m.len()));
    p.table(
        &["File", "Summary"],
        m.into_iter().map(|m| vec![c(&m.file), m.summary]).collect(),
    );
    p
}
