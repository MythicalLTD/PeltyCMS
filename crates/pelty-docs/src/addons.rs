//! One page per bundled addon, generated from its `addon.json` plus the
//! setting/field declarations found in its Rust source, and a catalog page.
use crate::doc::{slugify, Page};
use crate::extract::{read, repo};
use serde_json::Value;
use std::path::PathBuf;

pub const SECTION: &str = "Bundled addons";

#[derive(Debug, Clone, serde::Serialize)]
pub struct Addon {
    /// Folder under `extensions/` (`addons/stripe`).
    pub dir: String,
    pub slug: String,
    pub manifest: Value,
    /// Admin settings (addon.json `settings` + `SettingField`s in the source).
    pub settings: Vec<(String, String, bool)>,
    pub server_fields: Vec<(String, String, bool)>,
    pub product_fields: Vec<(String, String, bool)>,
    pub actions: Vec<String>,
    pub fixture: bool,
}

impl Addon {
    pub fn id(&self) -> &str {
        self.manifest["id"].as_str().unwrap_or("")
    }
    pub fn kind(&self) -> &str {
        self.manifest["kind"].as_str().unwrap_or("")
    }
}

fn locale() -> Value {
    serde_json::from_str(&read("locales/en.json")).unwrap_or(Value::Null)
}

/// Rust sources of an addon, unit tests cut off.
fn sources(dir: &std::path::Path) -> String {
    let mut out = String::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("src"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    for f in files {
        let s = std::fs::read_to_string(f).unwrap_or_default();
        out.push_str(s.split("#[cfg(test)]").next().unwrap_or(""));
        out.push('\n');
    }
    out
}

/// Body of the first `fn <name>` (up to the next item at the same indent).
fn fn_body<'a>(src: &'a str, needle: &str) -> Option<&'a str> {
    let at = src.find(needle)?;
    let rest = &src[at..];
    let end = rest[1..]
        .find("\n    fn ")
        .or_else(|| rest[1..].find("\nfn "))
        .or_else(|| rest[1..].find("\npub fn "))
        .map(|e| e + 1)
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

