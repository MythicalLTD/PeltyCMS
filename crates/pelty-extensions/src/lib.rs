//! Themes and plugins: manifest loading/validation, hook bus, registry.
pub mod addon_pkg;
pub mod addon_signing;
pub mod events;
pub mod filters;
pub mod functions;
pub mod plugins;
pub mod themes;
pub mod ui_schema;

/// Shared id rule for themes/plugins: `^[a-z][a-z0-9_-]{1,63}$`.
pub fn valid_id(id: &str) -> bool {
    let mut c = id.chars();
    id.len() >= 2
        && id.len() <= 64
        && c.next().is_some_and(|f| f.is_ascii_lowercase())
        && c.all(|x| x.is_ascii_lowercase() || x.is_ascii_digit() || x == '-' || x == '_')
}
