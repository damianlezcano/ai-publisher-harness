//! Sidecar resolution for the packaged app (M10).
//!
//! Tauri-free: this module locates the bundled `opencode`/`cloudflared`
//! binaries relative to the installed app directory, falling back to a bare
//! `PATH` name for development builds. Resolution is pure and offline; the
//! shell (Tauri) supplies the install directory. No unsafe code.

use std::fs;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the sidecar directory. Lets manual and
/// packaged-layout testing point at a sidecar bundle without a full install.
pub const EDUCAI_SIDECAR_DIR: &str = "EDUCAI_SIDECAR_DIR";

/// Tauri's `bundle.externalBin` names bundled binaries with a host target
/// triple suffix (e.g. `opencode-x86_64-unknown-linux-gnu`). This must agree
/// with Tauri's native bundle target; it is deliberately bounded here rather
/// than leaked into domain/process code.
#[cfg(target_os = "windows")]
const TARGET_TRIPLE: &str = "x86_64-pc-windows-msvc";
#[cfg(not(target_os = "windows"))]
const TARGET_TRIPLE: &str = "x86_64-unknown-linux-gnu";

/// Where a sidecar was located: a bundled absolute path, or a bare name to be
/// resolved through `PATH` (the dev fallback; lazy failure preserved).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SidecarLocation {
    Bundled(PathBuf),
    OnPath(String),
}

/// Resolves `name` relative to `install_dir`, then falls back to `PATH`.
///
/// Resolution order (first hit wins):
/// 1. `install_dir/<name>` if it is a regular, executable file.
/// 2. `install_dir/<name>-<triple>` (Tauri external-bin suffix) if executable.
/// 3. Otherwise `OnPath(name)` — the bare dev/`PATH` fallback, regardless of
///    `path_var`'s content (`path_var` is passed for determinism).
pub fn resolve_sidecar(name: &str, install_dir: &Path, path_var: &str) -> SidecarLocation {
    let _ = path_var;
    bundled_in(install_dir, name)
        .map(SidecarLocation::Bundled)
        .unwrap_or_else(|| SidecarLocation::OnPath(name.to_owned()))
}

/// Like [`resolve_sidecar`], but first honors `EDUCAI_SIDECAR_DIR` when set:
/// `<dir>/<name>` and `<dir>/<name>-<triple>` there win over the install dir.
/// Falls through to the install-dir/`PATH` logic when the override is unset,
/// empty, or does not contain an executable match.
pub fn resolve_sidecar_from_env(name: &str, install_dir: &Path, path_var: &str) -> SidecarLocation {
    let override_dir = std::env::var_os(EDUCAI_SIDECAR_DIR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    if let Some(dir) = override_dir
        && let Some(path) = bundled_in(&dir, name)
    {
        return SidecarLocation::Bundled(path);
    }
    resolve_sidecar(name, install_dir, path_var)
}

/// Looks for an executable regular `name` (and its target-triple-suffixed
/// variant) directly under `dir`, returning the absolute bundled path on a hit.
fn bundled_in(dir: &Path, name: &str) -> Option<PathBuf> {
    for candidate in bundled_candidates(name) {
        let path = dir.join(candidate);
        if is_executable_regular_file(&path) {
            return Some(absolute(path));
        }
    }
    None
}

fn bundled_candidates(name: &str) -> Vec<String> {
    let suffix = if cfg!(target_os = "windows") {
        ".exe"
    } else {
        ""
    };
    bundled_candidates_for(name, TARGET_TRIPLE, suffix)
}

fn bundled_candidates_for(name: &str, target_triple: &str, suffix: &str) -> Vec<String> {
    vec![
        format!("{name}{suffix}"),
        format!("{name}-{target_triple}{suffix}"),
    ]
}

/// `true` when `path` exists, is a regular file (not a symlink or directory),
/// and — on unix — has at least one execute bit set (`mode & 0o111`). Any other
/// outcome (missing, directory, symlink, non-executable) is skipped.
fn is_executable_regular_file(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_file() {
        return false;
    }
    executable_permissions(&meta)
}

#[cfg(unix)]
fn executable_permissions(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable_permissions(_meta: &fs::Metadata) -> bool {
    true
}

/// Lexically makes `path` absolute without touching the filesystem (absolute
/// inputs are returned unchanged), so `Bundled` always carries an absolute
/// path even when the caller passes a relative install directory.
fn absolute(path: PathBuf) -> PathBuf {
    std::path::absolute(&path).unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::bundled_candidates_for;

    #[test]
    fn windows_bundle_candidates_use_exe_and_msvc_suffix() {
        assert_eq!(
            bundled_candidates_for("cloudflared", "x86_64-pc-windows-msvc", ".exe"),
            ["cloudflared.exe", "cloudflared-x86_64-pc-windows-msvc.exe"]
        );
    }

    #[test]
    fn linux_bundle_candidates_keep_linux_names() {
        assert_eq!(
            bundled_candidates_for("cloudflared", "x86_64-unknown-linux-gnu", ""),
            ["cloudflared", "cloudflared-x86_64-unknown-linux-gnu"]
        );
    }
}
