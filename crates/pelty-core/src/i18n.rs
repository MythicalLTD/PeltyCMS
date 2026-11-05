//! Translations from flat JSON locale files (`locales/<lang>.json`).
//! `en` is the base; missing keys fall back to `en`, then to the key itself.
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, RwLock};

type Overrides = HashMap<String, HashMap<String, &'static str>>;

#[derive(Debug, Clone, Default)]
pub struct I18n {
    locales: HashMap<String, BTreeMap<String, String>>,
    default: String,
    /// Admin edits from the translation editor (DB-backed), checked before
    /// the shipped files. Values are leaked once per distinct text so `t`
    /// can keep returning `&str`; edits are staff-only and length-capped,
    /// and unchanged values are reused on reload.
    overrides: Arc<RwLock<Overrides>>,
    /// Languages registered from the DB without a `locales/<code>.json` file
    /// (code → display name). Empty message maps; keys fall back to `en`.
    extra: Arc<RwLock<BTreeMap<String, String>>>,
}

fn empty_messages() -> &'static BTreeMap<String, String> {
    static EMPTY: std::sync::OnceLock<BTreeMap<String, String>> = std::sync::OnceLock::new();
    EMPTY.get_or_init(BTreeMap::new)
}

pub const BASE: &str = "en";

fn valid_lang(l: &str) -> bool {
    !l.is_empty() && l.len() <= 10 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl I18n {
    /// Load every `*.json` in `dir`. Fails if `en.json` is missing or invalid.
    pub fn load(dir: &Path, default: &str) -> Result<Self, String> {
        let mut locales = HashMap::new();
        let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let lang = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !valid_lang(&lang) {
                continue;
            }
            let raw = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let map: BTreeMap<String, String> =
                serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", p.display()))?;
            locales.insert(lang, map);
        }
        if !locales.contains_key(BASE) {
            return Err("locales/en.json is required".into());
        }
        let default = if locales.contains_key(default) {
            default
        } else {
            BASE
        }
        .to_string();
        Ok(I18n {
            locales,
            default,
            overrides: Arc::default(),
            extra: Arc::default(),
        })
    }

    /// Merge an extension's locale files (`<dir>/<lang>.json`, flat keys
    /// without prefix) under `prefix` (e.g. `plugin.billing-extra.`). Only
    /// languages the core already ships are merged; keys outside the prefix
    /// can never override core strings. Returns number of keys merged.
    pub fn merge_namespace(&mut self, prefix: &str, dir: &Path) -> Result<usize, String> {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return Ok(0);
        };
        let mut n = 0;
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let lang = p
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let Some(target) = self.locales.get_mut(&lang) else {
                continue;
            };
            let raw = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let map: BTreeMap<String, String> =
                serde_json::from_str(&raw).map_err(|e| format!("{}: {e}", p.display()))?;
            for (k, v) in map {
                target.insert(format!("{prefix}{k}"), v);
                n += 1;
            }
        }
        Ok(n)
    }

    pub fn from_maps(locales: HashMap<String, BTreeMap<String, String>>) -> Self {
        I18n {
            locales,
            default: BASE.into(),
            overrides: Arc::default(),
            extra: Arc::default(),
        }
    }

    pub fn languages(&self) -> Vec<String> {
        let mut v: Vec<_> = self.locales.keys().cloned().collect();
        if let Ok(extra) = self.extra.read() {
            for k in extra.keys() {
                if !v.iter().any(|x| x == k) {
                    v.push(k.clone());
                }
            }
        }
        v.sort();
        v
    }

    pub fn has(&self, lang: &str) -> bool {
        self.has_file(lang)
            || self
                .extra
                .read()
                .map(|e| e.contains_key(lang))
                .unwrap_or(false)
    }

    /// True when a `locales/<lang>.json` was loaded at startup.
    pub fn has_file(&self, lang: &str) -> bool {
        self.locales.contains_key(lang)
    }

    pub fn messages(&self, lang: &str) -> Option<&BTreeMap<String, String>> {
        if let Some(m) = self.locales.get(lang) {
            return Some(m);
        }
        let ok = self
            .extra
            .read()
            .map(|e| e.contains_key(lang))
            .unwrap_or(false);
        if ok {
            Some(empty_messages())
        } else {
            None
        }
    }

    /// Display name for a language: DB-registered name, else `language.name`.
    pub fn lang_name(&self, code: &str) -> String {
        if let Ok(extra) = self.extra.read() {
            if let Some(n) = extra.get(code) {
                return n.clone();
            }
        }
        self.t(code, "language.name").to_string()
    }

    /// Replace the set of DB-only languages (code → name). File locales are unchanged.
    pub fn set_extra_langs(&self, rows: impl IntoIterator<Item = (String, String)>) {
        let mut w = self.extra.write().unwrap_or_else(|e| e.into_inner());
        w.clear();
        for (code, name) in rows {
            if valid_lang(&code) && !self.locales.contains_key(&code) {
                w.insert(code, name);
            }
        }
    }

    /// Register one DB language immediately (no-op if a locale file exists).
    pub fn ensure_lang(&self, code: &str, name: &str) {
        if !valid_lang(code) || self.locales.contains_key(code) {
            return;
        }
        if let Ok(mut w) = self.extra.write() {
            w.insert(code.to_string(), name.to_string());
        }
    }

    pub fn drop_lang(&self, code: &str) {
        if let Ok(mut w) = self.extra.write() {
            w.remove(code);
        }
    }

    /// Replace all overrides (lang → key → text).
    pub fn set_overrides(&self, rows: impl IntoIterator<Item = (String, String, String)>) {
        let mut w = self.overrides.write().unwrap_or_else(|e| e.into_inner());
        let mut next: Overrides = HashMap::new();
        for (lang, key, text) in rows {
            let prev = w
                .get(&lang)
                .and_then(|m| m.get(&key))
                .copied()
                .filter(|p| *p == text);
            let v: &'static str = prev.unwrap_or_else(|| Box::leak(text.into_boxed_str()));
            next.entry(lang).or_default().insert(key, v);
        }
        *w = next;
    }

    /// Overrides for one language (for `/i18n/{lang}`).
    pub fn overrides(&self, lang: &str) -> Vec<(String, String)> {
        let r = self.overrides.read().unwrap_or_else(|e| e.into_inner());
        r.get(lang)
            .map(|m| {
                m.iter()
                    .map(|(k, v)| (k.clone(), (*v).to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn override_for(&self, lang: &str, key: &str) -> Option<&'static str> {
        let r = self.overrides.read().unwrap_or_else(|e| e.into_inner());
        r.get(lang).and_then(|m| m.get(key)).copied()
    }

    pub fn t<'a>(&'a self, lang: &str, key: &'a str) -> &'a str {
        if let Some(v) = self.override_for(lang, key) {
            return v;
        }
        if self.locales.get(lang).and_then(|m| m.get(key)).is_none() {
            if let Some(v) = self.override_for(BASE, key) {
                return v;
            }
        }
        self.locales
            .get(lang)
            .and_then(|m| m.get(key))
            .or_else(|| self.locales.get(BASE).and_then(|m| m.get(key)))
            .map(String::as_str)
            .unwrap_or(key)
    }

    /// Pick the best locale from an Accept-Language header.
    pub fn negotiate(&self, accept_language: Option<&str>) -> String {
        if let Some(h) = accept_language {
            let mut prefs: Vec<(f32, String)> = h
                .split(',')
                .filter_map(|part| {
                    let mut it = part.trim().split(';');
                    let tag = it.next()?.trim().to_ascii_lowercase();
                    let q = it
                        .find_map(|p| p.trim().strip_prefix("q=").and_then(|q| q.parse().ok()))
                        .unwrap_or(1.0);
                    Some((q, tag))
                })
                .collect();
            prefs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            for (_, tag) in prefs {
                if self.has(&tag) {
                    return tag;
                }
                if let Some(primary) = tag.split('-').next() {
                    if self.has(primary) {
                        return primary.to_string();
                    }
                }
            }
        }
        self.default.clone()
    }

    /// Keys present in `en` but missing in `lang`.
    pub fn missing_keys(&self, lang: &str) -> Vec<String> {
        let (Some(base), Some(other)) = (self.locales.get(BASE), self.locales.get(lang)) else {
            return vec![];
        };
        base.keys()
            .filter(|k| !other.contains_key(*k))
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extra_lang_registers_without_file() {
        let i = sample();
        assert!(!i.has("de"));
        i.ensure_lang("de", "Deutsch");
        assert!(i.has("de"));
        assert!(!i.has_file("de"));
        assert_eq!(i.lang_name("de"), "Deutsch");
        assert_eq!(i.t("de", "a"), "A"); // falls back to en
        i.drop_lang("de");
        assert!(!i.has("de"));
    }

    #[test]
    fn overrides_win_and_fall_back() {
        let i = sample();
        let before = i.t("ro", "a").to_string();
        i.set_overrides([("ro".to_string(), "a".to_string(), "Salut!".to_string())]);
        assert_eq!(i.t("ro", "a"), "Salut!");
        assert_ne!(i.t("en", "a"), "Salut!");
        assert_eq!(
            i.overrides("ro"),
            vec![("a".to_string(), "Salut!".to_string())]
        );
        let p1 = i.t("ro", "a").as_ptr();
        i.set_overrides([("ro".to_string(), "a".to_string(), "Salut!".to_string())]);
        assert_eq!(
            i.t("ro", "a").as_ptr(),
            p1,
            "unchanged value is reused, not re-leaked"
        );
        // An English override reaches languages that lack the key.
        i.set_overrides([("en".to_string(), "b".to_string(), "Bee".to_string())]);
        assert_eq!(i.t("ro", "b"), "Bee");
        assert_eq!(i.t("ro", "a"), before);
        i.set_overrides(Vec::new());
        assert_eq!(i.t("ro", "a"), before);
    }

    fn sample() -> I18n {
        let mut m = HashMap::new();
        m.insert(
            "en".into(),
            BTreeMap::from([("a".into(), "A".into()), ("b".into(), "B".into())]),
        );
        m.insert("ro".into(), BTreeMap::from([("a".into(), "Ă".into())]));
        I18n::from_maps(m)
    }
    #[test]
    fn fallback() {
        let i = sample();
        assert_eq!(i.t("ro", "a"), "Ă");
        assert_eq!(i.t("ro", "b"), "B");
        assert_eq!(i.t("xx", "zz"), "zz");
        assert_eq!(i.missing_keys("ro"), vec!["b".to_string()]);
    }
    #[test]
    fn negotiation() {
        let i = sample();
        assert_eq!(i.negotiate(Some("ro-RO,ro;q=0.9,en;q=0.8")), "ro");
        assert_eq!(i.negotiate(Some("de, en;q=0.5")), "en");
        assert_eq!(i.negotiate(Some("de")), "en");
        assert_eq!(i.negotiate(None), "en");
    }

    #[test]
    fn merges_extension_namespace_without_overriding_core() {
        let dir = std::env::temp_dir().join(format!("pelty-i18n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("en.json"),
            r#"{"menu":"Logs","error.not_found":"hijack"}"#,
        )
        .unwrap();
        std::fs::write(dir.join("xx.json"), r#"{"menu":"??"}"#).unwrap();
        let mut maps = HashMap::new();
        maps.insert(
            "en".to_string(),
            BTreeMap::from([("error.not_found".to_string(), "Not found".to_string())]),
        );
        let mut i = I18n::from_maps(maps);
        assert_eq!(i.merge_namespace("plugin.logs.", &dir).unwrap(), 2);
        assert_eq!(i.t("en", "plugin.logs.menu"), "Logs");
        assert_eq!(i.t("en", "error.not_found"), "Not found");
        assert!(!i.has("xx"));
    }
}
