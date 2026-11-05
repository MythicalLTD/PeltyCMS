//! Addon packages (`.zip`): pack, inspect and safely extract.
//!
//! A package holds `addon.json` at its root (or under one top-level folder)
//! plus every file listed in the manifest `files` map with its SHA-256.
//! Inspection rejects path traversal (`..`, absolute paths, backslashes,
//! drive letters), symlinks, undeclared or mismatching files, and enforces
//! size / entry-count limits before anything touches the disk. Shared by the
//! admin upload endpoint, the registry installer and `pelty-cli`.
//! Optional `addon.sig` is reserved metadata, excluded from content hashes.
//! Inspection parses but does not authenticate it: callers must invoke
//! `addon_signing::verify` with operator trust before installation.
use crate::addon_signing::{ReleaseSignature, MAX_SIGNATURE_BYTES, SIGNATURE_FILE};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_zip_bytes: usize,
    pub max_unpacked_bytes: u64,
    pub max_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_zip_bytes: 10 * 1024 * 1024,
            max_unpacked_bytes: 50 * 1024 * 1024,
            max_entries: 1000,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PkgError {
    #[error("package too large")]
    TooLarge,
    #[error("not a valid zip archive")]
    BadZip,
    #[error("unsafe path in archive: {0}")]
    UnsafePath(String),
    #[error("addon.json missing or invalid")]
    BadManifest,
    #[error("integrity check failed for {0}")]
    Integrity(String),
    #[error("file not declared in manifest: {0}")]
    Undeclared(String),
    #[error("invalid addon signature: {0}")]
    Signature(String),
    #[error("io: {0}")]
    Io(String),
}

pub struct Package {
    pub manifest_raw: Vec<u8>,
    pub manifest: Value,
    pub id: String,
    /// Parsed metadata only; inspection does NOT establish signer trust.
    pub signature: Option<ReleaseSignature>,
    /// Relative path → contents (manifest and detached signature excluded).
    pub files: BTreeMap<String, Vec<u8>>,
}

pub fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

pub fn id_ok(s: &str) -> bool {
    (3..=64).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !s.starts_with('-')
}

