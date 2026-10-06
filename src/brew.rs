use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;

/// A unified Homebrew package (formula or cask).
#[derive(Debug, Clone)]
pub struct Package {
    pub name: String,
    pub desc: String,
    pub version: String,
    pub cask: bool,
    pub outdated: bool,
    pub installed_version: Option<String>,
    pub pinned: bool,
}

#[derive(Deserialize)]
struct InstalledV2 {
    #[serde(default)]
    formulae: Vec<InstalledFormula>,
    #[serde(default)]
    casks: Vec<InstalledCask>,
}

#[derive(Deserialize)]
struct InstalledFormula {
    name: String,
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    desc: Option<String>,
    versions: Versions,
    #[serde(default)]
    installed: Vec<InstalledRef>,
    #[serde(default)]
    pinned: bool,
}

#[derive(Deserialize)]
struct Versions {
    stable: Option<String>,
}

#[derive(Deserialize)]
struct InstalledRef {
    version: Option<String>,
}

#[derive(Deserialize)]
struct InstalledCask {
    token: String,
    #[serde(default)]
    desc: Option<String>,
    version: Option<String>,
    #[serde(default)]
    installed: Option<String>,
}

#[derive(Deserialize)]
struct OutdatedV2 {
    #[serde(default)]
    formulae: Vec<OutdatedFormula>,
    #[serde(default)]
    casks: Vec<OutdatedCask>,
}

#[derive(Deserialize)]
struct OutdatedFormula {
    name: String,
    current_version: Option<String>,
}

#[derive(Deserialize)]
struct OutdatedCask {
    name: String,
    current_version: Option<String>,
}

fn brew_cmd(args: &[&str]) -> Command {
    let mut cmd = Command::new("brew");
    cmd.args(args)
        .env("NONINTERACTIVE", "1")
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1");
    cmd
}

fn brew_json<T: for<'de> Deserialize<'de>>(args: &[&str]) -> Result<T> {
    let out = brew_cmd(args)
        .output()
        .with_context(|| format!("failed to run brew {:?}", args))?;
    if !out.status.success() {
        anyhow::bail!(
            "brew {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    serde_json::from_slice(&out.stdout).with_context(|| format!("parsing brew {:?} output", args))
}

/// Load all installed formulae and casks, annotated with outdated status.
pub fn load_installed() -> Result<Vec<Package>> {
    let installed: InstalledV2 = brew_json(&["info", "--installed", "--json=v2"])?;

    let mut outdated: HashMap<(bool, String), String> = HashMap::new();
    if let Ok(o) = brew_json::<OutdatedV2>(&["outdated", "--json=v2"]) {
        for f in o.formulae {
            outdated.insert((false, f.name), f.current_version.unwrap_or_default());
        }
        for c in o.casks {
            outdated.insert((true, c.name), c.current_version.unwrap_or_default());
        }
    }

    let mut pkgs = Vec::new();
    for f in installed.formulae {
        let is_outdated = outdated.contains_key(&(false, f.name.clone()))
            || f
                .full_name
                .as_ref()
                .map(|fn_| outdated.contains_key(&(false, fn_.clone())))
                .unwrap_or(false);
        pkgs.push(Package {
            version: f.versions.stable.unwrap_or_else(|| "?".into()),
            installed_version: f.installed.first().and_then(|i| i.version.clone()),
            name: f.name,
            desc: f.desc.unwrap_or_default(),
            cask: false,
            outdated: is_outdated,
            pinned: f.pinned,
        });
    }
    for c in installed.casks {
        let is_outdated = outdated.contains_key(&(true, c.token.clone()));
        pkgs.push(Package {
            version: c.version.unwrap_or_else(|| "?".into()),
            installed_version: c.installed,
            name: c.token,
            desc: c.desc.unwrap_or_default(),
            cask: true,
            outdated: is_outdated,
            pinned: false,
        });
    }
    pkgs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(pkgs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_shape() {
        let json = r#"{"formulae":[{"name":"git","desc":"dvcs","versions":{"stable":"2.5.0"},"installed":[{"version":"2.4.0"}]}],"casks":[{"token":"firefox","desc":"browser","version":"1.0","installed":"1.0"}]}"#;
        let v: InstalledV2 = serde_json::from_str(json).unwrap();
        assert_eq!(v.formulae.len(), 1);
        assert_eq!(v.casks[0].token, "firefox");
        assert_eq!(v.formulae[0].installed[0].version.as_deref(), Some("2.4.0"));
    }
}
