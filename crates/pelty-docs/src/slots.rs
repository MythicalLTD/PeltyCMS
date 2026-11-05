//! Addon UI injection points discovered from the frontend source: every
//! page under the admin and client layouts (`frontend/src/App.tsx`), its
//! route key and the slot names the Shell renders for it, plus the named
//! slots placed by hand (`<PluginSlot slot="…">`, `<PluginWidgets slot="…">`).
use crate::extract::read;

#[derive(Debug, Clone, serde::Serialize)]
pub struct PageSlots {
    pub area: &'static str,
    /// Route as written in the router (`/admin/invoices/:id`).
    pub path: String,
    /// Page component (`AdminInvoicePage`).
    pub component: String,
    /// Route key used in slot names (`invoices`).
    pub key: String,
    /// The page has an id segment, so `detail-panel` renders.
    pub detail: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct NamedSlot {
    pub slot: String,
    /// `render` (node tree, `<PluginSlot>`), `widget` (`<PluginWidgets>`
    /// stat card from a webhook plugin) or `nav` (sidebar entries).
    pub kind: &'static str,
    pub files: Vec<String>,
    /// `ctx` keys passed by the page.
    pub ctx: Vec<String>,
}

/// Route key as computed by `routeKey()` in `frontend/src/lib/addons/addon-frame.ts`:
/// id segments are dropped. Router params named `id` are ids, except under
/// `hub/` where the id is a word and stays (`hub-<id>`); other params keep a
/// `<param>` placeholder because their value is part of the key.
pub fn route_key(path: &str, area: &str) -> (String, bool) {
    let mut segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if area == "admin" && segs.first() == Some(&"admin") {
        segs.remove(0);
    }
    let mut out = vec![];
    let mut detail = false;
    for (i, s) in segs.iter().enumerate() {
        match s.strip_prefix(':') {
            Some("id") if i == 0 || segs[i - 1] != "hub" => detail = true,
            Some(p) => out.push(format!("<{p}>")),
            None => out.push(s.to_string()),
        }
    }
    (if out.is_empty() { "dashboard".into() } else { out.join("-") }, detail)
}

fn block_end(lines: &[&str], start: usize) -> usize {
    let ind = lines[start].len() - lines[start].trim_start().len();
    (start + 1..lines.len()).find(|&i| lines[i].trim() == "</Route>" && lines[i].len() - lines[i].trim_start().len() == ind).unwrap_or(lines.len())
}

/// Pages rendered inside `Shell` (admin and client layouts), in router order.
pub fn pages() -> Vec<PageSlots> {
    let src = read("frontend/src/App.tsx");
    let lines: Vec<&str> = src.lines().collect();
    let re = regex::Regex::new(r#"<Route (?:path="([^"]+)"|(index)) element=\{(.*)\}\s*/>"#).unwrap();
    let comp = regex::Regex::new(r#"<([A-Z][A-Za-z]+Page)\b"#).unwrap();
    let mut out = vec![];
    for (area, marker) in [("client", "<ClientLayout"), ("admin", "<AdminLayout")] {
        let Some(start) = lines.iter().position(|l| l.contains("<Route") && l.contains(marker)) else { continue };
        for l in &lines[start + 1..block_end(&lines, start)] {
            let Some(c) = re.captures(l) else { continue };
            if c[3].contains("<Navigate") || c.get(1).is_some_and(|m| m.as_str() == "*") {
                continue;
            }
            let raw = c.get(1).map(|m| m.as_str()).unwrap_or("");
            let path = match (area, raw) {
                ("admin", "") => "/admin".to_string(),
                ("admin", p) => format!("/admin/{p}"),
                (_, p) => p.to_string(),
            };
            let component = comp.captures(&c[3]).map(|m| m[1].to_string()).unwrap_or_default();
            let (key, detail) = route_key(&path, area);
            out.push(PageSlots { area, path, component, key, detail });
        }
    }
    out
}

fn ts_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            ts_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "tsx" || x == "ts") && !p.to_string_lossy().contains(".test.") {
            out.push(p);
        }
    }
}

/// Every literal `slot="…"` on `<PluginSlot>`, `<PluginWidgets>` and `<Shell>`.
pub fn named() -> Vec<NamedSlot> {
    let root = crate::extract::repo();
    let mut files = vec![];
    ts_files(&root.join("frontend/src"), &mut files);
    files.sort();
    let re = regex::Regex::new(r#"<(PluginSlot|PluginWidgets|Shell) slot="([a-z.]+)"([^>]*)/?>"#).unwrap();
    let ctx_re = regex::Regex::new(r#"([a-z_]+):"#).unwrap();
    let mut out: Vec<NamedSlot> = vec![];
    for f in files {
        let s = std::fs::read_to_string(&f).unwrap_or_default();
        let rel = f.strip_prefix(&root).unwrap().display().to_string();
        for c in re.captures_iter(&s) {
            let kind = match &c[1] {
                "PluginSlot" => "render",
                "PluginWidgets" => "widget",
                _ => "nav",
            };
            let ctx: Vec<String> = c[3].split_once("ctx={{").map(|(_, r)| ctx_re.captures_iter(r).map(|m| m[1].to_string()).collect()).unwrap_or_default();
            match out.iter_mut().find(|n| n.slot == c[2] && n.kind == kind) {
                Some(n) => {
                    if !n.files.contains(&rel) {
                        n.files.push(rel.clone());
                    }
                }
                None => out.push(NamedSlot { slot: c[2].to_string(), kind, files: vec![rel.clone()], ctx }),
            }
        }
    }
    out.sort_by(|a, b| (a.kind, &a.slot).cmp(&(b.kind, &b.slot)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_match_frontend_route_key() {
        assert_eq!(route_key("/admin", "admin"), ("dashboard".into(), false));
        assert_eq!(route_key("/admin/invoices/:id", "admin"), ("invoices".into(), true));
        assert_eq!(route_key("/admin/hub/:id", "admin"), ("hub-<id>".into(), false));
        assert_eq!(route_key("/invoices/:id/pay", "client"), ("invoices-pay".into(), true));
        assert_eq!(route_key("/p/:plugin/:page", "client"), ("p-<plugin>-<page>".into(), false));
    }
    #[test]
    fn discovers_pages_and_slots() {
        let p = pages();
        assert!(p.iter().filter(|x| x.area == "admin").count() >= 60, "{}", p.len());
        assert!(p.iter().filter(|x| x.area == "client").count() >= 20);
        assert!(p.iter().any(|x| x.path == "/admin/invoices/:id" && x.key == "invoices" && x.detail));
        let n = named();
        for (s, _) in pelty_extensions::ui_schema::UI_RENDER_SLOTS {
            assert!(n.iter().any(|x| x.slot == *s && x.kind == "render"), "{s} not found in the frontend");
        }
        assert!(n.iter().any(|x| x.slot == "admin.nav" && x.kind == "nav"));
    }
    /// Every page/layout position the backend accepts is rendered somewhere.
    #[test]
    fn every_position_is_rendered_by_the_frontend() {
        let shell = read("frontend/src/layouts/Shell.tsx") + &read("frontend/src/components/PageHeader.tsx") + &read("frontend/src/components/plugin-ui/AddonSlots.tsx");
        for p in pelty_extensions::ui_schema::LAYOUT_POSITIONS.iter().chain(pelty_extensions::ui_schema::PAGE_POSITIONS) {
            assert!(shell.contains(&format!("\"{p}\"")), "position {p} is not rendered");
        }
    }
}
