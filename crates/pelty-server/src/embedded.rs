//! Files compiled into the binary by the `embed-frontend` feature (see
//! build.rs): the web UI and the default themes, plus the addon zips only
//! with the opt-in `embed-addons` feature (off by default and in releases).
//! The UI is served straight from memory; themes and addon zips are written
//! to disk because the server (and the admin, for themes) works with folders.
use std::path::{Path, PathBuf};

#[cfg(pelty_embed_frontend)]
static FRONTEND: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../frontend/dist");
#[cfg(pelty_embed_addons)]
static ADDONS: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../extensions/addons/dist");
#[cfg(feature = "embed-frontend")]
static THEMES: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/../../extensions/themes");

/// File of the compiled-in web UI (path relative to the build root).
#[allow(unused_variables)]
pub fn frontend_file(rel: &str) -> Option<&'static [u8]> {
    #[cfg(pelty_embed_frontend)]
    return FRONTEND.get_file(rel).map(|f| f.contents());
    #[cfg(not(pelty_embed_frontend))]
    None
}

/// Whether this binary carries a web UI build.
pub fn has_frontend() -> bool {
    frontend_file("index.html").is_some()
}

/// Make the HTTP layer serve the compiled-in UI when no build is on disk.
pub fn install() {
    if has_frontend() {
        pelty_api::static_files::set_embedded(frontend_file);
    }
}

#[cfg(feature = "embed-frontend")]
fn write_dir(d: &include_dir::Dir<'_>, to: &Path, overwrite: bool) -> std::io::Result<usize> {
    let mut n = 0;
    for f in d.files() {
        let p = to.join(f.path());
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if overwrite || !p.exists() {
            std::fs::write(&p, f.contents())?;
            n += 1;
        }
    }
    for sub in d.dirs() {
        n += write_dir(sub, to, overwrite)?;
    }
    Ok(n)
}

/// First run only: copy the default themes into an empty themes folder
/// (themes are user-editable, so existing folders are never touched).
#[allow(unused_variables, clippy::needless_return)]
pub fn seed_themes(dir: &Path) -> std::io::Result<Vec<String>> {
    #[cfg(feature = "embed-frontend")]
    {
        let has_any = std::fs::read_dir(dir)?
            .flatten()
            .any(|e| e.path().join("theme.json").is_file());
        if has_any {
            return Ok(vec![]);
        }
        let mut made = vec![];
        for t in THEMES.dirs() {
            if t.get_file(t.path().join("theme.json")).is_some() {
                write_dir(t, dir, false)?;
                made.push(t.path().display().to_string());
            }
        }
        return Ok(made);
    }
    #[cfg(not(feature = "embed-frontend"))]
    Ok(vec![])
}

/// Folder the bundled addon zips are unpacked to (next to the addon data).
#[cfg_attr(not(pelty_embed_addons), allow(dead_code))]
pub const ADDONS_DIST: &str = "data/bundled-addons";

/// Write the compiled-in addon zips to `<root>/data/bundled-addons` when they
/// are missing or from another binary version (an update ships new addon
/// versions; the server then upgrades installed addons from them). Returns
/// the folder, or None when this binary carries no addon zips.
// `return` is needed: the cfg'd-out branch follows.
#[allow(unused_variables, clippy::needless_return)]
pub fn unpack_addons(root: &Path) -> std::io::Result<Option<PathBuf>> {
    #[cfg(pelty_embed_addons)]
    {
        let dir = root.join(ADDONS_DIST);
        let stamp = dir.join(".version");
        let want = format!("{} {}", env!("CARGO_PKG_VERSION"), ADDONS.files().count());
        if std::fs::read_to_string(&stamp).ok().as_deref() != Some(want.as_str()) {
            std::fs::create_dir_all(&dir)?;
            write_dir(&ADDONS, &dir, true)?;
            std::fs::write(&stamp, want)?;
        }
        return Ok(Some(dir));
    }
    #[cfg(not(pelty_embed_addons))]
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "embed-frontend")]
    #[test]
    fn default_themes_seed_only_an_empty_folder() {
        let d =
            std::env::temp_dir().join(format!("pelty-themes-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&d).unwrap();
        let made = seed_themes(&d).unwrap();
        assert!(made.contains(&"default".to_string()), "{made:?}");
        assert!(d.join("default/theme.json").is_file());
        std::fs::write(d.join("default/theme.json"), "{}").unwrap();
        assert!(seed_themes(&d).unwrap().is_empty());
        assert_eq!(
            std::fs::read_to_string(d.join("default/theme.json")).unwrap(),
            "{}"
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(not(feature = "embed-addons"))]
    #[test]
    fn no_addons_in_the_binary_by_default() {
        let d =
            std::env::temp_dir().join(format!("pelty-noaddons-{}", uuid::Uuid::new_v4().simple()));
        assert!(unpack_addons(&d).unwrap().is_none());
        assert!(!d.join(ADDONS_DIST).exists());
    }

    #[cfg(pelty_embed_frontend)]
    #[test]
    fn ui_build_is_compiled_in() {
        assert!(has_frontend());
        let html = std::str::from_utf8(frontend_file("index.html").unwrap()).unwrap();
        let asset = html
            .split("/assets/")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("index.html links an asset");
        assert!(frontend_file(&format!("assets/{asset}")).is_some());
    }
}
