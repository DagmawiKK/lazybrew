//! Catalog: the full remote package universe from formulae.brew.sh,
//! cached for 24h in the XDG cache directory.

use crate::brew::Package;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

const FORMULAE_URL: &str = "https://formulae.brew.sh/api/formula.json";
const CASK_URL: &str = "https://formulae.brew.sh/api/cask.json";
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const MIN_SIZE: i64 = 1024;

#[derive(Deserialize)]
struct RemoteFormula {
    name: String,
    #[serde(default)]
    desc: Option<String>,
    versions: RemoteVersions,
}

#[derive(Deserialize)]
struct RemoteVersions {
    stable: Option<String>,
}

#[derive(Deserialize)]
struct RemoteCask {
    token: String,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    version: Option<String>,
}

fn cache_dir() -> Option<std::path::PathBuf> {
    dirs::cache_dir().map(|d| d.join("lazybrew"))
}

fn fetch_url(url: &str) -> Result<Vec<u8>> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "--max-time", "60", url])
        .output()
        .with_context(|| format!("downloading {url}"))?;
    if !out.status.success() {
        anyhow::bail!("curl {url} failed");
    }
    Ok(out.stdout)
}

/// Read a cached file if it is fresh and large enough, otherwise fetch and store it.
fn cached_fetch(url: &str, cache_file: &str) -> Result<Vec<u8>> {
    let Some(dir) = cache_dir() else {
        return fetch_url(url);
    };
    let path = dir.join(cache_file);
    if let Some(data) = read_fresh(&path) {
        return Ok(data);
    }
    let data = fetch_url(url)?;
    if data.len() as i64 >= MIN_SIZE {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, &data);
    }
    Ok(data)
}

fn read_fresh(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() < MIN_SIZE as u64 {
        return None;
    }
    let age = SystemTime::now()
        .duration_since(meta.modified().ok()?)
        .ok()?;
    if age > CACHE_TTL {
        return None;
    }
    std::fs::read(path).ok()
}

/// Merge remote JSON payloads with installed package status.
fn merge_remote(installed: &[Package], f_data: &[u8], c_data: &[u8]) -> Result<Vec<Package>> {
    let remote_formulae: Vec<RemoteFormula> =
        serde_json::from_slice(f_data).context("parsing formula.json")?;
    let remote_casks: Vec<RemoteCask> =
        serde_json::from_slice(c_data).context("parsing cask.json")?;

    // Index installed packages for status merging
    let by_name: HashMap<&str, &Package> = installed.iter().map(|p| (p.name.as_str(), p)).collect();

    let mut pkgs: Vec<Package> = Vec::with_capacity(remote_formulae.len() + remote_casks.len());
    for f in remote_formulae {
        let local = by_name.get(f.name.as_str()).copied();
        pkgs.push(Package {
            installed_version: local.and_then(|p| p.installed_version.clone()),
            outdated: local.is_some_and(|p| p.outdated),
            pinned: local.is_some_and(|p| p.pinned),
            version: f.versions.stable.unwrap_or_else(|| "?".into()),
            name: f.name,
            desc: f.desc.unwrap_or_default(),
            cask: false,
            service_status: None,
        });
    }
    for c in remote_casks {
        let local = by_name.get(c.token.as_str()).copied();
        pkgs.push(Package {
            installed_version: local.and_then(|p| p.installed_version.clone()),
            outdated: local.is_some_and(|p| p.outdated),
            pinned: false,
            version: c.version.unwrap_or_else(|| "latest".into()),
            name: c.token,
            desc: c.desc.unwrap_or_default(),
            cask: true,
            service_status: None,
        });
    }
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(pkgs)
}

/// Load the remote catalog and merge installed status into it.
/// Returns a single package vector sorted by name.
pub fn load_catalog(installed: &[Package]) -> Result<Vec<Package>> {
    let (f_data, c_data) = std::thread::scope(|s| {
        let a = s.spawn(|| cached_fetch(FORMULAE_URL, "formula.json"));
        let b = s.spawn(|| cached_fetch(CASK_URL, "cask.json"));
        (
            a.join().unwrap_or_else(|_| fetch_url(FORMULAE_URL)),
            b.join().unwrap_or_else(|_| fetch_url(CASK_URL)),
        )
    });

    merge_remote(installed, &f_data?, &c_data?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_remote_shapes() {
        let f: Vec<RemoteFormula> =
            serde_json::from_str(r#"[{"name":"git","desc":"vcs","versions":{"stable":"2.0"}}]"#)
                .unwrap();
        assert_eq!(f[0].name, "git");
        let c: Vec<RemoteCask> =
            serde_json::from_str(r#"[{"token":"firefox","version":"1.0"}]"#).unwrap();
        assert_eq!(c[0].token, "firefox");
    }

    #[test]
    fn merge_marks_installed() {
        let inst = Package {
            name: "git".into(),
            desc: "vcs".into(),
            version: "2.0".into(),
            cask: false,
            outdated: true,
            installed_version: Some("1.9".into()),
            pinned: false,
            service_status: None,
        };
        let remote = r#"[{"name":"git","desc":"vcs","versions":{"stable":"2.0"}}]"#;
        let catalog = merge_remote(&[inst], remote.as_bytes(), b"[]").unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].installed_version.as_deref(), Some("1.9"));
        assert!(catalog[0].outdated);
    }
}
