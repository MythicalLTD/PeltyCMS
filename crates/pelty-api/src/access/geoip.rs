//! Offline IP → location lookups via an MMDB file (DB-IP Lite or GeoLite2).
//!
//! Prefer [DB-IP City Lite](https://db-ip.com/db/download/ip-to-city-lite)
//! (CC BY 4.0, no account): download with `scripts/download-geoip.sh` or set
//! `PELTY_GEOIP_DB` to any compatible `.mmdb`. MaxMind GeoLite2 also works if
//! you already have a license key and file.
use serde::Serialize;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Default relative path when `PELTY_GEOIP_DB` is unset.
pub const DEFAULT_PATH: &str = "data/geoip/dbip-city-lite.mmdb";

#[derive(Debug, Clone, Default, Serialize)]
pub struct GeoHit {
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
    pub country_name: Option<String>,
    /// Human line for emails / UI, e.g. `Reșița, Caraș-Severin, Romania`.
    pub location: String,
}

impl GeoHit {
    fn private() -> Self {
        Self {
            location: "private network".into(),
            ..Default::default()
        }
    }

    fn unknown() -> Self {
        Self {
            location: "-".into(),
            ..Default::default()
        }
    }

    fn compose(city: Option<String>, region: Option<String>, country: Option<String>, country_name: Option<String>) -> Self {
        let mut parts = Vec::new();
        if let Some(c) = city.as_ref().filter(|s| !s.is_empty()) {
            parts.push(c.clone());
        }
        if let Some(r) = region.as_ref().filter(|s| !s.is_empty()) {
            parts.push(r.clone());
        }
        let tail = country_name
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| country.clone());
        if let Some(t) = tail {
            parts.push(t);
        }
        let location = if parts.is_empty() { "-".into() } else { parts.join(", ") };
        Self { city, region, country, country_name, location }
    }
}

/// Loaded MMDB reader, or an empty stub when no file is available.
pub struct GeoIp {
    reader: Option<maxminddb::Reader<Vec<u8>>>,
    path: Option<PathBuf>,
}

impl GeoIp {
    /// Resolve path from config / env default, then open the MMDB if present.
    pub fn load(configured: Option<&Path>) -> Arc<Self> {
        let path = configured
            .map(Path::to_path_buf)
            .filter(|p| !p.as_os_str().is_empty())
            .or_else(|| {
                let p = PathBuf::from(DEFAULT_PATH);
                p.is_file().then_some(p)
            });
        let Some(path) = path else {
            tracing::info!("GeoIP DB unset (set PELTY_GEOIP_DB or place {DEFAULT_PATH})");
            return Arc::new(Self { reader: None, path: None });
        };
        match maxminddb::Reader::open_readfile(&path) {
            Ok(reader) => {
                tracing::info!(path = %path.display(), "GeoIP MMDB loaded");
                Arc::new(Self { reader: Some(reader), path: Some(path) })
            }
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "GeoIP MMDB not loaded");
                Arc::new(Self { reader: None, path: Some(path) })
            }
        }
    }

    pub fn loaded(&self) -> bool {
        self.reader.is_some()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Status for Admin → Security (read-only).
    pub fn status(&self) -> Value {
        json!({
            "loaded": self.loaded(),
            "path": self.path.as_ref().map(|p| p.display().to_string()),
            "default_path": DEFAULT_PATH,
            "env": "PELTY_GEOIP_DB",
            "attribution": "https://db-ip.com",
            "attribution_label": "IP Geolocation by DB-IP",
            "download_hint": "scripts/download-geoip.sh",
        })
    }

    /// Look up an IP string. Private/loopback ranges never hit the DB.
    pub fn lookup(&self, ip: &str) -> GeoHit {
        let Ok(addr) = ip.parse::<IpAddr>() else {
            return GeoHit::unknown();
        };
        if is_private(addr) {
            return GeoHit::private();
        }
        let Some(reader) = &self.reader else {
            return GeoHit::unknown();
        };
        match reader.lookup::<maxminddb::geoip2::City>(addr) {
            Ok(Some(city)) => {
                let country = city.country.as_ref().and_then(|c| c.iso_code.map(str::to_string));
                let country_name = city
                    .country
                    .as_ref()
                    .and_then(|c| c.names.as_ref())
                    .and_then(|n| n.get("en").or_else(|| n.values().next()).map(|s| (*s).to_string()));
                let city_name = city
                    .city
                    .as_ref()
                    .and_then(|c| c.names.as_ref())
                    .and_then(|n| n.get("en").or_else(|| n.values().next()).map(|s| (*s).to_string()));
                let region = city
                    .subdivisions
                    .as_ref()
                    .and_then(|s| s.first())
                    .and_then(|s| {
                        s.names
                            .as_ref()
                            .and_then(|n| n.get("en").or_else(|| n.values().next()).map(|x| (*x).to_string()))
                            .or_else(|| s.iso_code.map(str::to_string))
                    });
                GeoHit::compose(city_name, region, country, country_name)
            }
            Ok(None) | Err(_) => GeoHit::unknown(),
        }
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified(),
        IpAddr::V6(v) => v.is_loopback() || v.is_unique_local() || v.is_unicast_link_local() || v.is_unspecified(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ips_skip_db() {
        let g = GeoIp { reader: None, path: None };
        assert_eq!(g.lookup("127.0.0.1").location, "private network");
        assert_eq!(g.lookup("10.0.0.1").location, "private network");
        assert_eq!(g.lookup("192.168.1.1").location, "private network");
        assert_eq!(g.lookup("::1").location, "private network");
    }

    #[test]
    fn missing_db_is_dash() {
        let g = GeoIp { reader: None, path: None };
        assert_eq!(g.lookup("8.8.8.8").location, "-");
    }

    #[test]
    fn compose_joins_parts() {
        let h = GeoHit::compose(
            Some("Reșița".into()),
            Some("Caraș-Severin".into()),
            Some("RO".into()),
            Some("Romania".into()),
        );
        assert_eq!(h.location, "Reșița, Caraș-Severin, Romania");
    }

    #[test]
    fn loads_default_db_when_present() {
        let g = GeoIp::load(None);
        if !Path::new(DEFAULT_PATH).is_file() {
            return;
        }
        assert!(g.loaded());
        let hit = g.lookup("8.8.8.8");
        assert!(hit.country.is_some(), "{hit:?}");
        assert_ne!(hit.location, "-");
        assert_ne!(hit.location, "private network");
    }
}
