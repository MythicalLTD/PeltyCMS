//! The general documentation set at `/docs/`: a landing page and the
//! Administrator and Client guides. Prose lives in `templates/user/*.md`;
//! tables are pulled from code with directives (see [`expand`]).
use crate::doc::{esc, parse_template, Block, Page};
use crate::extract::{self, read};
use crate::pages::{self, fields_table, Ctx};
use crate::Site;

fn c(s: &str) -> String {
    format!("`{s}`")
}

pub const ADMIN: &str = "Administrator guide";
pub const CLIENT: &str = "Client guide";

const TEMPLATES: &[(&str, &str, &str)] = &[
    ("admin-install", ADMIN, include_str!("../templates/user/admin-install.md")),
    ("admin-setup", ADMIN, include_str!("../templates/user/admin-setup.md")),
    ("admin-catalog", ADMIN, include_str!("../templates/user/admin-catalog.md")),
    ("admin-billing", ADMIN, include_str!("../templates/user/admin-billing.md")),
    ("admin-domains", ADMIN, include_str!("../templates/user/admin-domains.md")),
    ("admin-provisioning", ADMIN, include_str!("../templates/user/admin-provisioning.md")),
    ("admin-support", ADMIN, include_str!("../templates/user/admin-support.md")),
    ("admin-security", ADMIN, include_str!("../templates/user/admin-security.md")),
    ("admin-email", ADMIN, include_str!("../templates/user/admin-email.md")),
    ("admin-automation", ADMIN, include_str!("../templates/user/admin-automation.md")),
    ("admin-addons", ADMIN, include_str!("../templates/user/admin-addons.md")),
    ("admin-maintenance", ADMIN, include_str!("../templates/user/admin-maintenance.md")),
    ("admin-api", ADMIN, include_str!("../templates/user/admin-api.md")),
    ("admin-map", ADMIN, include_str!("../templates/user/admin-map.md")),
    ("client-account", CLIENT, include_str!("../templates/user/client-account.md")),
    ("client-orders", CLIENT, include_str!("../templates/user/client-orders.md")),
    ("client-billing", CLIENT, include_str!("../templates/user/client-billing.md")),
    ("client-services", CLIENT, include_str!("../templates/user/client-services.md")),
    ("client-domains", CLIENT, include_str!("../templates/user/client-domains.md")),
    ("client-support", CLIENT, include_str!("../templates/user/client-support.md")),
    ("client-security", CLIENT, include_str!("../templates/user/client-security.md")),
    ("client-api", CLIENT, include_str!("../templates/user/client-api.md")),
    ("client-map", CLIENT, include_str!("../templates/user/client-map.md")),
];

/// Template sources, for tests.
pub fn sources() -> impl Iterator<Item = (&'static str, &'static str)> {
    TEMPLATES.iter().map(|(s, _, src)| (*s, *src))
}

fn nav_table(scope: &str, cat: Option<&str>) -> Vec<Block> {
    let leaves: Vec<_> = extract::nav(scope).into_iter().filter(|l| cat.is_none_or(|c| l.category == c)).collect();
    assert!(!leaves.is_empty(), "nav:{scope}:{cat:?} matched nothing");
    let mut blocks = vec![];
    if cat.is_some() {
        blocks.push(Block::Table(vec!["Page".into(), "Path".into(), "What it is for".into()], leaves.iter().map(|l| vec![format!("**{}**", l.label), c(&l.path), l.desc.clone()]).collect()));
        return blocks;
    }
    let mut cats: Vec<(String, String)> = vec![];
    for l in &leaves {
        if !cats.iter().any(|(id, _)| *id == l.category) {
            cats.push((l.category.clone(), l.category_label.clone()));
        }
    }
    for (id, label) in cats {
        blocks.push(Block::H(2, label));
        blocks.push(Block::Table(vec!["Page".into(), "Path".into(), "What it is for".into()], leaves.iter().filter(|l| l.category == id).map(|l| vec![format!("**{}**", l.label), c(&l.path), l.desc.clone()]).collect()));
    }
    blocks
}

/// Operations whose path starts with one of `prefixes` (comma separated,
/// without `/api/v1`).
fn routes_table(ctx: &Ctx, prefixes: &str) -> Vec<Block> {
    let pre: Vec<String> = prefixes.split(',').map(|p| format!("/api/v1{}", p.trim())).collect();
    let mut rows = vec![];
    for (path, item) in ctx.spec["paths"].as_object().unwrap() {
        if !pre.iter().any(|p| path == p || path.starts_with(&format!("{p}/"))) {
            continue;
        }
        for (m, op) in item.as_object().unwrap() {
            if !op.is_object() {
                continue;
            }
            let s = op["summary"].as_str().or(op["description"].as_str()).unwrap_or("").lines().next().unwrap_or("").to_string();
            rows.push(vec![m.to_uppercase(), c(path), s]);
        }
    }
    assert!(!rows.is_empty(), "routes:{prefixes} matched nothing");
    vec![Block::Table(vec!["Method".into(), "Path".into(), "Summary".into()], rows), Block::P("Full request and response details: [HTTP API](developers/api.md).".into())]
}

