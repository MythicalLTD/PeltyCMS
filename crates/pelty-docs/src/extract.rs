//! Source extractors: struct fields with doc comments, trait/fn source,
//! marked snippets, env vars and migrations.
use std::path::{Path, PathBuf};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

pub fn read(rel: &str) -> String {
    std::fs::read_to_string(repo().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FieldDoc {
    pub name: String,
    pub ty: String,
    pub doc: String,
    /// `#[serde(default…)]` present: may be omitted.
    pub optional: bool,
}

fn doc_join(lines: &[String]) -> String {
    let mut s = String::new();
    for l in lines {
        if l.is_empty() {
            s.push(' ');
        } else {
            if !s.is_empty() && !s.ends_with(' ') {
                s.push(' ');
            }
            s.push_str(l);
        }
    }
    s.trim().to_string()
}

/// Doc comment (`///`) directly above `pub struct|enum|trait|fn|const <name>`.
pub fn item_doc(src: &str, name: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let pats = [
        "struct", "enum", "trait", "fn", "const", "static", "async fn",
    ]
    .map(|k| format!("pub {k} {name}"));
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if pats.iter().any(|p| {
            t.starts_with(p.as_str())
                && t[p.len()..].starts_with(|c: char| !c.is_alphanumeric() && c != '_')
        }) {
            let mut doc = vec![];
            let mut j = i;
            while j > 0 {
                j -= 1;
                let u = lines[j].trim();
                if let Some(d) = u.strip_prefix("///") {
                    doc.push(d.trim().to_string());
                } else if u.starts_with("#[") {
                    continue;
                } else {
                    break;
                }
            }
            doc.reverse();
            return doc_join(&doc);
        }
    }
    String::new()
}

/// Named fields of `pub struct <name> { … }`.
pub fn struct_fields(src: &str, name: &str) -> Vec<FieldDoc> {
    let start = src
        .find(&format!("pub struct {name} {{"))
        .or_else(|| src.find(&format!("pub struct {name}<")))
        .unwrap_or_else(|| panic!("struct {name} not found"));
    let body = &src[start..];
    let body = &body[body.find('{').unwrap() + 1..];
    let mut out = vec![];
    let (mut doc, mut optional) = (vec![], false);
    let mut depth = 0i32;
    for l in body.lines() {
        let t = l.trim();
        if depth == 0 && t.starts_with('}') {
            break;
        }
        if let Some(d) = t.strip_prefix("///") {
            doc.push(d.trim().to_string());
            continue;
        }
        if t.starts_with("#[") {
            if t.contains("default") || t.contains("skip_serializing_if") {
                optional = true;
            }
            continue;
        }
        if depth == 0 {
            if let Some(f) = t.strip_prefix("pub ") {
                if let Some((n, ty)) = f.split_once(':') {
                    let ty = ty.trim().trim_end_matches(',').to_string();
                    let opt = optional || ty.starts_with("Option<");
                    out.push(FieldDoc {
                        name: n.trim().to_string(),
                        ty,
                        doc: doc_join(&doc),
                        optional: opt,
                    });
                }
            }
        }
        depth +=
            t.matches(['{', '(', '[']).count() as i32 - t.matches(['}', ')', ']']).count() as i32;
        doc.clear();
        optional = false;
    }
    out
}

/// Full source of an item starting at `pub trait <name>` (balanced braces).
pub fn item_source(src: &str, head: &str) -> String {
    let start = src.find(head).unwrap_or_else(|| panic!("{head} not found"));
    let mut line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    // Include doc comments above.
    let before: Vec<&str> = src[..line_start].lines().collect();
    let mut k = before.len();
    while k > 0
        && (before[k - 1].trim().starts_with("///") || before[k - 1].trim().starts_with("#["))
    {
        k -= 1;
        line_start -= before[k].len() + 1;
    }
    let (mut depth, mut end) = (0i32, src.len());
    let mut seen = false;
    for (i, c) in src[start..].char_indices() {
        match c {
            '{' => {
                depth += 1;
                seen = true;
            }
            '}' => {
                depth -= 1;
                if seen && depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            ';' if !seen => {
                end = start + i + 1;
                break;
            }
            _ => {}
        }
    }
    src[line_start..end].to_string()
}

/// Text between `// docs:start:<name>` and `// docs:end:<name>` (dedented, nested markers removed).
pub fn snippet(src: &str, name: &str) -> Option<String> {
    let marker = format!("// docs:start:{name}");
    let a = src.find(&marker)?;
    let body = &src[a + marker.len()..];
    let body = body
        .strip_prefix("\r\n")
        .or_else(|| body.strip_prefix('\n'))?;
    let e = body.find(&format!("// docs:end:{name}"))?;
    let body = &body[..e];
    let lines: Vec<&str> = body
        .lines()
        .filter(|l| !l.trim_start().starts_with("// docs:"))
        .collect();
    let ind = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    Some(
        lines
            .iter()
            .map(|l| if l.len() >= ind { &l[ind..] } else { l.trim() })
            .collect::<Vec<_>>()
            .join("\n")
            .trim_end()
            .to_string(),
    )
}

pub fn snippet_names(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|l| {
            l.trim()
                .strip_prefix("// docs:start:")
                .map(|s| s.trim().to_string())
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EnvVar {
    pub name: String,
    pub default: String,
    pub doc: String,
    pub used_in: Vec<String>,
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    v.sort();
    for p in v {
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `"PELTY_*"` / `DATABASE_URL` read in `crates/*/src`, with the
/// `Config` field doc and default when it comes from `pelty-core/src/config.rs`.
pub fn env_vars() -> Vec<EnvVar> {
    let root = repo();
    let cfg = read("crates/pelty-core/src/config.rs");
    let fields = struct_fields(&cfg, "Config");
    let re = regex::Regex::new(r#""((?:PELTY_[A-Z0-9_]+)|DATABASE_URL)""#).unwrap();
    let mut map: std::collections::BTreeMap<String, EnvVar> = Default::default();
    let mut files = vec![];
    for c in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        rs_files(&c.path().join("src"), &mut files);
    }
    for f in files {
        // Normalize so Windows `\` matches the `/` skips below (and used_in paths).
        let rel = f
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with("crates/pelty-docs") || rel.ends_with("/tests.rs") {
            continue;
        }
        let s = std::fs::read_to_string(&f).unwrap();
        let s = crate::routes::non_test(&s);
        for c in re.captures_iter(s) {
            let e = map.entry(c[1].to_string()).or_insert_with(|| EnvVar {
                name: c[1].to_string(),
                default: String::new(),
                doc: String::new(),
                used_in: vec![],
            });
            if !e.used_in.contains(&rel) {
                e.used_in.push(rel.clone());
            }
        }
    }
    // Config::from_env: `field: parse("PELTY_X", default)` / `var("PELTY_X")`.
    let fre = regex::Regex::new(
        r#"(?s)([a-z_]+):\s*(?:parse|var)\(\s*"([A-Z_]+)"(?:,\s*(.*?))?\)\??[\s.]*(?:map|,|\n)"#,
    )
    .unwrap();
    for c in fre.captures_iter(&cfg) {
        if let Some(e) = map.get_mut(&c[2]) {
            if let Some(f) = fields.iter().find(|f| f.name == c[1]) {
                e.doc = f.doc.clone();
            }
            if let Some(d) = c.get(3) {
                let d = d.as_str().trim();
                let d = d
                    .split(".parse()")
                    .next()
                    .unwrap()
                    .trim_start_matches("PathBuf::from(")
                    .trim_end_matches(".to_string()")
                    .trim_matches(|c| c == '(' || c == ')');
                if d.len() < 60 {
                    e.default = d.to_string();
                }
            }
        }
    }
    if let Some(e) = map.get_mut("DATABASE_URL") {
        e.doc = "PostgreSQL connection URL (`postgres://…`). Required.".into();
    }
    // Free-standing reads: take the nearest comment above the first use that
    // has one (the setup wizard and bootstrap also mention many variables).
    let dre =
        regex::Regex::new(r#"unwrap_or(?:_else)?\((?:\|_\|\s*)?(?:[A-Za-z:]*from\()?"([^"]+)""#)
            .unwrap();
    for e in map.values_mut().filter(|e| e.doc.is_empty()) {
        for file in e.used_in.clone() {
            if !e.doc.is_empty() {
                break;
            }
            let s = read(&file);
            if let Some(i) = s.find(&format!("\"{}\"", e.name)) {
                let line = s[i..].lines().next().unwrap_or("");
                if let Some(d) = dre.captures(line) {
                    e.default = format!("\"{}\"", &d[1]);
                }
                let before: Vec<&str> = s[..i].lines().collect();
                let mut doc = vec![];
                for l in before.iter().rev().skip(1).take(6) {
                    let t = l.trim();
                    if let Some(d) = t.strip_prefix("///").or_else(|| t.strip_prefix("//")) {
                        doc.push(d.trim().to_string());
                    } else {
                        break;
                    }
                }
                doc.reverse();
                e.doc = doc_join(&doc);
            }
        }
    }
    map.into_values().collect()
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Migration {
    pub file: String,
    pub summary: String,
}

pub fn migrations() -> Vec<Migration> {
    let dir = repo().join("migrations");
    let mut v: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "sql"))
        .collect();
    v.sort();
    v.iter()
        .map(|p| {
            let s = std::fs::read_to_string(p).unwrap();
            let summary: Vec<String> = s
                .lines()
                .take_while(|l| l.starts_with("--"))
                .map(|l| l.trim_start_matches('-').trim().to_string())
                .collect();
            Migration {
                file: p.file_name().unwrap().to_string_lossy().into(),
                summary: doc_join(&summary),
            }
        })
        .collect()
}

/// `//!` module doc of a file.
pub fn module_doc(src: &str) -> String {
    src.lines()
        .take_while(|l| l.starts_with("//!"))
        .map(|l| {
            l.trim_start_matches("//!")
                .strip_prefix(' ')
                .unwrap_or(l.trim_start_matches("//!"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One Rust module of the workspace and the first paragraph of its `//!` doc.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModuleInfo {
    pub krate: String,
    pub path: String,
    pub summary: String,
}

/// Every `.rs` file under `crates/*/src`, with its `//!` summary (empty when missing).
pub fn modules() -> Vec<ModuleInfo> {
    let root = repo().join("crates");
    let mut crates: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src").is_dir())
        .collect();
    crates.sort();
    // Rustdoc intra-doc links do not resolve in the HTML docs; keep only their text.
    let link = regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)").unwrap();
    let bare = regex::Regex::new(r"\[(`[^`]*`)\]").unwrap();
    let mut out = vec![];
    for c in crates {
        let mut files = vec![];
        rs_files(&c.join("src"), &mut files);
        for f in files {
            let doc = module_doc(&std::fs::read_to_string(&f).unwrap_or_default());
            let first = doc.split("\n\n").next().unwrap_or("").replace('\n', " ");
            let first = link.replace_all(&first, "$1");
            let summary = bare.replace_all(&first, "$1").trim().to_string();
            out.push(ModuleInfo {
                krate: c.file_name().unwrap().to_string_lossy().into_owned(),
                path: f.strip_prefix(repo()).unwrap().display().to_string(),
                summary,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_module_has_a_doc() {
        let mods = modules();
        assert!(mods.len() > 300, "module scan found only {}", mods.len());
        let missing: Vec<_> = mods
            .iter()
            .filter(|m| m.summary.is_empty())
            .map(|m| m.path.as_str())
            .collect();
        assert!(missing.is_empty(), "add a `//!` doc to: {missing:?}");
    }
    #[test]
    fn extracts_fields_and_snippets() {
        let src = "/// A thing.\npub struct X {\n    /// The id.\n    pub id: String,\n    #[serde(default)]\n    pub n: Vec<(u8, u8)>,\n}\n// docs:start:a\n    let x = 1;\n    // docs:start:b\n    let y = 2;\n    // docs:end:b\n// docs:end:a\n";
        let f = struct_fields(src, "X");
        assert_eq!(f.len(), 2);
        assert_eq!(
            (f[0].doc.as_str(), f[0].optional, f[1].optional),
            ("The id.", false, true)
        );
        assert_eq!(item_doc(src, "X"), "A thing.");
        assert_eq!(snippet(src, "a").unwrap(), "let x = 1;\nlet y = 2;");
        assert_eq!(snippet(src, "b").unwrap(), "let y = 2;");
        assert_eq!(
            snippet(
                "// docs:start:a\r\n    let x = 1;\r\n// docs:end:a\r\n",
                "a"
            )
            .unwrap(),
            "let x = 1;"
        );
    }
    #[test]
    fn local_postgres_env_settings_are_documented() {
        let vars = env_vars();
        for (name, detail) in [
            ("PELTY_LOCAL_PG", "true or 1"),
            ("PELTY_LOCAL_PG_PORT", "5432"),
            ("PELTY_PG_HOME", "data/pg"),
        ] {
            let setting = vars.iter().find(|v| v.name == name).expect(name);
            assert!(setting.doc.contains(detail), "{name}: {}", setting.doc);
        }
    }

    #[test]
    fn env_vars_come_from_code() {
        let v = env_vars();
        let bind = v.iter().find(|e| e.name == "PELTY_BIND").unwrap();
        assert!(bind.default.contains("0.0.0.0:8080"), "{bind:?}");
        assert!(v.iter().any(|e| e.name == "DATABASE_URL"));
        assert!(v.len() >= 20);
        let undocumented: Vec<_> = v
            .iter()
            .filter(|e| e.doc.is_empty())
            .map(|e| e.name.as_str())
            .collect();
        assert!(
            undocumented.is_empty(),
            "add a doc comment where these are read: {undocumented:?}"
        );
    }
}

/// Every `.rs` file under `crates/*/src` (unit-test modules cut off).
fn crate_sources() -> Vec<(String, String)> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let root = repo();
    let mut files = vec![];
    for c in std::fs::read_dir(root.join("crates"))
        .into_iter()
        .flatten()
        .flatten()
    {
        walk(&c.path().join("src"), &mut files);
    }
    files.sort();
    files
        .into_iter()
        .filter_map(|f| {
            let s = std::fs::read_to_string(&f).ok()?;
            let s = s.split("#[cfg(test)]").next().unwrap_or("").to_string();
            Some((f.strip_prefix(&root).ok()?.display().to_string(), s))
        })
        .collect()
}

/// Where each event is emitted: `event -> ["crates/…/file.rs:LINE", …]`.
/// Literal names after `DomainEvent::new(` and `addon_ext::pre_event(`, plus
/// `Some("<event>")` arms in files that emit a computed name.
pub fn emit_sites() -> std::collections::BTreeMap<String, Vec<String>> {
    let lit = regex::Regex::new(
        r#"(?s)(DomainEvent::new\(|addon_ext::pre_event\()([^"]{0,80}?)"([a-z_.]+)""#,
    )
    .unwrap();
    let computed = regex::Regex::new(r#"DomainEvent::new\(\s*[a-z_]+\s*,"#).unwrap();
    let arm = regex::Regex::new(r#"Some\("([a-z_]+\.[a-z_.]+)"\)"#).unwrap();
    let mut m: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    let line = |s: &str, at: usize| s[..at].matches('\n').count() + 1;
    for (f, s) in crate_sources() {
        for c in lit.captures_iter(&s) {
            // `pre_event(s, "x"` skips one argument; `new("x"` none.
            if c[1].starts_with("DomainEvent") && !c[2].trim().is_empty() {
                continue;
            }
            m.entry(c[3].to_string())
                .or_default()
                .push(format!("{f}:{}", line(&s, c.get(3).unwrap().start())));
        }
        if computed.is_match(&s) {
            for c in arm.captures_iter(&s) {
                if crate::reference::is_registered(&c[1]) {
                    m.entry(c[1].to_string())
                        .or_default()
                        .push(format!("{f}:{}", line(&s, c.get(1).unwrap().start())));
                }
            }
        }
    }
    for v in m.values_mut() {
        v.dedup();
    }
    m
}

/// Non-test `.ts`/`.tsx` sources under `frontend/src`, `(relative path, text)`.
pub fn frontend_sources() -> Vec<(String, String)> {
    fn walk(d: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "ts" || x == "tsx")
                && !p.to_string_lossy().contains(".test.")
            {
                out.push(p);
            }
        }
    }
    let root = repo();
    let mut files = vec![];
    walk(&root.join("frontend/src"), &mut files);
    files.sort();
    files
        .into_iter()
        .filter_map(|f| {
            Some((
                f.strip_prefix(&root).ok()?.display().to_string(),
                std::fs::read_to_string(&f).ok()?,
            ))
        })
        .collect()
}

/// A `system_settings` key the code reads or writes.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SettingKey {
    pub key: String,
    /// Const names that hold the key (empty for a literal).
    pub consts: Vec<String>,
    pub files: Vec<String>,
    pub doc: String,
}

/// Every `system_settings` key: arguments of `settings::get/set(pool, X)`
/// (string literals, or consts resolved in the same file, then in the file
/// named by the path, then anywhere when unique), `key = '…'` literals in raw
/// SQL on `system_settings`, and `*_KEY` consts of files that query
/// `system_settings` directly. Returns (keys, unresolved arguments).
pub fn settings_keys() -> (Vec<SettingKey>, Vec<String>) {
    let root = repo();
    let mut files = vec![];
    for c in std::fs::read_dir(root.join("crates")).unwrap().flatten() {
        rs_files(&c.path().join("src"), &mut files);
    }
    let srcs: Vec<(String, String)> = files
        .iter()
        .map(|f| {
            (
                f.strip_prefix(&root).unwrap().display().to_string(),
                crate::routes::non_test(&std::fs::read_to_string(f).unwrap()).to_string(),
            )
        })
        .filter(|(r, _)| !r.starts_with("crates/pelty-docs"))
        .collect();
    let const_re = regex::Regex::new(
        r#"(?m)^[ \t]*(?:pub(?:\([a-z]+\))? )?const ([A-Z0-9_]+): &str = "([^"]+)";"#,
    )
    .unwrap();
    let consts_in = |src: &str| -> Vec<(String, String)> {
        const_re
            .captures_iter(src)
            .map(|c| (c[1].to_string(), c[2].to_string()))
            .collect()
    };
    let call_re =
        regex::Regex::new(r#"settings::(?:get|set)\(\s*[^,]+,\s*([A-Za-z0-9_:"./-]+)"#).unwrap();
    let sql_re =
        regex::Regex::new(r#"system_settings[^"]{0,160}?key\s*=\s*'([a-z0-9_.]+)'"#).unwrap();
    let mut map: std::collections::BTreeMap<String, SettingKey> = Default::default();
    let mut unresolved = vec![];
    let mut add = |key: &str, cname: Option<&str>, file: &str, src: &str| {
        let e = map.entry(key.to_string()).or_insert_with(|| SettingKey {
            key: key.into(),
            consts: vec![],
            files: vec![],
            doc: String::new(),
        });
        if let Some(n) = cname {
            if !e.consts.iter().any(|x| x == n) {
                e.consts.push(n.into());
            }
            if e.doc.is_empty() {
                e.doc = item_doc(src, n);
            }
        }
        if !e.files.iter().any(|x| x == file) {
            e.files.push(file.into());
        }
    };
    for (rel, src) in &srcs {
        let local = consts_in(src);
        for c in call_re.captures_iter(src) {
            let arg = &c[1];
            if let Some(lit) = arg.strip_prefix('"') {
                add(lit.trim_end_matches('"'), None, rel, src);
                continue;
            }
            let name = arg.rsplit("::").next().unwrap();
            if !name
                .chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
            {
                unresolved.push(format!("{rel}: {arg}"));
                continue;
            }
            let module = arg.rsplit("::").nth(1);
            let found = local
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| (v.clone(), rel.clone(), src.clone()))
                .or_else(|| {
                    let m = module?;
                    srcs.iter()
                        .filter(|(r, _)| r.ends_with(&format!("/{m}.rs")))
                        .find_map(|(r, s)| {
                            consts_in(s)
                                .into_iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, v)| (v, r.clone(), s.clone()))
                        })
                })
                .or_else(|| {
                    let all: Vec<_> = srcs
                        .iter()
                        .filter_map(|(r, s)| {
                            consts_in(s)
                                .into_iter()
                                .find(|(n, _)| n == name)
                                .map(|(_, v)| (v, r.clone(), s.clone()))
                        })
                        .collect();
                    (all.len() == 1).then(|| all.into_iter().next().unwrap())
                });
            match found {
                Some((v, def_file, def_src)) => {
                    add(&v, Some(name), &def_file, &def_src);
                    add(&v, None, rel, src);
                }
                None => unresolved.push(format!("{rel}: {arg}")),
            }
        }
        for c in sql_re.captures_iter(src) {
            add(&c[1], None, rel, src);
        }
        if src.contains("system_settings") {
            for (n, v) in &local {
                if n.ends_with("_KEY") || n == "KEY" {
                    add(v, Some(n), rel, src);
                }
            }
        }
    }
    unresolved.sort();
    unresolved.dedup();
    (map.into_values().collect(), unresolved)
}

/// `locales/en.json`, the English strings the UI shows.
pub fn en() -> std::collections::BTreeMap<String, String> {
    serde_json::from_str(&read("locales/en.json")).unwrap()
}

/// One entry of the frontend navigation registry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NavLeaf {
    pub category: String,
    pub category_label: String,
    pub path: String,
    pub label: String,
    pub desc: String,
}

/// `ADMIN_NAV` or `CLIENT_NAV_TREE` from `frontend/src/lib/navRegistry.ts`,
/// with English labels.
pub fn nav(scope: &str) -> Vec<NavLeaf> {
    let src = read("frontend/src/lib/navRegistry.ts");
    let start = if scope == "admin" {
        "export const ADMIN_NAV"
    } else {
        "export const CLIENT_NAV_TREE"
    };
    let body = src
        .split(start)
        .nth(1)
        .unwrap()
        .split("\n];")
        .next()
        .unwrap();
    let en = en();
    let cat_re = regex::Regex::new(r#"\{ id: "([a-z_-]+)", label: "([a-z_.]+)""#).unwrap();
    let leaf_re =
        regex::Regex::new(r#"\{ to: "([^"]+)", label: "([a-z_.]+)", desc: "([a-z_.]+)""#).unwrap();
    let mut out = vec![];
    let (mut cat, mut cat_label) = (String::new(), String::new());
    for l in body.lines() {
        if let Some(c) = cat_re.captures(l) {
            cat = c[1].to_string();
            cat_label = en.get(&c[2]).cloned().unwrap_or_else(|| c[2].to_string());
        }
        if let Some(c) = leaf_re.captures(l) {
            out.push(NavLeaf {
                category: cat.clone(),
                category_label: cat_label.clone(),
                path: c[1].to_string(),
                label: en.get(&c[2]).cloned().unwrap_or_else(|| c[2].to_string()),
                desc: en.get(&c[3]).cloned().unwrap_or_default(),
            });
        }
    }
    out
}

/// Steps of the first-run setup checklist (`pelty-database/src/config/setup_wizard.rs`).
pub fn setup_steps() -> Vec<(String, bool, String)> {
    let re = regex::Regex::new(
        r#"(?s)Step \{\s*id: "([a-z_]+)",\s*done: .*?,\s*optional: (true|false),\s*href: "([^"]+)""#,
    )
    .unwrap();
    re.captures_iter(&read("crates/pelty-database/src/config/setup_wizard.rs"))
        .map(|c| (c[1].to_string(), &c[2] == "true", c[3].to_string()))
        .collect()
}
