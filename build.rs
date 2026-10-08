//! Stamp the compile-time target triple into the binary so self-update knows
//! which release tarball to download (`lazybrew-<target>.tar.gz`).
//!
//! Cargo sets `TARGET` for build scripts even when cross-compiling, so the
//! value always matches the names used in the release workflow. Builds that
//! skip the build script (e.g. rust-analyzer) fall back to a runtime mapping
//! in `self_update::target_triple`.

fn main() {
    if let Ok(target) = std::env::var("TARGET") {
        println!("cargo:rustc-env=LAZYBREW_TARGET={target}");
    }
}