fn addons_table(kind: &str) -> Vec<Block> {
    let rows: Vec<Vec<String>> = crate::addons::all()
        .into_iter()
        .filter(|a| !a.fixture && a.manifest["kind"] == kind)
        .map(|a| vec![format!("[{}](developers/{}.md)", a.manifest["name"].as_str().unwrap_or(&a.dir), a.slug), c(a.manifest["id"].as_str().unwrap_or("")), a.settings.iter().map(|s| c(&s.0)).collect::<Vec<_>>().join(" ")])
        .collect();
    assert!(!rows.is_empty(), "addons:{kind} matched nothing");
    vec![Block::Table(vec!["Addon".into(), "Id".into(), "Settings".into()], rows)]
}

fn expand(d: &str, ctx: &Ctx) -> Vec<Block> {
    let (kind, rest) = d.split_once(':').unwrap_or((d, ""));
    let en = extract::en();
    match kind {
        "nav" => {
            let (scope, cat) = rest.split_once(':').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
            nav_table(scope, cat)
        }
        "routes" => routes_table(ctx, rest),
        "addons" => addons_table(rest),
        "wizard" => vec![Block::Table(
            vec!["Step".into(), "Required".into(), "Where".into(), "What it checks".into()],
            extract::setup_steps()
                .into_iter()
                .map(|(id, opt, href)| vec![format!("**{}**", en.get(&format!("setup.step.{id}")).cloned().unwrap_or(id.clone())), if opt { "optional".into() } else { "yes".into() }, c(&href), en.get(&format!("setup.step.{id}_hint")).cloned().unwrap_or_default()])
                .collect(),
        )],
        "env" => {
            let want: Vec<&str> = rest.split(',').filter(|s| !s.is_empty()).collect();
            let vars: Vec<_> = extract::env_vars().into_iter().filter(|e| want.is_empty() || want.contains(&e.name.as_str())).collect();
            assert!(want.is_empty() || vars.len() == want.len(), "env:{rest}: unknown variable");
            vec![Block::Table(vec!["Variable".into(), "Default".into(), "Description".into()], vars.into_iter().map(|e| vec![c(&e.name), if e.default.is_empty() { String::new() } else { c(&e.default) }, e.doc]).collect())]
        }
        "tasks" => vec![Block::Table(
            vec!["Task".into(), "Shown as".into(), "Runs".into()],
            pelty_api::routes::automation::TASKS.iter().map(|(k, s)| vec![c(k), en.get(&format!("automation.task.{k}")).cloned().unwrap_or_default(), if s % 86_400 == 0 { "daily".into() } else if s % 3_600 == 0 { format!("every {} h", s / 3_600) } else { format!("every {} min", s / 60) }]).collect(),
        )],
        "sensitive" => vec![Block::List(pelty_api::extract::SENSITIVE_PERMS.iter().map(|p| c(p)).collect())],
        "fields" => {
            let (file, name) = rest.split_once(':').unwrap();
            let mut p = Page::new("", "", "");
            fields_table(&mut p, file, name);
            p.blocks.pop();
            p.blocks
        }
        "email_count" => vec![Block::P(format!("PeltyCMS ships {} email templates; the full list with variables is in [Email templates](developers/email-templates.md).", pelty_notifications::template::TEMPLATES.len()))],
        "general" => vec![Block::List(crate::platform::GENERAL_STRUCTS.iter().map(|s| format!("{}: {}", c(s), extract::item_doc(&read("crates/pelty-database/src/config/general_settings.rs"), s))).collect())],
        _ => pages::expand_pub(d, ctx),
    }
}

pub fn pages(ctx: &Ctx) -> Vec<Page> {
    TEMPLATES
        .iter()
        .map(|(slug, section, src)| {
            let (title, blocks) = parse_template(src, &mut |d| expand(d, ctx));
            Page { slug: slug.to_string(), title, section, blocks }
        })
        .collect()
}

/// `/docs/index.html`: pick a docs set.
pub fn landing(dev: &Site, user: &Site) -> Page {
    let mut p = Page::new("index", "PeltyCMS documentation", "Start");
    p.p("PeltyCMS is a self-hosted billing and automation panel for hosting companies. Pick the documentation for what you are doing:");
    let count = |s: &str| user.pages.iter().filter(|p| p.section == s).count();
    let card = |href: &str, title: &str, body: &str, more: &str| format!("<a class=\"card\" href=\"{href}\"><h3>{}</h3><p>{}</p><span class=\"more\">{} →</span></a>", esc(title), esc(body), esc(more));
    let html = format!(
        "<div class=\"cards\">{}{}{}</div>",
        card("admin-install.html", ADMIN, &format!("Planned installation (not released yet), configuration and running PeltyCMS: products, billing, domains, provisioning, support, staff security, backups and addons. {} pages.", count(ADMIN)), "For staff"),
        card("client-account.html", CLIENT, &format!("Using the client area: your account, orders, invoices and payments, services, domains, tickets, 2FA and API keys. {} pages.", count(CLIENT)), "For customers"),
        card("developers/index.html", "Developer docs", &format!("Build addons, plugins and themes; the HTTP API, events, hooks, mixins, UI slots, WIT and every bundled addon. {} pages.", dev.pages.len()), "For developers"),
    );
    let md = format!("- [{ADMIN}](admin-install.md)\n- [{CLIENT}](client-account.md)\n- [Developer docs](developers/index.md)\n");
    p.blocks.push(Block::Raw(html, md));
    p.h(2, "Everything here comes from the code");
    p.p("Both sets are generated by `pelty-docs` (`cargo xtask docs`, or `make docs`) from the PeltyCMS source: tables of admin pages, settings, tasks, email templates, addons and API routes are read from the code at build time, so they match the version you run. The developer set includes a [coverage report](developers/coverage.md) that the test suite checks.");
    p
}
