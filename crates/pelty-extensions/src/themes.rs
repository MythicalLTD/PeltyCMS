//! Themes: `extensions/themes/<id>/theme.json` with whitelisted design tokens and
//! optional static assets. No JS. Token values are validated against a strict
//! character set so they cannot break out of a CSS declaration.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// Allowed token keys → CSS variable `--pelty-<key>`.
pub const TOKEN_KEYS: &[&str] = &[
    "background",
    "foreground",
    "surface",
    "surface-foreground",
    "muted",
    "muted-foreground",
    "border",
    "input",
    "ring",
    "primary",
    "primary-foreground",
    "accent",
    "accent-foreground",
    "success",
    "warning",
    "danger",
    "sidebar",
    "sidebar-foreground",
    "radius",
    "font-sans",
    "font-mono",
    "spacing",
];

const ASSET_EXTS: &[&str] = &["css", "png", "svg", "jpg", "jpeg", "webp", "woff2", "ico"];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ThemeError {
    #[error("invalid manifest: {0}")]
    Invalid(String),
    #[error("io: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_scheme")]
    pub color_scheme: String,
    pub tokens: BTreeMap<String, String>,
    /// Optional asset file names inside the theme dir.
    #[serde(default)]
    pub logo: Option<String>,
    #[serde(default)]
    pub stylesheet: Option<String>,
    /// Optional full frontend bundle: a directory inside the theme dir that
    /// contains `index.html` (e.g. `"dist"`). When the theme is active the
    /// server serves this bundle instead of the default frontend.
    #[serde(default)]
    pub frontend: Option<String>,
}
fn default_scheme() -> String {
    "dark".into()
}

/// Public, sanitized view.
#[derive(Debug, Clone, Serialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub color_scheme: String,
    pub tokens: BTreeMap<String, String>,
    pub logo_url: Option<String>,
    pub stylesheet_url: Option<String>,
    pub has_frontend: bool,
}

/// Conservative CSS value validator: colors, lengths, font stacks, functions
/// like `oklch(...)`, `rgb(...)`. Rejects `;{}<>\\` `url(` `expression` `@` etc.
pub fn valid_css_value(v: &str) -> bool {
    if v.is_empty() || v.len() > 200 {
        return false;
    }
    let lower = v.to_ascii_lowercase();
    if ["url(", "expression", "javascript", "@import", "var(--"]
        .iter()
        .any(|b| lower.contains(b))
    {
        return false;
    }
    let mut depth = 0i32;
    for c in v.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            c if c.is_ascii_alphanumeric() => {}
            ' ' | '#' | '.' | ',' | '%' | '-' | '/' | '"' | '\'' | '_' | '+' => {}
            _ => return false,
        }
    }
    depth == 0
        && v.matches('"').count().is_multiple_of(2)
        && v.matches('\'').count().is_multiple_of(2)
}

fn valid_asset_name(n: &str) -> bool {
    let p = Path::new(n);
    p.components().all(|c| matches!(c, Component::Normal(_)))
        && !n.starts_with('.')
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c))
        && p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| ASSET_EXTS.contains(&e))
}

