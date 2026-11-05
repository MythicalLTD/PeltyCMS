//! Route inventory parsed from `crates/pelty-api/src`: every `.route("…", …)`
//! call outside `#[cfg(test)]`, with its methods, handlers and mount prefix.
//! `full_spec` merges it with the utoipa document so every registered route
//! appears in the API reference, documented or not.
use regex::Regex;
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RouteEntry {
    /// Absolute URL path (`/api/v1/...` or a root path such as `/healthz`).
    pub path: String,
    /// Lowercase HTTP method.
    pub method: String,
    /// Handler expression, e.g. `billing::get_invoice`.
    pub handler: String,
    /// Source file relative to `crates/pelty-api/src`.
    pub source: String,
}

pub const API_PREFIX: &str = "/api/v1";

pub fn api_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../pelty-api/src")
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut v: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Source with `#[cfg(test)]` modules cut off (they are always at the end of a file here).
pub fn non_test(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

/// Number of `.route(` call sites outside tests (the parser must match all of them).
pub fn route_call_sites() -> usize {
    let mut files = vec![];
    rs_files(&api_src(), &mut files);
    files.iter().map(|f| non_test(&std::fs::read_to_string(f).unwrap()).matches(".route(").count()).sum()
}

/// Built-in plugin modules: `("id", fn_name())` pairs from `routes/plugins.rs::modules`.
fn plugin_modules(src: &str) -> Vec<(String, String)> {
    let re = Regex::new(r#"\("([a-z0-9-]+)",\s*([a-z_0-9]+)\(\)\)"#).unwrap();
    let body = src.split("pub fn modules()").nth(1).unwrap_or("");
    let body = body.split("\n}").next().unwrap_or("");
    re.captures_iter(body).map(|c| (c[1].to_string(), c[2].to_string())).collect()
}

/// Returns `(entries, call_sites_parsed)`.
pub fn inventory() -> (Vec<RouteEntry>, usize) {
    let root = api_src();
    let mut files = vec![];
    rs_files(&root, &mut files);
    let fn_re = Regex::new(r"(?m)^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:async\s+)?fn\s+([a-z_0-9]+)").unwrap();
    let m_re = Regex::new(r"(?:^|[^A-Za-z0-9_])(get|post|put|patch|delete|head|options|any)\(\s*([A-Za-z0-9_:]+|\||async\s+move\s+\||move\s+\|)").unwrap();
    let mut out = vec![];
    let mut sites = 0;
    for f in files {
        let rel = f.strip_prefix(&root).unwrap().display().to_string();
        let full = std::fs::read_to_string(&f).unwrap();
        let src = non_test(&full);
        let modules = if rel.ends_with("plugins.rs") { plugin_modules(src) } else { vec![] };
        let fns: Vec<(usize, String)> = fn_re.captures_iter(src).map(|c| (c.get(0).unwrap().start(), c[1].to_string())).collect();
        let mut at = 0;
        while let Some(i) = src[at..].find(".route(") {
            let start = at + i + ".route(".len();
            at = start;
            // Balanced argument list.
            let (mut depth, mut end) = (1i32, start);
            for (j, ch) in src[start..].char_indices() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let args = &src[start..end];
            let t = args.trim_start();
            let Some(t) = t.strip_prefix('"') else { continue };
            let Some(q) = t.find('"') else { continue };
            let path = &t[..q];
            let rest = &t[q + 1..];
            sites += 1;
            let in_fn = fns.iter().rev().find(|(p, _)| *p < start).map(|(_, n)| n.as_str()).unwrap_or("");
            let prefix = if rel == "lib.rs" || in_fn == "root_router" || rel == "routes/health.rs" {
                String::new()
            } else if let Some((id, _)) = modules.iter().find(|(_, f)| f == in_fn) {
                format!("{API_PREFIX}/plugins/{id}")
            } else {
                API_PREFIX.to_string()
            };
            if !m_re.is_match(rest) {
                panic!("{rel}: cannot read the methods of route {path}");
            }
            for c in m_re.captures_iter(rest) {
                let method = if &c[1] == "any" { "get".to_string() } else { c[1].to_string() };
                let h = c[2].trim_start_matches("axum::routing::");
                let handler = if h.ends_with('|') { format!("{in_fn}::{{closure}}") } else { h.to_string() };
                out.push(RouteEntry { path: format!("{prefix}{path}"), method, handler, source: rel.clone() });
            }
        }
    }
    out.sort_by(|a, b| (&a.path, &a.method).cmp(&(&b.path, &b.method)));
    out.dedup_by(|a, b| a.path == b.path && a.method == b.method);
    (out, sites)
}

fn tag_for(path: &str) -> &str {
    let p = path.strip_prefix(API_PREFIX).unwrap_or(path).trim_start_matches('/');
    let mut it = p.split('/');
    match it.next().unwrap_or("") {
        s @ ("admin" | "client" | "public") => it.next().map(|x| x.trim_start_matches('{')).unwrap_or(s),
        s => s,
    }
}

fn access_for(path: &str) -> &'static str {
    let p = path.strip_prefix(API_PREFIX).unwrap_or(path);
    if p.starts_with("/admin") {
        "staff"
    } else if p.starts_with("/client") || p.starts_with("/auth/me") {
        "customer"
    } else {
        "public or per-handler"
    }
}

/// utoipa document with absolute paths, plus a stub operation (`x-undocumented: true`)
/// for every registered route utoipa does not describe.
pub fn full_spec() -> Value {
    let mut spec = serde_json::to_value(<pelty_api::routes::openapi::ApiDoc as utoipa::OpenApi>::openapi()).unwrap();
    let (inv, _) = inventory();
    let roots: std::collections::HashSet<&str> = inv.iter().filter(|r| !r.path.starts_with(API_PREFIX)).map(|r| r.path.as_str()).collect();
    let old = spec["paths"].as_object().cloned().unwrap_or_default();
    let mut paths = Map::new();
    for (p, v) in old {
        let abs = if roots.contains(p.as_str()) || p.starts_with("/api/") { p } else { format!("{API_PREFIX}{p}") };
        paths.insert(abs, v);
    }
    for r in &inv {
        let item = paths.entry(r.path.clone()).or_insert_with(|| json!({}));
        let op = item.as_object_mut().unwrap();
        if let Some(o) = op.get_mut(&r.method) {
            o["x-handler"] = json!(r.handler);
            o["x-source"] = json!(format!("crates/pelty-api/src/{}", r.source));
            continue;
        }
        op.insert(
            r.method.clone(),
            json!({
                "operationId": r.handler.replace("::", "_"),
                "tags": [tag_for(&r.path)],
                "summary": r.handler.rsplit("::").next().unwrap().replace('_', " "),
                "description": format!("Registered route without a hand-written schema. Access: {}.", access_for(&r.path)),
                "x-undocumented": true,
                "x-handler": r.handler,
                "x-source": format!("crates/pelty-api/src/{}", r.source),
                "responses": { "default": { "description": "See handler source" } }
            }),
        );
    }
    spec["paths"] = Value::Object(paths);
    spec["servers"] = json!([{ "url": "/" }]);
    spec
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_sees_every_route_call() {
        let (inv, sites) = inventory();
        // `.route(` with a non-literal path would be missed: make that loud.
        assert_eq!(sites, route_call_sites(), "a `.route(` call has a non-literal path the inventory cannot read");
        assert!(inv.len() > 500, "only {} routes parsed", inv.len());
        assert!(inv.iter().any(|r| r.path == "/healthz" && r.method == "get"));
        assert!(inv.iter().any(|r| r.path == "/includes/api.php" && r.method == "post"));
        assert!(inv.iter().any(|r| r.path.starts_with("/api/v1/plugins/event-logger/")));
    }

    #[test]
    fn full_spec_covers_every_registered_route() {
        let spec = full_spec();
        let (inv, _) = inventory();
        let missing: Vec<_> = inv.iter().filter(|r| spec["paths"][&r.path][&r.method].is_null()).map(|r| format!("{} {}", r.method, r.path)).collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn every_utoipa_path_is_a_real_route() {
        // Catches schema paths that drifted from the router (typos, removed routes).
        let spec = full_spec();
        let (inv, _) = inventory();
        let mut stale = vec![];
        for (p, item) in spec["paths"].as_object().unwrap() {
            for m in item.as_object().unwrap().keys() {
                if !inv.iter().any(|r| &r.path == p && &r.method == m) {
                    stale.push(format!("{m} {p}"));
                }
            }
        }
        assert!(stale.is_empty(), "documented but not routed: {stale:?}");
    }
}