fn fields(s: &str) -> Vec<(String, String, bool)> {
    let re = regex::Regex::new(r#"(?:fld!|\bf|SettingField::of|SettingField::text|SettingField::required)\(\s*"([a-z0-9_]+)",\s*(?:"([a-z]+)"(?:,\s*(true|false))?|(true|false))"#).unwrap();
    re.captures_iter(s)
        .map(|c| {
            let kind = c.get(2).map(|m| m.as_str()).unwrap_or("text").to_string();
            let req = c.get(3).or(c.get(4)).is_some_and(|m| m.as_str() == "true");
            (c[1].to_string(), kind, req)
        })
        .collect()
}

/// A module field: key, label, required.
type Field = (String, String, bool);

/// Split the `fn info` body of a provisioning module into server and product fields.
fn module_fields(body: &str) -> (Vec<Field>, Vec<Field>) {
    let (mut server, mut product) = (vec![], vec![]);
    let mut unlabeled = vec![];
    let mut i = 0;
    while let Some(at) = body[i..].find("vec![") {
        let start = i + at + 5;
        let mut depth = 1;
        let mut end = start;
        for (j, ch) in body[start..].char_indices() {
            match ch {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        let before = &body[i + at.saturating_sub(0)..i + at];
        let label = body[..i + at].trim_end();
        let inner = &body[start..end];
        if label.ends_with("server_fields:") {
            server = fields(inner);
        } else if label.ends_with("product_fields:") {
            product = fields(inner);
        } else if !label.ends_with("actions:") {
            unlabeled.push(fields(inner));
        }
        let _ = before;
        i = end.max(start);
    }
    // Tuple form: `let (name, server_fields, product_fields) = ("…", vec![…], vec![…])`.
    if server.is_empty()
        && product.is_empty()
        && body.contains("server_fields, product_fields")
        && unlabeled.len() >= 2
    {
        server = unlabeled[0].clone();
        product = unlabeled[1].clone();
    }
    (server, product)
}

pub fn all() -> Vec<Addon> {
    let root = repo();
    let mut dirs = vec![];
    for base in ["extensions/addons", "extensions/examples"] {
        for e in std::fs::read_dir(root.join(base))
            .into_iter()
            .flatten()
            .flatten()
        {
            if e.path().join("addon.json").is_file() {
                dirs.push(e.path());
            }
        }
    }
    dirs.sort();
    dirs.into_iter()
        .map(|d| {
            let rel = d
                .strip_prefix(root.join("extensions"))
                .unwrap()
                .display()
                .to_string();
            let manifest: Value =
                serde_json::from_str(&std::fs::read_to_string(d.join("addon.json")).unwrap())
                    .unwrap_or_else(|e| panic!("{rel}/addon.json: {e}"));
            let src = sources(&d);
            let mut settings: Vec<(String, String, bool)> = manifest["settings"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| {
                    (
                        s["key"].as_str().unwrap_or("").to_string(),
                        s["kind"].as_str().unwrap_or("text").to_string(),
                        s["required"] == true,
                    )
                })
                .collect();
            for body in [
                fn_body(&src, "fn settings("),
                src.find("const SETTINGS")
                    .map(|a| &src[a..a + src[a..].find("];").unwrap_or(0)]),
            ]
            .into_iter()
            .flatten()
            {
                for f in fields(body) {
                    if !settings.iter().any(|s| s.0 == f.0) {
                        settings.push(f);
                    }
                }
            }
            let (server_fields, product_fields) = fn_body(&src, "fn info(&self) -> ModuleInfo")
                .map(module_fields)
                .unwrap_or_default();
            let actions = fn_body(&src, "fn custom_actions(")
                .map(|b| {
                    regex::Regex::new(r#""([a-z_]+)""#)
                        .unwrap()
                        .captures_iter(b)
                        .map(|c| c[1].to_string())
                        .collect()
                })
                .unwrap_or_default();
            let dir_name = d.file_name().unwrap().to_string_lossy().to_string();
            Addon {
                slug: format!("addon-{}", slugify(&dir_name)),
                fixture: dir_name.starts_with("fixture-") || dir_name.starts_with("tests-"),
                dir: rel,
                manifest,
                settings,
                server_fields,
                product_fields,
                actions,
            }
        })
        .collect()
}

fn code(s: &str) -> String {
    format!("`{s}`")
}

fn list(v: &Value) -> String {
    match v.as_array() {
        Some(a) if !a.is_empty() => a
            .iter()
            .map(|x| {
                code(
                    &x.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| x.to_string()),
                )
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => "none".into(),
    }
}

fn kind_title(k: &str) -> &str {
    match k {
        "gateway" => "Payment gateways",
        "provisioning" => "Provisioning modules",
        "registrar" => "Domain registrars",
        "captcha" => "Captcha providers",
        "sso" => "Sign-in (SSO) providers",
        "integration" => "Integrations",
        _ => "Other",
    }
}

fn field_rows(f: &[(String, String, bool)]) -> Vec<Vec<String>> {
    f.iter()
        .map(|(k, t, r)| {
            vec![
                code(k),
                code(t),
                if *r { "yes".into() } else { "no".into() },
            ]
        })
        .collect()
}

fn page(a: &Addon, tr: &Value) -> Page {
    let m = &a.manifest;
    let name = m["name"].as_str().unwrap_or(a.id());
    let mut p = Page::new(&a.slug, name, SECTION);
    let desc_key = m["description"].as_str().unwrap_or("");
    let desc = tr[desc_key].as_str().unwrap_or(desc_key);
    p.p(format!("{desc}{}", if a.fixture { " This is a **test fixture** used by the integration tests; it is not offered in the marketplace." } else { "" }));
    p.table(
        &["Field", "Value"],
        vec![
            vec!["Addon id".into(), code(a.id())],
            vec!["Kind".into(), code(a.kind())],
            vec![
                "Provides".into(),
                m["provides"]
                    .as_str()
                    .map(code)
                    .unwrap_or_else(|| "-".into()),
            ],
            vec!["Engine".into(), code(m["engine"].as_str().unwrap_or("-"))],
            vec![
                "ABI".into(),
                m["abi"].as_str().map(code).unwrap_or_else(|| "-".into()),
            ],
            vec![
                "Version".into(),
                m["version"].as_str().unwrap_or("-").into(),
            ],
            vec!["Author".into(), m["author"].as_str().unwrap_or("-").into()],
            vec!["Source".into(), code(&format!("extensions/{}", a.dir))],
            vec![
                "Permissions (shown at install)".into(),
                list(&m["permissions"]),
            ],
        ],
    );
    let caps = &m["capabilities"];
    p.h(2, "Capabilities");
    match caps.as_object() {
        Some(c) if !c.is_empty() => {
            p.table(
                &["Capability", "Value"],
                c.iter()
                    .map(|(k, v)| {
                        vec![
                            format!("[`{k}`](capabilities.md)"),
                            match v {
                                Value::Array(_)
                                    if v.as_array().unwrap().iter().all(|x| x.is_string()) =>
                                {
                                    list(v)
                                }
                                Value::Bool(b) => b.to_string(),
                                _ => code(&v.to_string()),
                            },
                        ]
                    })
                    .collect(),
            );
        }
        _ => {
            p.p("None. The addon only uses what its kind gives it.");
        }
    }
    p.h(2, "Events and hooks");
    p.table(
        &["What", "Value"],
        vec![
            vec!["Listens to (`events`)".into(), list(&caps["events"])],
            vec![
                "Hooks".into(),
                caps["hooks"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .map(|a| {
                        a.iter()
                            .map(|h| code(h["event"].as_str().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "none".into()),
            ],
            vec!["Emits".into(), list(&caps["emits"])],
            vec![
                "Mixins".into(),
                caps["mixins"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .map(|a| {
                        a.iter()
                            .map(|x| {
                                format!(
                                    "`{}` ({})",
                                    x["target"].as_str().unwrap_or(""),
                                    x["at"].as_str().unwrap_or("")
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "none".into()),
            ],
            vec![
                "Filters".into(),
                caps["filters"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .map(|a| {
                        a.iter()
                            .map(|x| code(x["name"].as_str().unwrap_or("")))
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "none".into()),
            ],
            vec![
                "Scheduled jobs".into(),
                caps["jobs"]
                    .as_array()
                    .filter(|a| !a.is_empty())
                    .map(|a| {
                        a.iter()
                            .map(|j| {
                                format!(
                                    "`{}` every {} s",
                                    j["op"].as_str().unwrap_or(""),
                                    j["every_secs"]
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_else(|| "none".into()),
            ],
        ],
    );
    p.h(2, "Settings and secrets");
    if a.settings.is_empty() {
        p.p("No admin settings are declared in `addon.json` or in the source.");
    } else {
        p.table(&["Setting", "Type", "Required"], field_rows(&a.settings));
    }
    p.p(format!(
        "Secrets (stored encrypted, never returned by the API): {}.",
        list(&caps["secrets"])
    ));
    p.p(format!("Outbound HTTP allowed to: {}. `$server` is the hostname of the server configured for the product, `$setting:<key>` the host of that setting.", list(&caps["http"])));
    if !a.server_fields.is_empty() || !a.product_fields.is_empty() || !a.actions.is_empty() {
        p.h(2, "Module fields");
        if !a.server_fields.is_empty() {
            p.p("Server fields (entered once per server under Admin → Servers):");
            p.table(&["Field", "Type", "Required"], field_rows(&a.server_fields));
        }
        if !a.product_fields.is_empty() {
            p.p("Product fields (entered per product, module settings tab):");
            p.table(
                &["Field", "Type", "Required"],
                field_rows(&a.product_fields),
            );
        }
        if !a.actions.is_empty() {
            p.p(format!(
                "Extra service actions: {}.",
                a.actions
                    .iter()
                    .map(|x| code(x))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if let Some(r) = caps["routes"].as_array().filter(|a| !a.is_empty()) {
        p.h(2, "HTTP routes");
        p.p(format!("Mounted under `/api/v1/addons/{}/`.", a.id()));
        p.table(
            &["Method", "Path", "Access", "Permission"],
            r.iter()
                .map(|x| {
                    vec![
                        x["method"].as_str().unwrap_or("").into(),
                        code(x["path"].as_str().unwrap_or("")),
                        x["access"].as_str().unwrap_or("").into(),
                        x["permission"]
                            .as_str()
                            .map(code)
                            .unwrap_or_else(|| "-".into()),
                    ]
                })
                .collect(),
        );
    }
    if m["ui"].is_object() {
        p.h(2, "UI");
        let pages: Vec<String> = m["ui"]["pages"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| {
                format!(
                    "`{}` ({})",
                    x["id"].as_str().unwrap_or(""),
                    x["area"].as_str().unwrap_or("")
                )
            })
            .collect();
        let slots: Vec<String> = m["ui"]["slots"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|x| code(x["slot"].as_str().unwrap_or("")))
            .collect();
        p.p(format!(
            "Pages: {}. Slots: {}. See [Widgets and slots](widgets.md).",
            if pages.is_empty() {
                "none".into()
            } else {
                pages.join(", ")
            },
            if slots.is_empty() {
                "none".into()
            } else {
                slots.join(", ")
            }
        ));
    }
    p.h(2, "addon.json");
    p.code(
        &format!("json title=extensions/{}/addon.json", a.dir),
        serde_json::to_string_pretty(m).unwrap(),
    );
    p
}

/// Catalog page plus one page per addon.
pub fn pages() -> Vec<Page> {
    let tr = locale();
    let all = all();
    let mut cat = Page::new("addons", "Bundled addons", SECTION);
    cat.p(format!("{} addons ship in `extensions/` ({} of them test fixtures). Each page below is generated from the addon's `addon.json` and the setting/field declarations in its source. Build them with `cargo xtask addons`; install them under Admin → Extensions.", all.len(), all.iter().filter(|a| a.fixture).count()));
    let mut kinds: Vec<&str> = all
        .iter()
        .filter(|a| !a.fixture)
        .map(|a| a.kind())
        .collect();
    kinds.sort();
    kinds.dedup();
    for k in kinds {
        cat.h(2, kind_title(k));
        cat.table(
            &["Addon", "Id", "Description"],
            all.iter()
                .filter(|a| a.kind() == k && !a.fixture)
                .map(|a| {
                    let d = a.manifest["description"].as_str().unwrap_or("");
                    vec![
                        format!(
                            "[{}]({}.md)",
                            a.manifest["name"].as_str().unwrap_or(a.id()),
                            a.slug
                        ),
                        code(a.id()),
                        tr[d].as_str().unwrap_or(d).to_string(),
                    ]
                })
                .collect(),
        );
    }
    cat.h(2, "Test fixtures");
    cat.table(
        &["Addon", "Id", "Kind"],
        all.iter()
            .filter(|a| a.fixture)
            .map(|a| {
                vec![
                    format!(
                        "[{}]({}.md)",
                        a.manifest["name"].as_str().unwrap_or(a.id()),
                        a.slug
                    ),
                    code(a.id()),
                    code(a.kind()),
                ]
            })
            .collect(),
    );
    let mut out = vec![cat];
    out.extend(all.iter().map(|a| page(a, &tr)));
    out
}