impl ThemeManifest {
    pub fn parse(raw: &str) -> Result<Self, ThemeError> {
        let m: ThemeManifest =
            serde_json::from_str(raw).map_err(|e| ThemeError::Invalid(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), ThemeError> {
        let bad = |s: &str| Err(ThemeError::Invalid(s.to_string()));
        if !crate::valid_id(&self.id) {
            return bad("id");
        }
        if self.name.trim().is_empty()
            || self.name.len() > 80
            || self.version.len() > 32
            || self.author.len() > 80
        {
            return bad("name/version/author");
        }
        if self.color_scheme != "dark" && self.color_scheme != "light" {
            return bad("color_scheme");
        }
        for (k, v) in &self.tokens {
            if !TOKEN_KEYS.contains(&k.as_str()) {
                return Err(ThemeError::Invalid(format!("unknown token {k}")));
            }
            if !valid_css_value(v) {
                return Err(ThemeError::Invalid(format!("unsafe value for {k}")));
            }
        }
        for a in [&self.logo, &self.stylesheet].into_iter().flatten() {
            if !valid_asset_name(a) {
                return bad("asset path");
            }
        }
        if let Some(f) = &self.frontend {
            let p = Path::new(f);
            if f.is_empty()
                || f.len() > 64
                || !p.components().all(|c| matches!(c, Component::Normal(_)))
            {
                return bad("frontend path");
            }
        }
        Ok(())
    }

    pub fn public(&self) -> Theme {
        let url = |f: &Option<String>| {
            f.as_ref()
                .map(|f| format!("/api/v1/themes/{}/assets/{}", self.id, f))
        };
        Theme {
            id: self.id.clone(),
            name: self.name.clone(),
            version: self.version.clone(),
            author: self.author.clone(),
            description: self.description.clone(),
            color_scheme: self.color_scheme.clone(),
            tokens: self.tokens.clone(),
            logo_url: url(&self.logo),
            stylesheet_url: url(&self.stylesheet),
            has_frontend: self.frontend.is_some(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ThemeStore {
    dir: PathBuf,
    themes: BTreeMap<String, ThemeManifest>,
}

impl ThemeStore {
    /// Load all valid themes; invalid ones are logged and skipped.
    pub fn load(dir: &Path) -> Self {
        let mut themes = BTreeMap::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let mf = e.path().join("theme.json");
                let Ok(raw) = std::fs::read_to_string(&mf) else {
                    continue;
                };
                match ThemeManifest::parse(&raw) {
                    Ok(m) if e.file_name().to_str() == Some(m.id.as_str()) => {
                        themes.insert(m.id.clone(), m);
                    }
                    Ok(_) => {
                        tracing::warn!(path = %mf.display(), "theme id must match directory name")
                    }
                    Err(err) => tracing::warn!(path = %mf.display(), %err, "invalid theme skipped"),
                }
            }
        }
        ThemeStore {
            dir: dir.to_path_buf(),
            themes,
        }
    }

    pub fn list(&self) -> Vec<Theme> {
        self.themes.values().map(ThemeManifest::public).collect()
    }

    pub fn get(&self, id: &str) -> Option<Theme> {
        self.themes.get(id).map(ThemeManifest::public)
    }

    /// Resolve an asset path safely inside the theme directory.
    pub fn asset_path(&self, id: &str, file: &str) -> Option<PathBuf> {
        if !self.themes.contains_key(id) || !valid_asset_name(file) {
            return None;
        }
        let base = self.dir.join(id).canonicalize().ok()?;
        let p = base.join(file).canonicalize().ok()?;
        p.starts_with(&base).then_some(p)
    }

    /// Canonical root of a theme's full frontend bundle, if it declares one
    /// and it exists with an `index.html` inside the theme directory.
    pub fn frontend_root(&self, id: &str) -> Option<PathBuf> {
        let rel = self.themes.get(id)?.frontend.as_ref()?;
        let base = self.dir.join(id).canonicalize().ok()?;
        let p = base.join(rel).canonicalize().ok()?;
        (p.starts_with(&base) && p.join("index.html").is_file()).then_some(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const OK: &str = r##"{"id":"neo","name":"Neo","version":"1.0.0","tokens":{"primary":"oklch(0.7 0.15 250)","radius":"0.5rem","font-sans":"\"Inter\", sans-serif","background":"#0b0d10"}}"##;

    #[test]
    fn accepts_valid_manifest() {
        let m = ThemeManifest::parse(OK).unwrap();
        assert_eq!(m.public().tokens["radius"], "0.5rem");
    }

    #[test]
    fn rejects_css_injection() {
        for v in [
            "red; } body { display:none",
            "url(https://evil)",
            "red</style><script>",
            "expression(alert(1))",
            "a{b}",
            "\\61",
            "rgb(1,2,3",
        ] {
            assert!(!valid_css_value(v), "should reject {v}");
        }
        let bad = OK.replace("0.5rem", "0.5rem;}*{x:y");
        assert!(ThemeManifest::parse(&bad).is_err());
    }

    #[test]
    fn rejects_unknown_tokens_and_fields() {
        assert!(ThemeManifest::parse(&OK.replace("\"radius\"", "\"--evil\"")).is_err());
        assert!(ThemeManifest::parse(&OK.replace("\"id\":\"neo\"", "\"id\":\"../neo\"")).is_err());
        assert!(ThemeManifest::parse(&OK.replace("}}", "},\"script\":\"x.js\"}")).is_err());
    }

    #[test]
    fn frontend_path_must_be_relative() {
        let with =
            |f: &str| OK.replacen("\"tokens\"", &format!("\"frontend\":{f:?},\"tokens\""), 1);
        assert!(ThemeManifest::parse(&with("dist")).is_ok());
        assert!(ThemeManifest::parse(&with("../x")).is_err());
        assert!(ThemeManifest::parse(&with("/etc")).is_err());
    }

    #[test]
    fn rejects_asset_traversal() {
        for a in [
            "../../etc/passwd.css",
            "/etc/x.css",
            "a/../../b.css",
            ".hidden.css",
            "x.js",
            "x.html",
        ] {
            assert!(!valid_asset_name(a), "should reject {a}");
        }
        assert!(valid_asset_name("assets/logo.svg"));
        let s = ThemeStore::load(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../extensions/themes"));
        assert!(s.get("default").is_some());
        assert!(s.asset_path("default", "../midnight/theme.json").is_none());
        assert!(s.asset_path("default", "../../Cargo.toml").is_none());
        assert!(s.asset_path("nope", "logo.svg").is_none());
        assert!(s.asset_path("default", "logo.svg").is_some());
    }
}
