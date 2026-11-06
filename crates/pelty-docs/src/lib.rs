//! Documentation generator for PeltyCMS. Everything it writes is derived from
//! code: the route inventory and utoipa schema, the event registry, the WASM
//! runtime's capability/hook types, the WIT world, the UI schema, the config
//! loader, the migrations directory and the `extensions/examples/addon-guide`
//! sources. Guide prose lives in `templates/`.
#![forbid(unsafe_code)]
pub mod addons;
pub mod coverage;
pub mod doc;
pub mod extract;
pub mod highlight;
pub mod pages;
pub mod platform;
pub mod reference;
pub mod routes;
pub mod slots;
pub mod userdocs;

use doc::{esc, Page};
use serde_json::json;
use std::path::Path;

const CSS: &str = include_str!("../templates/style.css");
const JS: &str = include_str!("../templates/app.js");
const LOGO: &[u8] = include_bytes!("../templates/logo.png");
const FAVICON: &[u8] = include_bytes!("../templates/favicon.png");
const TOUCH: &[u8] = include_bytes!("../templates/apple-touch-icon.png");
const OG: &[u8] = include_bytes!("../templates/og.png");
/// Public URL of the docs root; override with `PELTY_DOCS_BASE_URL` at generation time.
fn base_url() -> String {
    let b = std::env::var("PELTY_DOCS_BASE_URL")
        .unwrap_or_else(|_| "https://peltycms.com/docs/".into());
    if b.ends_with('/') {
        b
    } else {
        format!("{b}/")
    }
}
/// Plain-text summary from the page's first paragraph, cut to ~160 chars.
fn summary(p: &Page, set: &str) -> String {
    let first = p.blocks.iter().find_map(|b| match b {
        doc::Block::P(t) => Some(doc::inline_text(t)),
        _ => None,
    });
    let raw = first
        .map(|t| strip_tags(&t))
        .unwrap_or_else(|| format!("{}: PeltyCMS {set}", p.title));
    let raw = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if raw.chars().count() <= 160 {
        return raw;
    }
    let cut: String = raw.chars().take(157).collect();
    format!(
        "{}...",
        cut.rsplit_once(' ').map(|(a, _)| a).unwrap_or(&cut)
    )
}
fn strip_tags(s: &str) -> String {
    let mut o = String::new();
    let mut tag = false;
    for c in s.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => o.push(c),
            _ => {}
        }
    }
    o.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}
fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap().replace("</", "<\\/")
}

/// One docs set written to its own folder (`""` = the root `/docs/`).
pub struct Site {
    pub dir: &'static str,
    pub name: &'static str,
    /// Sidebar groups in order; every page's `section` must be one of them.
    pub sections: &'static [&'static str],
    pub pages: Vec<Page>,
}

pub const DEV_SECTIONS: &[&str] = &[
    "Get started",
    "Guides",
    "Extension reference",
    "Platform reference",
    addons::SECTION,
];
pub const USER_SECTIONS: &[&str] = &["Start", "Administrator guide", "Client guide"];

/// Pages in sidebar order (the order prev/next follows).
fn ordered(site: &Site) -> Vec<&Page> {
    site.sections
        .iter()
        .flat_map(|s| site.pages.iter().filter(move |p| p.section == *s))
        .collect()
}