/// A relative, forward-slash path with no traversal components.
pub fn safe_rel(p: &str) -> bool {
    !p.is_empty()
        && p.len() <= 255
        && !p.starts_with('/')
        && !p.contains('\\')
        && !p.contains('\0')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

pub fn inspect(bytes: &[u8], lim: Limits) -> Result<Package, PkgError> {
    if bytes.len() > lim.max_zip_bytes {
        return Err(PkgError::TooLarge);
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| PkgError::BadZip)?;
    if zip.len() > lim.max_entries {
        return Err(PkgError::TooLarge);
    }
    let mut raw: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|_| PkgError::BadZip)?;
        let name = f.name().map_err(|_| PkgError::BadZip)?.to_string();
        if f.is_dir() {
            let n = name.trim_end_matches('/');
            if !n.is_empty() && !safe_rel(n) {
                return Err(PkgError::UnsafePath(name));
            }
            continue;
        }
        if f.is_symlink() || f.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
            return Err(PkgError::UnsafePath(name));
        }
        if !safe_rel(&name) {
            return Err(PkgError::UnsafePath(name));
        }
        // Trust nothing the header says: count real decompressed bytes.
        let remaining = lim.max_unpacked_bytes.saturating_sub(total);
        let is_signature = name.rsplit('/').next() == Some(SIGNATURE_FILE);
        let read_limit = if is_signature {
            remaining.min(MAX_SIGNATURE_BYTES as u64)
        } else {
            remaining
        };
        let mut buf = Vec::new();
        (&mut f)
            .take(read_limit.saturating_add(1))
            .read_to_end(&mut buf)
            .map_err(|_| PkgError::BadZip)?;
        if is_signature && buf.len() > MAX_SIGNATURE_BYTES {
            return Err(PkgError::Signature("addon.sig exceeds size limit".into()));
        }
        total += buf.len() as u64;
        if total > lim.max_unpacked_bytes {
            return Err(PkgError::TooLarge);
        }
        if raw.insert(name.clone(), buf).is_some() {
            return Err(PkgError::UnsafePath(name));
        }
    }
    // Strip one common top-level folder if addon.json is not at the root.
    let prefix = if raw.contains_key("addon.json") {
        String::new()
    } else {
        let tops: std::collections::BTreeSet<&str> = raw
            .keys()
            .filter_map(|k| k.split_once('/').map(|(a, _)| a))
            .collect();
        let only_nested = raw.keys().all(|k| k.contains('/'));
        match (tops.len(), only_nested) {
            (1, true) => format!("{}/", tops.iter().next().copied().unwrap_or_default()),
            _ => return Err(PkgError::BadManifest),
        }
    };
    let mut files = BTreeMap::new();
    let mut manifest_raw = None;
    for (k, v) in raw {
        let rel = k
            .strip_prefix(&prefix)
            .ok_or(PkgError::BadManifest)?
            .to_string();
        if rel == "addon.json" {
            manifest_raw = Some(v);
        } else {
            files.insert(rel, v);
        }
    }
    let signature = files
        .remove(SIGNATURE_FILE)
        .map(|raw| ReleaseSignature::parse(&raw).map_err(|e| PkgError::Signature(e.to_string())))
        .transpose()?;
    let manifest_raw = manifest_raw.ok_or(PkgError::BadManifest)?;
    let manifest: Value =
        serde_json::from_slice(&manifest_raw).map_err(|_| PkgError::BadManifest)?;
    let id = manifest
        .get("id")
        .and_then(Value::as_str)
        .filter(|s| id_ok(s))
        .ok_or(PkgError::BadManifest)?
        .to_string();
    let declared = manifest
        .get("files")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for (rel, want) in &declared {
        if !safe_rel(rel) || rel == "addon.json" || rel == SIGNATURE_FILE {
            return Err(PkgError::UnsafePath(rel.clone()));
        }
        let got = files
            .get(rel)
            .ok_or_else(|| PkgError::Integrity(rel.clone()))?;
        if !want
            .as_str()
            .is_some_and(|w| w.eq_ignore_ascii_case(&sha256_hex(got)))
        {
            return Err(PkgError::Integrity(rel.clone()));
        }
    }
    if let Some(extra) = files.keys().find(|k| !declared.contains_key(k.as_str())) {
        return Err(PkgError::Undeclared(extra.clone()));
    }
    for m in manifest
        .get("migrations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if !m.as_str().is_some_and(|m| declared.contains_key(m)) {
            return Err(PkgError::Integrity(m.to_string()));
        }
    }
    Ok(Package {
        manifest_raw,
        manifest,
        id,
        signature,
        files,
    })
}

/// Publish into canonical `<root>/<id>` using staging and rename, restoring the
/// previous release on publication failure. Replacement has a brief rename gap.
/// Fails if the destination exists and `replace` is false, or this addon is busy.
/// Root and destination must not be symlinks; root and ancestors must be
/// operator-controlled (these path checks do not defeat hostile local writers).
/// A process crash may leave an extraction lock requiring operator cleanup.
pub fn extract(pkg: &Package, root: &Path, replace: bool) -> Result<PathBuf, PkgError> {
    extract_with_rename(pkg, root, replace, |from, to| std::fs::rename(from, to))
}

struct ExtractionDir {
    path: PathBuf,
    preserve: bool,
}

