//! Brewfile parsing (local path or https URL).

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Brew,
    Cask,
    Tap,
    Other,
}

/// Extract the first quoted value from a line.
/// Handles `brew "git", restart_service: true # note "x"` — first pair wins.
fn extract_quoted_value(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let end = line[start + 1..].find('"')?;
    Some(line[start + 1..start + 1 + end].to_string())
}

/// Parse Brewfile content into entries.
pub fn parse(content: &str) -> Vec<Entry> {
    content
        .lines()
        .filter_map(|raw| {
            let line = raw.trim();
            let kind = if line.starts_with("brew") {
                EntryKind::Brew
            } else if line.starts_with("cask") {
                EntryKind::Cask
            } else if line.starts_with("tap") {
                EntryKind::Tap
            } else {
                EntryKind::Other
            };
            if kind == EntryKind::Other {
                return None;
            }
            let name = extract_quoted_value(line)?;
            if name.is_empty() {
                return None;
            }
            Some(Entry { kind, name })
        })
        .collect()
}

/// Load a Brewfile from a local path or an https URL.
pub fn load(path_or_url: &str) -> Result<Vec<Entry>> {
    let content = if path_or_url.starts_with("https://") {
        let out = std::process::Command::new("curl")
            .args(["-fsSL", "--max-time", "30", path_or_url])
            .output()
            .context("downloading Brewfile")?;
        if !out.status.success() {
            anyhow::bail!("failed to download {path_or_url}");
        }
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        std::fs::read_to_string(path_or_url).with_context(|| format!("reading {path_or_url}"))?
    };
    Ok(parse(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_entries() {
        let content = r#"
tap "homebrew/services"
brew "git"
brew "zsh", args: ["with-..."] # note "ignored"
cask "firefox"
# comment
mas "xcode"
"#;
        let entries = parse(content);
        assert_eq!(
            entries,
            vec![
                Entry {
                    kind: EntryKind::Tap,
                    name: "homebrew/services".into()
                },
                Entry {
                    kind: EntryKind::Brew,
                    name: "git".into()
                },
                Entry {
                    kind: EntryKind::Brew,
                    name: "zsh".into()
                },
                Entry {
                    kind: EntryKind::Cask,
                    name: "firefox".into()
                },
            ]
        );
    }

    #[test]
    fn ignores_commented_lines() {
        let entries = parse("# brew \"git\"\nbrew \"wget\"");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "wget");
    }
}