fn layout(p: &Page, site: &Site, css_v: &str) -> String {
    let (body, toc) = doc::to_html(p);
    let order = ordered(site);
    let mut nav = String::new();
    for section in site.sections {
        let items: Vec<&&Page> = order.iter().filter(|q| q.section == *section).collect();
        if items.is_empty() {
            continue;
        }
        // Long groups (the addon pages) collapse unless the page is in them.
        let fold = items.len() > 20;
        let open = if items.iter().any(|q| q.slug == p.slug) {
            " open"
        } else {
            ""
        };
        if fold {
            nav.push_str(&format!("<div class=\"nav-group\"><details{open}><summary><p class=\"nav-title\">{} ({})</p></summary><ul>", esc(section), items.len()));
        } else {
            nav.push_str(&format!(
                "<div class=\"nav-group\"><p class=\"nav-title\">{}</p><ul>",
                esc(section)
            ));
        }
        for q in items {
            let cur = if q.slug == p.slug {
                " class=\"cur\" aria-current=\"page\""
            } else {
                ""
            };
            nav.push_str(&format!(
                "<li><a href=\"{}.html\"{cur}>{}</a></li>",
                q.slug,
                esc(&q.title)
            ));
        }
        nav.push_str(if fold {
            "</ul></details></div>"
        } else {
            "</ul></div>"
        });
    }
    let idx = order.iter().position(|q| q.slug == p.slug).unwrap_or(0);
    let mut pn = String::from("<nav class=\"pager\" aria-label=\"Previous and next page\">");
    if idx > 0 {
        let q = order[idx - 1];
        pn.push_str(&format!(
            "<a class=\"prev\" href=\"{}.html\"><span>Previous</span><strong>{}</strong></a>",
            q.slug,
            esc(&q.title)
        ));
    } else {
        pn.push_str("<span></span>");
    }
    if let Some(q) = order.get(idx + 1) {
        pn.push_str(&format!(
            "<a class=\"next\" href=\"{}.html\"><span>Next</span><strong>{}</strong></a>",
            q.slug,
            esc(&q.title)
        ));
    }
    pn.push_str("</nav>");
    let toc: String = toc
        .iter()
        .filter(|(l, _, _)| *l <= 3)
        .map(|(l, t, id)| {
            format!(
                "<li class=\"l{l}\"><a href=\"#{id}\">{}</a></li>",
                doc::inline_text(t)
            )
        })
        .collect();
    let (home, other_href, other_name) = if site.dir.is_empty() {
        ("", "developers/index.html", "Developers")
    } else {
        ("../", "../index.html", "User docs")
    };
    let switch = if site.dir.is_empty() {
        format!("<a class=\"set cur\" href=\"index.html\">User docs</a><a class=\"set\" href=\"{other_href}\">{other_name}</a>")
    } else {
        format!("<a class=\"set\" href=\"{other_href}\">{other_name}</a><a class=\"set cur\" href=\"index.html\">Developers</a>")
    };
    let base = base_url();
    let set_url = format!(
        "{base}{}",
        if site.dir.is_empty() {
            String::new()
        } else {
            format!("{}/", site.dir)
        }
    );
    let url = if p.slug == "index" {
        set_url.clone()
    } else {
        format!("{set_url}{}.html", p.slug)
    };
    let og = format!("{set_url}og.png");
    let desc = summary(p, site.name);
    let full_title = format!("{} · {} · PeltyCMS", p.title, site.name);
    let ld = format!(
        "[{{\"@context\":\"https://schema.org\",\"@type\":\"TechArticle\",\"headline\":{h},\"description\":{d},\"url\":{u},\"image\":{i},\"inLanguage\":\"en\",\"isPartOf\":{{\"@type\":\"WebSite\",\"name\":\"PeltyCMS\",\"url\":\"https://peltycms.com/\"}},\"publisher\":{{\"@type\":\"Organization\",\"name\":\"MythicalSystems Studios LLC\",\"logo\":{{\"@type\":\"ImageObject\",\"url\":{l}}}}}}},\
{{\"@context\":\"https://schema.org\",\"@type\":\"BreadcrumbList\",\"itemListElement\":[{{\"@type\":\"ListItem\",\"position\":1,\"name\":{sn},\"item\":{su}}},{{\"@type\":\"ListItem\",\"position\":2,\"name\":{h},\"item\":{u}}}]}}]",
        h = json_str(&p.title), d = json_str(&desc), u = json_str(&url), i = json_str(&og), l = json_str(&format!("{set_url}logo.png")),
        sn = json_str(site.name), su = json_str(&set_url),
    );
    let (ea, ed, eu, et) = (esc(&full_title), esc(&desc), esc(&url), esc(&og));
    let seo = format!(
        "<meta name=\"description\" content=\"{ed}\"><meta name=\"robots\" content=\"index,follow\"><meta name=\"theme-color\" content=\"#060b16\"><link rel=\"canonical\" href=\"{eu}\">\
<link rel=\"icon\" type=\"image/png\" sizes=\"32x32\" href=\"favicon.png\"><link rel=\"apple-touch-icon\" sizes=\"180x180\" href=\"apple-touch-icon.png\">\
<meta property=\"og:type\" content=\"article\"><meta property=\"og:site_name\" content=\"PeltyCMS\"><meta property=\"og:title\" content=\"{ea}\"><meta property=\"og:description\" content=\"{ed}\"><meta property=\"og:url\" content=\"{eu}\"><meta property=\"og:image\" content=\"{et}\"><meta property=\"og:image:width\" content=\"1200\"><meta property=\"og:image:height\" content=\"630\"><meta property=\"og:image:alt\" content=\"PeltyCMS logo\">\
<meta name=\"twitter:card\" content=\"summary_large_image\"><meta name=\"twitter:title\" content=\"{ea}\"><meta name=\"twitter:description\" content=\"{ed}\"><meta name=\"twitter:image\" content=\"{et}\">\
<script type=\"application/ld+json\">{ld}</script>"
    );
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title} · {set} · PeltyCMS</title>{seo}<link rel=\"stylesheet\" href=\"style.css?v={css_v}\"></head><body>\
<div class=\"prerelease\" role=\"note\"><strong>Not released yet.</strong> PeltyCMS is under development and not released yet; these docs describe the upcoming release and may change.</div>\
<a class=\"skip\" href=\"#content\">Skip to content</a>\
<header class=\"top\"><button id=\"menu\" class=\"icon-btn\" aria-label=\"Open navigation\" aria-controls=\"nav\" aria-expanded=\"false\"><span></span><span></span><span></span></button>\
<a class=\"brand\" href=\"{home}index.html\"><img class=\"logo\" src=\"logo.png\" width=\"28\" height=\"28\" alt=\"PeltyCMS logo\">PeltyCMS <span class=\"brand-set\">{set}</span></a>\
<div class=\"sets\">{switch}</div>\
<div class=\"search\"><input id=\"q\" type=\"search\" placeholder=\"Search docs\" aria-label=\"Search docs\" autocomplete=\"off\"><kbd>/</kbd><div id=\"results\" class=\"results\" role=\"listbox\" hidden></div></div>\
<span class=\"ver\">v{ver}</span></header>\
<div class=\"scrim\" id=\"scrim\" hidden></div>\
<div class=\"wrap\"><nav id=\"nav\" class=\"side\" aria-label=\"Docs navigation\">{nav}</nav>\
<main id=\"content\"><p class=\"crumbs\"><a href=\"index.html\">{set}</a><span>/</span>{section}</p><h1>{title}</h1>{body}{pn}\
<footer>Generated by <code>pelty-docs</code> from the PeltyCMS source. <a href=\"{slug}.md\">View as Markdown</a></footer></main>\
<aside class=\"toc\">{toc_block}</aside></div><script src=\"app.js?v={css_v}\"></script></body></html>",
        title = esc(&p.title),
        set = esc(site.name),
        section = esc(p.section),
        ver = env!("CARGO_PKG_VERSION"),
        slug = p.slug,
        toc_block = if toc.is_empty() { String::new() } else { format!("<p class=\"toc-title\">On this page</p><ul>{toc}</ul>") },
    )
}

