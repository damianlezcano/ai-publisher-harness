//! Offline, deterministic integration tests for sidecar resolution (M10 T1).
//!
//! Every test runs against `tempfile::tempdir()` install directories; no
//! network, no bundle build, no sidecar download. Permission-bit assertions are
//! unix-only (M10 is Linux-first).

use std::fs;
use std::path::{Path, PathBuf};

use project_app::sidecar::{
    EDUCAI_SIDECAR_DIR, SidecarLocation, resolve_sidecar, resolve_sidecar_from_env,
};
use tempfile::tempdir;

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, b"#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

#[cfg(unix)]
fn make_non_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::write(path, b"#!/bin/sh\nexit 0\n").unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(path, perms).unwrap();
}

#[test]
#[cfg(unix)]
fn bundled_when_executable_in_install_dir() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("opencode");
    make_executable(&file);
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), ""),
        SidecarLocation::Bundled(file)
    );
}

#[test]
fn on_path_when_install_dir_empty_and_path_var_contains_name() {
    let dir = tempdir().unwrap();
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), "opencode"),
        SidecarLocation::OnPath("opencode".to_owned())
    );
}

#[test]
fn on_path_when_install_dir_and_path_var_empty() {
    let dir = tempdir().unwrap();
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), ""),
        SidecarLocation::OnPath("opencode".to_owned())
    );
}

/// Runs `resolve_sidecar_from_env` under a real `EDUCAI_SIDECAR_DIR`. Passes
/// trivially when launched in-process (env unset); the parent
/// [`env_override_wins`] test re-runs it as a subprocess with the variable set,
/// because setting process-wide env vars in a parallel test run is unsafe in
/// edition 2024.
#[test]
#[cfg(unix)]
fn env_override_probe() {
    let Some(env_dir) = std::env::var_os(EDUCAI_SIDECAR_DIR) else {
        return;
    };
    let install = tempdir().unwrap();
    let location = resolve_sidecar_from_env("cloudflared", install.path(), "");
    assert_eq!(
        location,
        SidecarLocation::Bundled(PathBuf::from(env_dir).join("cloudflared"))
    );
    println!("sidecar-probe-env-override-ok");
}

#[test]
#[cfg(unix)]
fn env_override_wins() {
    let override_dir = tempdir().unwrap();
    make_executable(&override_dir.path().join("cloudflared"));

    let probe = std::env::current_exe().unwrap();
    let output = std::process::Command::new(probe)
        .args(["--nocapture", "env_override_probe"])
        .env(EDUCAI_SIDECAR_DIR, override_dir.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "env-override probe failed:\n{stderr}"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("sidecar-probe-env-override-ok"),
        "env-override probe did not run its assertion:\n{stdout}\n{stderr}"
    );
}

#[test]
#[cfg(unix)]
fn non_executable_install_dir_file_is_skipped() {
    let dir = tempdir().unwrap();
    make_non_executable(&dir.path().join("opencode"));
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), ""),
        SidecarLocation::OnPath("opencode".to_owned())
    );
}

#[test]
#[cfg(unix)]
fn suffixed_triple_fallback_when_plain_name_absent() {
    let dir = tempdir().unwrap();
    let suffixed = dir.path().join("opencode-x86_64-unknown-linux-gnu");
    make_executable(&suffixed);
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), ""),
        SidecarLocation::Bundled(suffixed)
    );
}

#[test]
#[cfg(unix)]
fn plain_name_wins_over_suffixed() {
    let dir = tempdir().unwrap();
    let plain = dir.path().join("opencode");
    make_executable(&plain);
    make_executable(&dir.path().join("opencode-x86_64-unknown-linux-gnu"));
    assert_eq!(
        resolve_sidecar("opencode", dir.path(), ""),
        SidecarLocation::Bundled(plain)
    );
}
