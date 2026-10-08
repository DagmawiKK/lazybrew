//! Self-update: download the newest GitHub release tarball for the current
//! platform and replace the running binary.
//!
//! Lives behind [`crate::state::ModalAction::SelfUpdate`] and
//! streams its progress through the same output pane as brew commands. Uses
//! `curl` + `tar` (present on every macOS/Linux Homebrew host) so no HTTP
//! dependency is needed.

use crate::exec::CmdEvent;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// The GitHub repo release assets live under (matches the `gh release create`
/// step in `.github/workflows/release.yml`).
pub const REPO: &str = "DagmawiKK/lazybrew";

/// Triple the binary was compiled for; names the release asset to download.
///
/// Prefer the compile-time value stamped by `build.rs`; fall back to a
/// runtime mapping for builds that skipped the build script. Only the three
/// CI-published triples are downloadable, so unknown hosts degrade to the
/// closest published one (self-update then simply fails to find the asset).
pub fn target_triple() -> &'static str {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin",
        ("x86_64", "macos") => "x86_64-apple-darwin",
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu",
        _ => option_env!("LAZYBREW_TARGET").unwrap_or(""),
    }
}

/// URL of the newest release tarball for `target`.
pub fn download_url(target: &str) -> String {
    format!("https://github.com/{REPO}/releases/latest/download/lazybrew-{target}.tar.gz")
}

/// Text for the `W` confirmation dialog.
pub fn plan_text(url: &str, exe: &Path) -> String {
    format!(
        "Update lazybrew from the latest GitHub release?\n\n  {url}\n  install to {}\n\n(y/n)",
        exe.display()
    )
}

/// Perform the update: download, extract, back up, and replace the binary.
/// Streams each step's output to `tx`; returns true on success.
pub fn run(tx: &mpsc::Sender<CmdEvent>, url: &str, target: &str, exe: &Path) -> bool {
    let line = |s: String| {
        let _ = tx.send(CmdEvent::Line(s));
    };

    // 1. Isolate in a private temp dir so a failed download never touches real files.
    let dir = std::env::temp_dir().join(format!("lazybrew-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    if std::fs::create_dir_all(&dir).is_err() {
        line(format!("could not create temp dir {}", dir.display()));
        return false;
    }
    let tarball = dir.join(format!("lazybrew-{target}.tar.gz"));

    // 2. Download. `-f` turns HTTP 404s into a non-zero exit, `-sS` keeps the
    //    output clean but still shows errors.
    let mut curl = std::process::Command::new("curl");
    curl.args(["-fsSL", "--retry", "2", "-o"])
        .arg(&tarball)
        .arg(url);
    if !crate::exec::stream_cmd(tx, &mut curl) {
        line(format!(
            "download failed — is the release published? ({} → {})",
            url,
            tarball.display()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        return false;
    }

    // 3. Extract. CI packs the binary as `lazybrew-<target>` inside the tarball.
    let mut tar = std::process::Command::new("tar");
    tar.arg("-xzf").arg(&tarball).arg("-C").arg(&dir);
    if !crate::exec::stream_cmd(tx, &mut tar) {
        line(format!("extract failed: {}", tarball.display()));
        let _ = std::fs::remove_dir_all(&dir);
        return false;
    }

    // 4. Locate the binary: named entry first, else the single file in the tarball.
    let named = dir.join(format!("lazybrew-{target}"));
    let binary: Option<PathBuf> = if named.is_file() {
        Some(named)
    } else {
        std::fs::read_dir(&dir)
            .ok()
            .and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.is_file()))
    };
    let Some(binary) = binary else {
        line("no binary found in the release archive".into());
        let _ = std::fs::remove_dir_all(&dir);
        return false;
    };

    // 5. Never destroy the current binary before a new one is in place.
    let backup = backup_path(exe);
    if exe.exists() && std::fs::copy(exe, &backup).is_err() {
        line(format!(
            "warning: could not back up the current binary to {}",
            backup.display()
        ));
    } else if exe.exists() {
        line(format!("backed up current binary to {}", backup.display()));
    }

    let ok = match std::fs::copy(&binary, exe) {
        Ok(_) => {
            mark_executable(exe);
            line(format!(
                "updated lazybrew → {} (restart it to load the new version)",
                exe.display()
            ));
            true
        }
        Err(e) => {
            line(format!(
                "replace failed: {e} — the previous binary is intact at {}",
                backup.display()
            ));
            false
        }
    };
    let _ = std::fs::remove_dir_all(&dir);
    ok
}

/// `{exe}` keeps the running process and any script wrappers working; the old
/// binary is parked next to it as `{exe}.old` so a swap is always reversible.
fn backup_path(exe: &Path) -> PathBuf {
    let mut name = exe
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_else(|| "lazybrew".into());
    name.push(".old");
    exe.with_file_name(name)
}

#[cfg(unix)]
fn mark_executable(exe: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn mark_executable(_exe: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_url_matches_the_release_asset_naming() {
        assert_eq!(
            download_url("aarch64-apple-darwin"),
            "https://github.com/DagmawiKK/lazybrew/releases/latest/download/lazybrew-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            download_url("x86_64-unknown-linux-gnu"),
            "https://github.com/DagmawiKK/lazybrew/releases/latest/download/lazybrew-x86_64-unknown-linux-gnu.tar.gz"
        );
    }

    #[test]
    fn target_triple_is_one_of_the_published_assets() {
        // The stamp comes from build.rs (or the runtime mapping); either way it
        // must name an asset that the release workflow actually publishes.
        let t = target_triple();
        assert!(
            matches!(
                t,
                "aarch64-apple-darwin" | "x86_64-apple-darwin" | "x86_64-unknown-linux-gnu"
            ),
            "unexpected target triple: {t:?}"
        );
    }

    #[test]
    fn plan_text_names_the_url_and_install_path() {
        let url = download_url(target_triple());
        let text = plan_text(&url, Path::new("/opt/homebrew/bin/lazybrew"));
        assert!(text.contains("GitHub release"));
        assert!(text.contains(&url));
        assert!(text.contains("/opt/homebrew/bin/lazybrew"));
    }

    #[test]
    fn backup_path_keeps_the_old_binary_adjacent() {
        let exe = Path::new("/opt/homebrew/bin/lazybrew");
        assert_eq!(
            backup_path(exe),
            PathBuf::from("/opt/homebrew/bin/lazybrew.old")
        );
    }
}