/// Writes one set: pages (HTML + Markdown), assets, `pages.json`, `search.json`.
fn write_site(out: &Path, site: &Site) -> Result<(), String> {
    let dir = out.join(site.dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let w = |name: &str, body: &[u8]| {
        std::fs::write(dir.join(name), body).map_err(|e| format!("{}/{name}: {e}", site.dir))
    };
    let v = format!("{:x}", CSS.len() * 31 + JS.len());
    w("style.css", CSS.as_bytes())?;
    w("app.js", JS.as_bytes())?;
    w("logo.png", LOGO)?;
    w("favicon.png", FAVICON)?;
    w("apple-touch-icon.png", TOUCH)?;
    w("og.png", OG)?;
    let mut index = vec![];
    let mut search = vec![];
    for p in ordered(site) {
        w(&format!("{}.html", p.slug), layout(p, site, &v).as_bytes())?;
        w(&format!("{}.md", p.slug), doc::to_md(p).as_bytes())?;
        index.push(json!({ "slug": p.slug, "title": p.title, "section": p.section }));
        let heads: Vec<serde_json::Value> = p
            .blocks
            .iter()
            .filter_map(|b| match b {
                doc::Block::H(l, t) if *l <= 3 => Some(json!([t, doc::slugify(t)])),
                _ => None,
            })
            .collect();
        search.push(json!({ "s": p.slug, "t": p.title, "g": p.section, "h": heads }));
    }
    for p in &site.pages {
        assert!(
            site.sections.contains(&p.section),
            "page {} has unknown section {}",
            p.slug,
            p.section
        );
    }
    w(
        "pages.json",
        serde_json::to_string_pretty(&index).unwrap().as_bytes(),
    )?;
    w(
        "search.json",
        serde_json::to_string(&search).unwrap().as_bytes(),
    )?;
    Ok(())
}

/// Writes both sets; returns `(user pages, developer pages)`.
pub fn generate(out: &Path) -> Result<(usize, usize), String> {
    let spec = routes::full_spec();
    let (inv, _) = routes::inventory();
    let undocumented = spec["paths"]
        .as_object()
        .unwrap()
        .values()
        .flat_map(|i| i.as_object().unwrap().values())
        .filter(|o| o["x-undocumented"] == true)
        .count();
    let env = extract::env_vars();
    let migrations = extract::migrations();
    let caps = json!({
        "capabilities": extract::struct_fields(&extract::read("crates/pelty-wasm/src/runtime.rs"), "Capabilities"),
        "hook": extract::struct_fields(&extract::read("crates/pelty-wasm/src/runtime.rs"), "Hook"),
        "mixin": extract::struct_fields(&extract::read("crates/pelty-wasm/src/runtime.rs"), "Mixin"),
        "manifest": extract::struct_fields(&extract::read("crates/pelty-api/src/addon_host/addons.rs"), "Manifest"),
        "permissions": pelty_api::addons::PERMISSIONS,
        "phases": pelty_wasm::PHASES,
        "mixin_at": pelty_api::addon_events::MIXIN_AT,
        "filter": extract::struct_fields(&extract::read("crates/pelty-wasm/src/runtime.rs"), "Filter"),
        "filters": pelty_extensions::filters::FILTERS,
        "functions": pelty_extensions::functions::FUNCTIONS,
        "ui_node_types": pelty_extensions::ui_schema::NODE_TYPES,
        "ui_slots": pelty_extensions::ui_schema::UI_RENDER_SLOTS,
        "abi": pelty_wasm::runtime::ABI,
    });
    let sites = extract::emit_sites();
    let files: Vec<(&str, serde_json::Value, &str)> = vec![
        (
            "api.json",
            spec.clone(),
            "OpenAPI 3 document, every registered route",
        ),
        (
            "routes.json",
            serde_json::to_value(&inv).unwrap(),
            "route inventory (path, method, handler, source)",
        ),
        (
            "events.json",
            serde_json::to_value(pelty_extensions::events::REGISTRY).unwrap(),
            "event registry",
        ),
        (
            "emit-sites.json",
            serde_json::to_value(&sites).unwrap(),
            "where each event is emitted (file:line)",
        ),
        (
            "slots.json",
            json!({ "pages": slots::pages(), "named": slots::named(), "page_positions": pelty_extensions::ui_schema::PAGE_POSITIONS, "layout_positions": pelty_extensions::ui_schema::LAYOUT_POSITIONS }),
            "UI injection points per page",
        ),
        (
            "plugin-api.json",
            caps,
            "capabilities, hooks, mixins, manifest fields, UI schema",
        ),
        (
            "env.json",
            serde_json::to_value(&env).unwrap(),
            "environment variables",
        ),
        (
            "migrations.json",
            serde_json::to_value(&migrations).unwrap(),
            "database migrations",
        ),
    ];
    let ctx = pages::Ctx {
        spec,
        routes: inv.len(),
        undocumented,
        json_files: files
            .iter()
            .map(|(f, _, d)| (f.to_string(), d.to_string()))
            .collect(),
    };
    let mut all = pages::guides(&ctx);
    all.extend([
        pages::api(&ctx),
        reference::events(&sites),
        reference::cancellable(&ctx.spec),
        reference::hook_points(&ctx.spec),
        pages::hooks(),
        reference::widgets(),
        pages::capabilities(),
        pages::manifest(),
        pages::wit(),
        pages::ui(),
        pages::configuration(),
        pages::cli(),
        pages::migrations(),
        pages::architecture(),
        platform::settings_keys(),
        platform::email_templates(),
        platform::scheduled_tasks(),
    ]);
    for p in all.iter_mut() {
        if p.section != addons::SECTION {
            p.section = dev_section(&p.slug);
        }
    }
    all.extend(addons::pages());
    let user = userdocs::pages(&ctx);
    let mut dev = Site {
        dir: "developers",
        name: "Developer docs",
        sections: DEV_SECTIONS,
        pages: all,
    };
    let mut user = Site {
        dir: "",
        name: "User docs",
        sections: USER_SECTIONS,
        pages: user,
    };
    let cov = coverage::report(&dev, &user, &ctx);
    dev.pages.push(coverage::page(&cov));
    user.pages.insert(0, userdocs::landing(&dev, &user));
    let _ = std::fs::remove_dir_all(out);
    std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
    write_site(out, &user)?;
    write_site(out, &dev)?;
    let dev_dir = out.join("developers");
    for (f, v, _) in &files {
        std::fs::write(dev_dir.join(f), serde_json::to_string_pretty(v).unwrap())
            .map_err(|e| format!("{f}: {e}"))?;
    }
    std::fs::write(
        dev_dir.join("coverage.json"),
        serde_json::to_string_pretty(&cov).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    let layout = json!({
        "version": env!("CARGO_PKG_VERSION"),
        "sets": [
            { "dir": "", "name": user.name, "pages": user.pages.len(), "index": "index.html" },
            { "dir": "developers", "name": dev.name, "pages": dev.pages.len(), "index": "developers/index.html", "data": files.iter().map(|(f, _, d)| json!({ "file": format!("developers/{f}"), "description": d })).collect::<Vec<_>>() },
        ],
    });
    std::fs::write(
        out.join("docs.json"),
        serde_json::to_string_pretty(&layout).unwrap(),
    )
    .map_err(|e| e.to_string())?;
    Ok((user.pages.len(), dev.pages.len()))
}

/// Sidebar group of a developer page.
fn dev_section(slug: &str) -> &'static str {
    match slug {
        "index" | "getting-started" => "Get started",
        "plugin-rust"
        | "events-guide"
        | "ui-guide"
        | "other-languages"
        | "kinds"
        | "themes"
        | "security"
        | "addon-signing"
        | "addon-upgrades"
        | "addon-testing"
        | "addon-compatibility"
        | "addon-security-review"
        | "hooks-and-slots" => "Guides",
        "api" | "configuration" | "cli" | "migrations" | "settings-keys" | "email-templates"
        | "scheduled-tasks" | "coverage" => "Platform reference",
        _ => "Extension reference",
    }
}

#[cfg(test)]
mod tests {
    fn files(dir: &std::path::Path, ext: &str, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                files(&p, ext, out);
            } else if p.extension().is_some_and(|x| x == ext) {
                out.push(p);
            }
        }
    }

    #[test]
    fn generates_both_sets_with_no_broken_links() {
        let dir = std::env::temp_dir().join(format!("pelty-docs-test-{}", std::process::id()));
        let (u, d) = super::generate(&dir).unwrap();
        assert!(u >= 20 && d >= 80, "user {u}, developer {d}");
        for f in [
            "index.html",
            "admin-install.html",
            "client-account.html",
            "search.json",
            "docs.json",
            "style.css",
            "app.js",
        ] {
            assert!(dir.join(f).is_file(), "{f}");
        }
        for f in [
            "index.html",
            "api.html",
            "events.html",
            "cancellable.html",
            "widgets.html",
            "hook-points.html",
            "slots.json",
            "emit-sites.json",
            "plugin-rust.html",
            "api.json",
            "events.json",
            "configuration.md",
            "coverage.json",
            "coverage.html",
            "settings-keys.html",
            "email-templates.html",
            "scheduled-tasks.html",
            "addons.html",
            "addon-stripe.html",
            "pages.json",
            "search.json",
        ] {
            assert!(dir.join("developers").join(f).is_file(), "developers/{f}");
        }
        let html = std::fs::read_to_string(dir.join("developers/events.html")).unwrap();
        assert!(html.contains("invoice.paid") && html.contains("order.before_place"));
        // Code blocks are highlighted at build time and have a copy button.
        let hooks = std::fs::read_to_string(dir.join("developers/hooks.html")).unwrap();
        assert!(
            hooks.contains("<figure class=\"code\">")
                && hooks.contains("code-copy")
                && hooks.contains("class=\"hl-"),
            "hooks.html has no highlighted code block"
        );
        // Every marked snippet in the example addon appears in some guide.
        let src = super::extract::read(super::pages::GUIDE_SRC);
        let mut mds = vec![];
        files(&dir, "md", &mut mds);
        let all: String = mds
            .iter()
            .map(|p| std::fs::read_to_string(p).unwrap())
            .collect();
        for name in super::extract::snippet_names(&src) {
            assert!(
                all.contains(&format!("· snippet {name}_")),
                "snippet {name} not used by any page"
            );
        }
        // Every relative link (and #anchor) resolves, in both sets.
        let mut htmls = vec![];
        files(&dir, "html", &mut htmls);
        let href = regex::Regex::new(r#"href="([^"]+)""#).unwrap();
        let id = regex::Regex::new(r#" id="([^"]+)""#).unwrap();
        let mut ids: std::collections::HashMap<
            std::path::PathBuf,
            std::collections::HashSet<String>,
        > = Default::default();
        for f in &htmls {
            let h = std::fs::read_to_string(f).unwrap();
            ids.insert(
                f.canonicalize().unwrap(),
                id.captures_iter(&h).map(|c| c[1].to_string()).collect(),
            );
        }
        let mut broken = vec![];
        let mut checked = 0;
        for f in &htmls {
            let h = std::fs::read_to_string(f).unwrap();
            for c in href.captures_iter(&h) {
                let l = c[1].replace("&amp;", "&");
                if l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:")
                {
                    continue;
                }
                checked += 1;
                let (path, frag) = l
                    .split_once('#')
                    .map(|(a, b)| (a.to_string(), Some(b.to_string())))
                    .unwrap_or((l.clone(), None));
                let path = path.split('?').next().unwrap().to_string();
                let target = if path.is_empty() {
                    f.clone()
                } else {
                    f.parent().unwrap().join(&path)
                };
                match target.canonicalize() {
                    Ok(t) if t.is_file() => {
                        if let (Some(fr), Some(set)) = (frag, ids.get(&t)) {
                            if !fr.is_empty() && !set.contains(&fr) {
                                broken.push(format!("{} -> {l} (no anchor)", f.display()));
                            }
                        }
                    }
                    _ => broken.push(format!("{} -> {l}", f.display())),
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(checked > 1000, "only {checked} links checked");
        assert!(
            broken.is_empty(),
            "{} broken links:\n{}",
            broken.len(),
            broken.join("\n")
        );
    }

    /// Every registered event is found at least once in the source.
    #[test]
    fn every_event_has_an_emit_site() {
        let s = super::extract::emit_sites();
        for e in pelty_extensions::events::REGISTRY {
            assert!(s.contains_key(e.name), "no emit site found for {}", e.name);
        }
    }

    /// Hand-placed widget/nav slots are ones webhook plugins may target.
    #[test]
    fn named_plugin_slots_are_known() {
        for n in super::slots::named().iter().filter(|n| n.kind != "render") {
            assert!(
                pelty_extensions::plugins::UI_SLOTS.contains(&n.slot.as_str()),
                "{} not in UI_SLOTS",
                n.slot
            );
        }
    }
}

#[cfg(test)]
mod asset_tests {
    /// Docs pages must load only same-origin assets (CSP `'self'`) and the
    /// script must not assume optional elements exist (null.addEventListener).
    #[test]
    fn same_origin_assets_and_guarded_js() {
        let html = super::JS;
        for id in ["menu", "scrim", "q", "evq"] {
            assert!(
                !html.contains(&format!("$(\"#{id}\").addEventListener")),
                "{id} unguarded"
            );
        }
        assert!(html.contains("if (q && res)") && html.contains("if (scrim)"));
        let css = super::CSS;
        assert!(
            !css.contains("@import") && !css.contains("http://") && !css.contains("https://"),
            "external CSS hotlink"
        );
    }
}