impl Drop for ExtractionDir {
    fn drop(&mut self) {
        if !self.preserve {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn private_dir(path: &Path) -> std::io::Result<()> {
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path)
}

fn extraction_workspace(root: &Path, id: &str) -> Result<ExtractionDir, PkgError> {
    use ring::rand::{SecureRandom, SystemRandom};
    for _ in 0..8 {
        let mut nonce = [0u8; 16];
        SystemRandom::new()
            .fill(&mut nonce)
            .map_err(|_| PkgError::Io("extraction random generator failed".into()))?;
        let nonce: String = nonce.iter().map(|b| format!("{b:02x}")).collect();
        let path = root.join(format!(".{id}.staging-{nonce}"));
        match private_dir(&path) {
            Ok(()) => {
                return Ok(ExtractionDir {
                    path,
                    preserve: false,
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(PkgError::Io(e.to_string())),
        }
    }
    Err(PkgError::Io(
        "could not create unique extraction workspace".into(),
    ))
}

fn destination_exists(dest: &Path) -> Result<bool, PkgError> {
    match std::fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            Err(PkgError::UnsafePath(dest.display().to_string()))
        }
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(PkgError::Io(e.to_string())),
    }
}

fn extract_with_rename(
    pkg: &Package,
    root: &Path,
    replace: bool,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<PathBuf, PkgError> {
    let io = |e: std::io::Error| PkgError::Io(e.to_string());
    if !id_ok(&pkg.id) {
        return Err(PkgError::BadManifest);
    }
    std::fs::create_dir_all(root).map_err(io)?;
    if std::fs::symlink_metadata(root)
        .map_err(io)?
        .file_type()
        .is_symlink()
    {
        return Err(PkgError::UnsafePath(root.display().to_string()));
    }
    let root = std::fs::canonicalize(root).map_err(io)?;
    let dest = root.join(&pkg.id);
    let lock_path = root.join(format!(".{}.extract-lock", pkg.id));
    private_dir(&lock_path).map_err(|e| {
        PkgError::Io(format!(
            "cannot acquire addon extraction lock {}: {e}",
            lock_path.display()
        ))
    })?;
    let _lock = ExtractionDir {
        path: lock_path,
        preserve: false,
    };
    if destination_exists(&dest)? && !replace {
        return Err(PkgError::Io("addon directory already exists".into()));
    }
    let mut workspace = extraction_workspace(&root, &pkg.id)?;
    let stage = workspace.path.join("new");
    private_dir(&stage).map_err(io)?;
    let write = || -> Result<(), PkgError> {
        std::fs::write(stage.join("addon.json"), &pkg.manifest_raw).map_err(io)?;
        if let Some(sig) = &pkg.signature {
            let raw =
                serde_json::to_vec_pretty(sig).map_err(|e| PkgError::Signature(e.to_string()))?;
            std::fs::write(stage.join(SIGNATURE_FILE), raw).map_err(io)?;
        }
        for (rel, b) in &pkg.files {
            if !safe_rel(rel) || rel == "addon.json" || rel == SIGNATURE_FILE {
                return Err(PkgError::UnsafePath(rel.clone()));
            }
            let p = stage.join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(&p, b).map_err(io)?;
        }
        Ok(())
    };
    write()?;
    let exists = destination_exists(&dest)?;
    if exists && !replace {
        return Err(PkgError::Io("addon directory already exists".into()));
    }
    if exists {
        let old = workspace.path.join("previous");
        rename(&dest, &old).map_err(io)?;
        if let Err(publication) = rename(&stage, &dest) {
            if let Err(rollback) = rename(&old, &dest) {
                // Do not let cleanup destroy the only surviving old release.
                workspace.preserve = true;
                return Err(PkgError::Io(format!(
                    "publication failed: {publication}; rollback failed: {rollback}; previous release preserved at {}",
                    old.display()
                )));
            }
            return Err(PkgError::Io(format!(
                "publication failed; previous release restored: {publication}"
            )));
        }
    } else {
        rename(&stage, &dest).map_err(io)?;
    }
    Ok(dest)
}

fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let ft = e.file_type()?;
        let p = e.path();
        if ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if ft.is_dir() {
            walk(&p, base, out)?;
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Recompute `files` hashes in `<dir>/addon.json` from the files on disk.
pub fn rehash(dir: &Path) -> Result<Value, PkgError> {
    let io = |e: std::io::Error| PkgError::Io(e.to_string());
    let raw = std::fs::read(dir.join("addon.json")).map_err(|_| PkgError::BadManifest)?;
    let mut m: Value = serde_json::from_slice(&raw).map_err(|_| PkgError::BadManifest)?;
    let mut list = vec![];
    walk(dir, dir, &mut list).map_err(io)?;
    list.sort();
    let mut files = serde_json::Map::new();
    for rel in list
        .into_iter()
        .filter(|r| r != "addon.json" && r != SIGNATURE_FILE)
    {
        let b = std::fs::read(dir.join(&rel)).map_err(io)?;
        files.insert(rel, json!(sha256_hex(&b)));
    }
    m.as_object_mut()
        .ok_or(PkgError::BadManifest)?
        .insert("files".into(), Value::Object(files));
    let pretty = serde_json::to_vec_pretty(&m).map_err(|_| PkgError::BadManifest)?;
    std::fs::write(dir.join("addon.json"), [pretty, b"\n".to_vec()].concat()).map_err(io)?;
    Ok(m)
}

/// Zip `<dir>` (manifest + declared files only, deterministic order).
pub fn pack(dir: &Path) -> Result<Vec<u8>, PkgError> {
    let io = |e: std::io::Error| PkgError::Io(e.to_string());
    let raw = std::fs::read(dir.join("addon.json")).map_err(|_| PkgError::BadManifest)?;
    let m: Value = serde_json::from_slice(&raw).map_err(|_| PkgError::BadManifest)?;
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let z = |e: zip::result::ZipError| PkgError::Io(e.to_string());
    w.start_file("addon.json", opt).map_err(z)?;
    w.write_all(&raw).map_err(io)?;
    let files = m
        .get("files")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for rel in files.keys() {
        if !safe_rel(rel) || rel == "addon.json" || rel == SIGNATURE_FILE {
            return Err(PkgError::UnsafePath(rel.clone()));
        }
        let b = std::fs::read(dir.join(rel)).map_err(io)?;
        w.start_file(rel.as_str(), opt).map_err(z)?;
        w.write_all(&b).map_err(io)?;
    }
    match std::fs::File::open(dir.join(SIGNATURE_FILE)) {
        Ok(file) => {
            let mut raw = Vec::new();
            file.take(MAX_SIGNATURE_BYTES as u64 + 1)
                .read_to_end(&mut raw)
                .map_err(io)?;
            ReleaseSignature::parse(&raw).map_err(|e| PkgError::Signature(e.to_string()))?;
            w.start_file(SIGNATURE_FILE, opt).map_err(z)?;
            w.write_all(&raw).map_err(io)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(io(e)),
    }
    let out = w.finish().map_err(z)?.into_inner();
    inspect(&out, Limits::default())?;
    Ok(out)
}

/// Create a minimal addon skeleton in `<parent>/<id>`.
pub fn scaffold(
    parent: &Path,
    id: &str,
    kind: &str,
    engine: &str,
    name: &str,
) -> Result<PathBuf, PkgError> {
    if !id_ok(id) {
        return Err(PkgError::BadManifest);
    }
    let io = |e: std::io::Error| PkgError::Io(e.to_string());
    let dir = parent.join(id);
    if dir.exists() {
        return Err(PkgError::Io(format!("{} already exists", dir.display())));
    }
    std::fs::create_dir_all(dir.join("migrations")).map_err(io)?;
    let table = format!("addon_{}_log", id.replace('-', "_"));
    std::fs::write(dir.join("migrations/0001_init.sql"), format!("-- Tables must be prefixed addon_{}_\nCREATE TABLE IF NOT EXISTS {table} (id BIGSERIAL PRIMARY KEY, created_at TIMESTAMPTZ NOT NULL DEFAULT now());\n", id.replace('-', "_"))).map_err(io)?;
    std::fs::write(dir.join("README.md"), format!("# {name}\n\nBuild with `pelty-cli addon validate {id}` and `pelty-cli addon pack {id}`.\n")).map_err(io)?;
    let m = json!({
        "id": id, "name": name, "version": "0.1.0", "kind": kind, "engine": engine,
        "description": "", "author": "", "permissions": [],
        "settings": [ { "key": "endpoint", "label": "Endpoint", "kind": "text", "required": false } ],
        "migrations": ["migrations/0001_init.sql"], "files": {}
    });
    std::fs::write(
        dir.join("addon.json"),
        serde_json::to_vec_pretty(&m).map_err(|_| PkgError::BadManifest)?,
    )
    .map_err(io)?;
    rehash(&dir)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default();
        for (n, b) in entries {
            w.start_file(*n, o).unwrap();
            w.write_all(b).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    fn manifest(files: Value) -> Vec<u8> {
        serde_json::to_vec(&json!({"id": "demo-addon", "name": "D", "version": "1", "kind": "plugin", "engine": "demo", "files": files})).unwrap()
    }

    #[test]
    fn accepts_valid_and_nested_root() {
        let a = b"hello";
        let m = manifest(json!({"a.txt": sha256_hex(a)}));
        let p = inspect(
            &zip_of(&[("addon.json", &m), ("a.txt", a)]),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(p.id, "demo-addon");
        let p = inspect(
            &zip_of(&[("demo/addon.json", &m), ("demo/a.txt", a)]),
            Limits::default(),
        )
        .unwrap();
        assert_eq!(p.files.keys().collect::<Vec<_>>(), vec!["a.txt"]);
    }

    #[test]
    fn rejects_traversal_hash_and_extras() {
        let a = b"x";
        let m = manifest(json!({"a.txt": sha256_hex(a)}));
        assert!(matches!(
            inspect(
                &zip_of(&[("addon.json", &m), ("../a.txt", a)]),
                Limits::default()
            ),
            Err(PkgError::UnsafePath(_))
        ));
        assert!(matches!(
            inspect(
                &zip_of(&[("addon.json", &m), ("/etc/a.txt", a)]),
                Limits::default()
            ),
            Err(PkgError::UnsafePath(_))
        ));
        assert!(matches!(
            inspect(
                &zip_of(&[("addon.json", &m), ("a\\..\\b", a)]),
                Limits::default()
            ),
            Err(PkgError::UnsafePath(_))
        ));
        assert!(matches!(
            inspect(
                &zip_of(&[("addon.json", &m), ("a.txt", b"y")]),
                Limits::default()
            ),
            Err(PkgError::Integrity(_))
        ));
        assert!(matches!(
            inspect(
                &zip_of(&[("addon.json", &m), ("a.txt", a), ("b.txt", a)]),
                Limits::default()
            ),
            Err(PkgError::Undeclared(_))
        ));
        let mt = manifest(json!({"../evil": sha256_hex(a)}));
        assert!(matches!(
            inspect(&zip_of(&[("addon.json", &mt)]), Limits::default()),
            Err(PkgError::UnsafePath(_))
        ));
        assert_eq!(
            inspect(b"not a zip", Limits::default()).err(),
            Some(PkgError::BadZip)
        );
        assert_eq!(
            inspect(&zip_of(&[("x.txt", a)]), Limits::default()).err(),
            Some(PkgError::BadManifest)
        );
    }

    #[test]
    fn enforces_limits() {
        let big = vec![0u8; 100_000];
        let m = manifest(json!({"big.bin": sha256_hex(&big)}));
        let z = zip_of(&[("addon.json", &m), ("big.bin", &big)]);
        let lim = Limits {
            max_zip_bytes: 1_000_000,
            max_unpacked_bytes: 50_000,
            max_entries: 10,
        };
        assert_eq!(inspect(&z, lim).err(), Some(PkgError::TooLarge));
        assert_eq!(
            inspect(
                &z,
                Limits {
                    max_zip_bytes: 10,
                    ..lim
                }
            )
            .err(),
            Some(PkgError::TooLarge)
        );
    }

    fn extraction_package() -> Package {
        let m = manifest(json!({"a.txt": sha256_hex(b"new")}));
        inspect(
            &zip_of(&[("addon.json", &m), ("a.txt", b"new")]),
            Limits::default(),
        )
        .unwrap()
    }

    #[test]
    fn replacement_restores_previous_release_on_publication_failure() {
        let tmp = extraction_workspace(&std::env::temp_dir(), "pelty-rollback-test").unwrap();
        let pkg = extraction_package();
        let dest = extract(&pkg, &tmp.path, false).unwrap();
        std::fs::write(dest.join("a.txt"), b"old").unwrap();
        // A stale workspace belonging to another extraction must remain untouched.
        let other = tmp
            .path
            .join(format!(".{}.staging-{}", pkg.id, std::process::id()));
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("marker"), b"owned by another extraction").unwrap();
        let mut calls = 0;
        let result = extract_with_rename(&pkg, &tmp.path, true, |from, to| {
            calls += 1;
            if calls == 1 {
                // Overlapping extraction of the same ID must fail without touching ours.
                assert!(extract(&pkg, &tmp.path, true).is_err());
            }
            if calls == 2 {
                return Err(std::io::Error::other("injected publication failure"));
            }
            std::fs::rename(from, to)
        });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("previous release restored"));
        assert_eq!(calls, 3);
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"old");
        assert_eq!(
            std::fs::read(other.join("marker")).unwrap(),
            b"owned by another extraction"
        );
        assert_eq!(std::fs::read_dir(&tmp.path).unwrap().count(), 2);
        // The lock was released, so a subsequent replacement succeeds.
        extract(&pkg, &tmp.path, true).unwrap();
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"new");
    }

    #[test]
    fn rollback_failure_preserves_backup_and_reports_its_path() {
        let tmp = extraction_workspace(&std::env::temp_dir(), "pelty-backup-test").unwrap();
        let pkg = extraction_package();
        let dest = extract(&pkg, &tmp.path, false).unwrap();
        std::fs::write(dest.join("a.txt"), b"old").unwrap();
        let mut calls = 0;
        let mut backup = PathBuf::new();
        let result = extract_with_rename(&pkg, &tmp.path, true, |from, to| {
            calls += 1;
            if calls == 1 {
                backup = to.to_path_buf();
                std::fs::rename(from, to)
            } else {
                Err(std::io::Error::other("injected rename failure"))
            }
        });
        let error = result.unwrap_err().to_string();
        assert!(error.contains("rollback failed"));
        assert!(error.contains(&backup.display().to_string()));
        assert_eq!(calls, 3);
        assert_eq!(std::fs::read(backup.join("a.txt")).unwrap(), b"old");
        assert!(!dest.exists());
        assert!(!tmp.path.join(format!(".{}.extract-lock", pkg.id)).exists());
    }

    #[test]
    fn failed_initial_rename_and_new_publication_clean_only_owned_workspace() {
        let tmp = extraction_workspace(&std::env::temp_dir(), "pelty-rename-test").unwrap();
        let pkg = extraction_package();
        let fail = |_: &Path, _: &Path| Err(std::io::Error::other("injected rename failure"));
        assert!(extract_with_rename(&pkg, &tmp.path, false, fail).is_err());
        assert_eq!(std::fs::read_dir(&tmp.path).unwrap().count(), 0);
        let dest = extract(&pkg, &tmp.path, false).unwrap();
        std::fs::write(dest.join("a.txt"), b"old").unwrap();
        assert!(extract_with_rename(&pkg, &tmp.path, true, fail).is_err());
        assert_eq!(std::fs::read(dest.join("a.txt")).unwrap(), b"old");
        assert_eq!(std::fs::read_dir(&tmp.path).unwrap().count(), 1);
        let first = extraction_workspace(&tmp.path, &pkg.id).unwrap();
        let second = extraction_workspace(&tmp.path, &pkg.id).unwrap();
        assert_ne!(first.path, second.path);
        drop(first);
        assert!(second.path.is_dir());
    }

    #[test]
    fn extraction_rejects_mutated_id_and_cleans_failed_writes() {
        let tmp = extraction_workspace(&std::env::temp_dir(), "pelty-path-test").unwrap();
        let mut pkg = extraction_package();
        pkg.id = "../escape".into();
        assert_eq!(extract(&pkg, &tmp.path, false), Err(PkgError::BadManifest));
        pkg.id = "demo-addon".into();
        pkg.files.insert("../escape".into(), Vec::new());
        assert!(matches!(
            extract(&pkg, &tmp.path, false),
            Err(PkgError::UnsafePath(_))
        ));
        assert_eq!(std::fs::read_dir(&tmp.path).unwrap().count(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn extraction_rejects_symlink_root_and_dangling_destination() {
        use std::os::unix::fs::symlink;
        let tmp = extraction_workspace(&std::env::temp_dir(), "pelty-symlink-test").unwrap();
        let real_root = tmp.path.join("real");
        std::fs::create_dir(&real_root).unwrap();
        let linked_root = tmp.path.join("linked");
        symlink(&real_root, &linked_root).unwrap();
        let pkg = extraction_package();
        assert!(matches!(
            extract(&pkg, &linked_root, true),
            Err(PkgError::UnsafePath(_))
        ));
        let dest = real_root.join(&pkg.id);
        let absent = tmp.path.join("absent");
        symlink(&absent, &dest).unwrap();
        for replace in [false, true] {
            assert!(matches!(
                extract(&pkg, &real_root, replace),
                Err(PkgError::UnsafePath(_))
            ));
        }
        assert!(std::fs::symlink_metadata(&dest)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(!absent.exists());
    }

    #[test]
    fn signed_pack_extract_and_rehash() {
        use crate::addon_signing::{self, TrustedKey, TrustedKeys};
        let tmp = std::env::temp_dir().join(format!("pelty-signed-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = scaffold(&tmp, "signed-addon", "plugin", "demo", "Signed").unwrap();
        let pkg = inspect(&pack(&dir).unwrap(), Limits::default()).unwrap();
        let (private, public_key) = addon_signing::generate_keypair().unwrap();
        let keys = TrustedKeys::from([(
            "release".into(),
            TrustedKey {
                public_key,
                revoked: false,
            },
        )]);
        let sig = addon_signing::sign(&pkg, "release", &private).unwrap();
        std::fs::write(dir.join(SIGNATURE_FILE), serde_json::to_vec(&sig).unwrap()).unwrap();
        let pkg = inspect(&pack(&dir).unwrap(), Limits::default()).unwrap();
        assert_eq!(
            addon_signing::verify(&pkg, &keys, true).unwrap(),
            Some("release".into())
        );
        let extracted = extract(&pkg, &tmp.join("installed"), false).unwrap();
        let pkg = inspect(&pack(&extracted).unwrap(), Limits::default()).unwrap();
        assert!(addon_signing::verify(&pkg, &keys, true).is_ok());
        std::fs::write(dir.join("README.md"), "changed").unwrap();
        let m = rehash(&dir).unwrap();
        assert!(m["files"].get(SIGNATURE_FILE).is_none());
        let pkg = inspect(&pack(&dir).unwrap(), Limits::default()).unwrap();
        assert!(addon_signing::verify(&pkg, &keys, false).is_err());
        let mut m = m;
        m["files"][SIGNATURE_FILE] = json!(sha256_hex(b"x"));
        std::fs::write(dir.join("addon.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(matches!(pack(&dir), Err(PkgError::UnsafePath(_))));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scaffold_pack_extract_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("pelty-pkg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let dir = scaffold(&tmp, "my-addon", "plugin", "my-addon", "My addon").unwrap();
        let z = pack(&dir).unwrap();
        let p = inspect(&z, Limits::default()).unwrap();
        assert!(p.files.contains_key("migrations/0001_init.sql"));
        let out = extract(&p, &tmp.join("installed"), false).unwrap();
        assert!(out.join("migrations/0001_init.sql").exists());
        assert!(extract(&p, &tmp.join("installed"), false).is_err());
        extract(&p, &tmp.join("installed"), true).unwrap();
        std::fs::write(dir.join("README.md"), "changed").unwrap();
        assert!(matches!(pack(&dir), Err(PkgError::Integrity(_))));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
