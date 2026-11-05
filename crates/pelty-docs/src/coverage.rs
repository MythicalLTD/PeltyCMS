//! Machine-checked documentation coverage. Each category lists the items the
//! code defines and checks that the page documenting them mentions every
//! one. `tests::nothing_is_missing` fails when an item is missing.
use crate::doc::{self, Page};
use crate::extract;
use crate::pages::Ctx;
use crate::Site;
use pelty_extensions::events::{EventKind, REGISTRY};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Category {
    pub name: String,
    /// Docs set and page(s) that must mention each item.
    pub page: String,
    pub source: String,
    pub total: usize,
    pub covered: usize,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiSchema {
    pub operations: usize,
    pub with_schema: usize,
    pub stubs: usize,
    /// Stub count per first path segment after `/api/v1`.
    pub stubs_by_group: std::collections::BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Coverage {
    pub categories: Vec<Category>,
    pub api_schema: ApiSchema,
    /// Known gaps, stated honestly (not failures).
    pub not_covered: Vec<String>,
}

impl Coverage {
    pub fn missing(&self) -> Vec<(String, String)> {
        self.categories.iter().flat_map(|c| c.missing.iter().map(move |m| (c.name.clone(), m.clone()))).collect()
    }
}

fn md(site: &Site, slugs: &[&str]) -> String {
    let mut s = String::new();
    for slug in slugs {
        let p = site.pages.iter().find(|p| p.slug == *slug).unwrap_or_else(|| panic!("coverage: no page {slug} in {}", site.name));
        s.push_str(&doc::to_md(p));
    }
    s
}

fn all_md(site: &Site, skip: &[&str]) -> String {
    site.pages.iter().filter(|p| !skip.contains(&p.slug.as_str())).map(doc::to_md).collect()
}

fn cat(name: &str, page: &str, source: &str, items: Vec<String>, hay: &str, fmt: impl Fn(&str) -> String) -> Category {
    let missing: Vec<String> = items.iter().filter(|i| !hay.contains(&fmt(i))).cloned().collect();
    Category { name: name.into(), page: page.into(), source: source.into(), total: items.len(), covered: items.len() - missing.len(), missing }
}

fn tick(s: &str) -> String {
    format!("`{s}`")
}

pub fn report(dev: &Site, user: &Site, ctx: &Ctx) -> Coverage {
    let mut cats = vec![];
    let rt = extract::read("crates/pelty-wasm/src/runtime.rs");
    // HTTP API
    let mut ops = vec![];
    let mut writes = vec![];
    for (path, item) in ctx.spec["paths"].as_object().unwrap() {
        for (m, op) in item.as_object().unwrap() {
            if op.is_object() {
                ops.push(format!("{} {path}", m.to_uppercase()));
                // Same scope as addon_events: core /api/v1 routes, not addon routes.
                if ["post", "put", "patch", "delete"].contains(&m.as_str()) && path.starts_with("/api/v1/") && !path.starts_with("/api/v1/addons/") {
                    writes.push(format!("before:{} {path}", m.to_uppercase()));
                }
            }
        }
    }
    let api = md(dev, &["api"]);
    cats.push(cat("HTTP API operations", "developers/api", "router (routes::inventory)", ops.clone(), &api, |o| {
        let (m, p) = o.split_once(' ').unwrap();
        format!("| {m} | `{p}` |")
    }));
    writes.sort();
    writes.dedup();
    cats.push(cat("Cancellable request hook points (mutating routes)", "developers/cancellable", "POST/PUT/PATCH/DELETE routes under /api/v1 (not /api/v1/addons/)", writes, &md(dev, &["cancellable"]), tick));
    cats.push(cat("Events", "developers/events", "pelty_extensions::events::REGISTRY", REGISTRY.iter().map(|e| e.name.to_string()).collect(), &md(dev, &["events"]), tick));
    cats.push(cat("Cancellable / modifiable events", "developers/cancellable", "REGISTRY (cancellable or modifiable)", REGISTRY.iter().filter(|e| e.cancellable || e.modifiable).map(|e| e.name.to_string()).collect(), &md(dev, &["cancellable"]), tick));
    cats.push(cat("Event payload keys", "developers/events", "REGISTRY payload", REGISTRY.iter().flat_map(|e| e.payload.iter().map(move |f| format!("{}.{}", e.name, f.name))).collect(), &md(dev, &["events"]), |k| tick(k.rsplit_once('.').unwrap().1)));
    cats.push(cat("Pre-events in hooks", "developers/hooks", "REGISTRY (kind Pre)", REGISTRY.iter().filter(|e| e.kind == EventKind::Pre).map(|e| e.name.to_string()).collect(), &md(dev, &["hooks"]), tick));
    cats.push(cat("Hook phases", "developers/hooks", "pelty_wasm::PHASES", pelty_wasm::PHASES.iter().map(|s| s.to_string()).collect(), &md(dev, &["hooks"]), tick));
    cats.push(cat("Mixin positions", "developers/hooks", "pelty_api::addon_events::MIXIN_AT", pelty_api::addon_events::MIXIN_AT.iter().map(|s| s.to_string()).collect(), &md(dev, &["hooks"]), tick));
    cats.push(cat("Core functions", "developers/hooks", "pelty_extensions::functions::FUNCTIONS", pelty_extensions::functions::FUNCTIONS.iter().map(|f| format!("fn {}", f.name)).collect(), &md(dev, &["hooks"]), tick));
    cats.push(cat("Filter points", "developers/hooks", "pelty_extensions::filters::FILTERS", pelty_extensions::filters::FILTERS.iter().map(|f| f.name.to_string()).collect(), &md(dev, &["hooks"]), tick));
    cats.push(cat("Hook / Mixin fields", "developers/hooks", "runtime.rs Hook, Mixin", extract::struct_fields(&rt, "Hook").into_iter().chain(extract::struct_fields(&rt, "Mixin")).chain(extract::struct_fields(&rt, "Filter")).map(|f| f.name).collect(), &md(dev, &["hooks"]), tick));
    // UI slots
    let w = md(dev, &["widgets"]);
    let pages = crate::slots::pages();
    cats.push(cat("UI page slots (one route key per page)", "developers/widgets", "frontend/src/App.tsx routes", pages.iter().map(|p| format!("{}.{}", p.area, p.key)).collect(), &w, |k| format!("`{k}.")));
    cats.push(cat("Named UI slots", "developers/widgets", "<PluginSlot>/<PluginWidgets> calls", crate::slots::named().into_iter().map(|n| n.slot).collect(), &w, tick));
    cats.push(cat("Layout slot positions", "developers/widgets", "ui_schema::LAYOUT_POSITIONS", pelty_extensions::ui_schema::LAYOUT_POSITIONS.iter().map(|s| s.to_string()).collect(), &w, |s| format!("layout.{s}")));
    cats.push(cat("UI node types", "developers/ui", "ui_schema::NODE_TYPES", pelty_extensions::ui_schema::NODE_TYPES.iter().map(|s| s.to_string()).collect(), &md(dev, &["ui"]), tick));
    // Addon API
    cats.push(cat("Capabilities fields", "developers/capabilities", "runtime.rs Capabilities", extract::struct_fields(&rt, "Capabilities").into_iter().map(|f| f.name).collect(), &md(dev, &["capabilities"]), tick));
    cats.push(cat("Package permissions", "developers/capabilities", "pelty_api::addons::PERMISSIONS", pelty_api::addons::PERMISSIONS.iter().map(|s| s.to_string()).collect(), &md(dev, &["capabilities"]), tick));
    cats.push(cat("Manifest fields", "developers/manifest", "addons.rs Manifest", extract::struct_fields(&extract::read("crates/pelty-api/src/addon_host/addons.rs"), "Manifest").into_iter().map(|f| f.name).collect(), &md(dev, &["manifest"]), tick));
    let wit = extract::read("crates/pelty-wasm/wit/pelty.wit");
    let funcs: Vec<String> = wit.lines().filter_map(|l| l.trim().split_once(": func").map(|(n, _)| n.trim().to_string())).collect();
    cats.push(cat("WIT host/guest functions", "developers/wit", "crates/pelty-wasm/wit/pelty.wit", funcs, &md(dev, &["wit"]), |f| format!("| `{f}` |")));
    // Platform
    cats.push(cat("Environment variables", "developers/configuration", "extract::env_vars (string literals in crates/*/src)", extract::env_vars().into_iter().map(|e| e.name).collect(), &md(dev, &["configuration"]), tick));
    cats.push(cat("Settings keys", "developers/settings-keys", "settings::get/set calls + raw SQL", extract::settings_keys().0.into_iter().map(|k| k.key).collect(), &md(dev, &["settings-keys"]), tick));
    cats.push(cat("General settings fields", "developers/settings-keys", "general_settings.rs structs", crate::platform::GENERAL_STRUCTS.iter().flat_map(|s| extract::struct_fields(&extract::read("crates/pelty-database/src/config/general_settings.rs"), s).into_iter().map(|f| f.name)).collect(), &md(dev, &["settings-keys"]), tick));
    cats.push(cat("Email templates", "developers/email-templates", "pelty_notifications::template::TEMPLATES", pelty_notifications::template::TEMPLATES.iter().map(|t| t.name.to_string()).collect(), &md(dev, &["email-templates"]), tick));
    cats.push(cat("Scheduled tasks", "developers/scheduled-tasks", "automation::TASKS", pelty_api::routes::automation::TASKS.iter().map(|t| t.0.to_string()).collect(), &md(dev, &["scheduled-tasks"]), tick));
    cats.push(cat("Background job kinds", "developers/scheduled-tasks", "JOB_KIND consts", crate::platform::job_kinds().into_iter().map(|j| j.0).collect(), &md(dev, &["scheduled-tasks"]), tick));
    cats.push(cat("Migrations", "developers/migrations", "migrations/", extract::migrations().into_iter().map(|m| m.file).collect(), &md(dev, &["migrations"]), tick));
    // Addons: one page each.
    let addons = crate::addons::all();
    let slugs: Vec<&str> = dev.pages.iter().map(|p| p.slug.as_str()).collect();
    let missing: Vec<String> = addons.iter().filter(|a| !slugs.contains(&a.slug.as_str())).map(|a| a.dir.clone()).collect();
    cats.push(Category { name: "Addons (one page each)".into(), page: "developers/addon-*".into(), source: "extensions/*/*/addon.json".into(), total: addons.len(), covered: addons.len() - missing.len(), missing });
    {
        let mut items = vec![];
        let mut missing = vec![];
        for a in &addons {
            let hay = dev.pages.iter().find(|p| p.slug == a.slug).map(doc::to_md).unwrap_or_default();
            for (k, _, _) in a.settings.iter().chain(&a.server_fields).chain(&a.product_fields) {
                let item = format!("{}: {k}", a.dir);
                if !hay.contains(&tick(k)) {
                    missing.push(item.clone());
                }
                items.push(item);
            }
        }
        cats.push(Category { name: "Addon settings and module fields".into(), page: "developers/addon-*".into(), source: "addon.json settings, SettingField / module fields in the source".into(), total: items.len(), covered: items.len() - missing.len(), missing });
    }
    // User docs: every page of both areas.
    let guide = all_md(user, &["admin-map", "client-map"]);
    let usr = all_md(user, &[]);
    let admin_nav: Vec<String> = extract::nav("admin").into_iter().map(|l| l.path).collect();
    let client_nav: Vec<String> = extract::nav("client").into_iter().map(|l| l.path).collect();
    cats.push(cat("Admin pages listed (user docs)", "admin-map", "frontend navRegistry ADMIN_NAV", admin_nav.clone(), &usr, tick));
    cats.push(cat("Client pages listed (user docs)", "client-map", "frontend navRegistry CLIENT_NAV_TREE", client_nav.clone(), &usr, tick));
    cats.push(cat("Setup checklist steps", "admin-setup", "setup_wizard.rs steps", extract::setup_steps().into_iter().map(|s| s.2).collect(), &md(user, &["admin-setup"]), tick));
    // API schema quality (reported, not a failure).
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    let mut stubs = 0;
    for (path, item) in ctx.spec["paths"].as_object().unwrap() {
        for op in item.as_object().unwrap().values() {
            if op["x-undocumented"] == true {
                stubs += 1;
                *by.entry(path.trim_start_matches("/api/v1/").split('/').next().unwrap_or("").to_string()).or_default() += 1;
            }
        }
    }
    let api_schema = ApiSchema { operations: ops.len(), with_schema: ops.len() - stubs, stubs, stubs_by_group: by };
    // Honest gaps.
    let mut not_covered = vec![format!(
        "{} of {} HTTP operations have no hand-written request/response schema (marked \"registered route\"): method, path, handler and source file only.",
        api_schema.stubs, api_schema.operations
    )];
    let explained_admin: Vec<&String> = admin_nav.iter().filter(|p| !guide.contains(&format!("`{p}`"))).collect();
    let explained_client: Vec<&String> = client_nav.iter().filter(|p| !guide.contains(&format!("`{p}`"))).collect();
    if !explained_admin.is_empty() {
        not_covered.push(format!("Admin pages only listed in the admin area map, without a guide section: {}.", explained_admin.iter().map(|s| tick(s)).collect::<Vec<_>>().join(", ")));
    }
    if !explained_client.is_empty() {
        not_covered.push(format!("Client pages only listed in the client area map, without a guide section: {}.", explained_client.iter().map(|s| tick(s)).collect::<Vec<_>>().join(", ")));
    }
    let routed: Vec<String> = pages.iter().filter(|p| p.area == "admin").map(|p| p.path.clone()).filter(|p| !p.contains(':') && !admin_nav.contains(p)).collect();
    if !routed.is_empty() {
        not_covered.push(format!("Admin routes that are not in the navigation (reached from other pages) are not described in the user docs: {}.", routed.iter().map(|s| tick(s)).collect::<Vec<_>>().join(", ")));
    }
    let outside: Vec<String> = ctx.spec["paths"].as_object().unwrap().keys().filter(|p| !p.starts_with("/api/v1/")).cloned().collect();
    if !outside.is_empty() {
        not_covered.push(format!("Routes outside `/api/v1` are listed in the API reference but have no request events or mixins (the addon hook layer only wraps `/api/v1`): {}.", outside.iter().map(|s| tick(s)).collect::<Vec<_>>().join(", ")));
    }
    not_covered.push("Plugins (`extensions/plugins/*/plugin.json`) and themes (`extensions/themes/*`) have no per-item reference page; they are covered by the Plugins and Themes guides.".into());
    not_covered.push("User-guide prose (what a button does, how a flow works) is written by hand and checked by review, not by tests; only the tables in it are generated.".into());
    let (_, dynamic) = extract::settings_keys();
    if !dynamic.is_empty() {
        not_covered.push(format!("Settings keys built at run time cannot be listed: {}.", dynamic.iter().map(|s| tick(s)).collect::<Vec<_>>().join(", ")));
    }
    Coverage { categories: cats, api_schema, not_covered }
}

pub fn page(cov: &Coverage) -> Page {
    let mut p = Page::new("coverage", "Documentation coverage", "Platform reference");
    let (t, c): (usize, usize) = cov.categories.iter().fold((0, 0), |a, x| (a.0 + x.total, a.1 + x.covered));
    p.p(format!(
        "Generated by `pelty-docs` on every build. For each category the items are read from the code and the documenting page must mention every one; the test `coverage::tests::nothing_is_missing` fails the build otherwise. {c} of {t} items covered. Machine-readable: [coverage.json](coverage.json)."
    ));
    p.table(
        &["Category", "Covered", "Page", "Source"],
        cov.categories.iter().map(|x| vec![x.name.clone(), format!("{}/{}{}", x.covered, x.total, if x.missing.is_empty() { String::new() } else { format!(" (missing: {})", x.missing.join(", ")) }), x.page.clone(), x.source.clone()]).collect(),
    );
    p.h(2, "API schemas");
    p.p(format!(
        "{} operations: {} with a hand-written schema, {} registered-route stubs (handler and source only).",
        cov.api_schema.operations, cov.api_schema.with_schema, cov.api_schema.stubs
    ));
    p.table(&["Group", "Stubs"], cov.api_schema.stubs_by_group.iter().map(|(g, n)| vec![format!("`{g}`"), n.to_string()]).collect());
    p.h(2, "Not covered");
    p.list(cov.not_covered.clone());
    p
}

#[cfg(test)]
mod tests {
    #[test]
    fn nothing_is_missing() {
        let dir = std::env::temp_dir().join(format!("pelty-docs-cov-{}", std::process::id()));
        crate::generate(&dir).unwrap();
        let cov: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("developers/coverage.json")).unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let mut bad = vec![];
        for c in cov["categories"].as_array().unwrap() {
            assert!(c["total"].as_u64().unwrap() > 0, "category {} found no items in the code", c["name"]);
            for m in c["missing"].as_array().unwrap() {
                bad.push(format!("{}: {}", c["name"].as_str().unwrap(), m.as_str().unwrap()));
            }
        }
        assert!(bad.is_empty(), "undocumented items:\n{}", bad.join("\n"));
    }
}
