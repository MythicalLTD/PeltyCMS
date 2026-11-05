//! Operator trust is separate from package hashes and publisher-supplied metadata.
use crate::{addons::CatalogEntry, AppState};
use pelty_core::{AppError, AppResult};
use pelty_extensions::{addon_pkg::{self, Package}, addon_signing::{self, ReleaseSignature, TrustedKeys}};
use std::{collections::BTreeMap, io::Read, path::Path};

const TRUST_LIMIT: usize = 64 * 1024;
const MANIFEST_LIMIT: usize = 1024 * 1024;

fn invalid() -> AppError { AppError::Validation("error.addon_signature") }

/// Read on every install/activation so operator key revocations take effect at
/// the next activation without treating a package's embedded key as trusted.
pub fn keys(s: &AppState) -> AppResult<TrustedKeys> {
    let Some(path) = &s.cfg.addon_trusted_keys else {
        if s.cfg.addon_require_signatures { return Err(invalid()); }
        return Ok(BTreeMap::new());
    };
    let file = std::fs::File::open(path).map_err(|_| invalid())?;
    let mut raw = Vec::new();
    file.take((TRUST_LIMIT + 1) as u64).read_to_end(&mut raw).map_err(|_| invalid())?;
    if raw.len() > TRUST_LIMIT { return Err(invalid()); }
    let keys: TrustedKeys = serde_json::from_slice(&raw).map_err(|_| invalid())?;
    if keys.len() > 256 || keys.iter().any(|(id, key)| !addon_signing::key_id_ok(id) || addon_signing::decode_public_key(&key.public_key).is_err()) {
        return Err(invalid());
    }
    Ok(keys)
}

pub fn verify_package(s: &AppState, pkg: &Package) -> AppResult<Option<String>> {
    addon_signing::verify(pkg, &keys(s)?, s.cfg.addon_require_signatures).map_err(|error| {
        tracing::warn!(addon = %pkg.id, error = %error, "addon release trust rejected");
        invalid()
    })
}

fn read_file(dir: &Path, rel: &str, limit: usize) -> AppResult<Vec<u8>> {
    if !addon_pkg::safe_rel(rel) { return Err(invalid()); }
    let mut path = dir.to_path_buf();
    if std::fs::symlink_metadata(&path).map_err(|_| invalid())?.file_type().is_symlink() { return Err(invalid()); }
    for part in rel.split('/') {
        path.push(part);
        if std::fs::symlink_metadata(&path).map_err(|_| invalid())?.file_type().is_symlink() { return Err(invalid()); }
    }
    let file = std::fs::File::open(&path).map_err(|_| invalid())?;
    if !file.metadata().map_err(|_| invalid())?.is_file() { return Err(invalid()); }
    let mut raw = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut raw).map_err(|_| invalid())?;
    if raw.len() > limit { return Err(invalid()); }
    Ok(raw)
}

/// Reverify the exact disk bytes before migrations or runtime activation. This
/// also covers direct-disk installs and restarts, not just the upload endpoint.
pub fn verify_entry(s: &AppState, entry: &CatalogEntry) -> AppResult<Option<String>> {
    let Some(dir) = &entry.dir else { return Ok(None); };
    let manifest_raw = read_file(dir, "addon.json", MANIFEST_LIMIT)?;
    if addon_pkg::sha256_hex(&manifest_raw) != entry.hash { return Err(invalid()); }
    let manifest = serde_json::from_slice(&manifest_raw).map_err(|_| invalid())?;
    if entry.manifest.files.len() > crate::addons::PKG_LIMITS.max_entries { return Err(invalid()); }
    let mut remaining = crate::addons::PKG_LIMITS.max_unpacked_bytes as usize;
    let mut files = BTreeMap::new();
    for (rel, expected) in &entry.manifest.files {
        if rel == "addon.json" || rel == addon_signing::SIGNATURE_FILE { return Err(invalid()); }
        let raw = read_file(dir, rel, remaining)?;
        if !addon_pkg::sha256_hex(&raw).eq_ignore_ascii_case(expected) { return Err(invalid()); }
        remaining = remaining.saturating_sub(raw.len());
        files.insert(rel.clone(), raw);
    }
    let signature = match std::fs::symlink_metadata(dir.join(addon_signing::SIGNATURE_FILE)) {
        Ok(_) => Some(ReleaseSignature::parse(&read_file(dir, addon_signing::SIGNATURE_FILE, addon_signing::MAX_SIGNATURE_BYTES)?).map_err(|_| invalid())?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(invalid()),
    };
    verify_package(s, &Package { manifest_raw, manifest, id: entry.manifest.id.clone(), signature, files })
}
