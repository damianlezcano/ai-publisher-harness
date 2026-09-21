//! Immutable, self-contained creation version snapshots.
//!
//! Each logical interactive resource is a lineage of complete version
//! directories. A new version is built as
//! `outputs/.staging-<version-id>/` by recursively copying the full base
//! version and overlaying only the changed/new files, then atomically renamed
//! to `outputs/<version-id>/`. The previous version is never modified.
//!
//! A crash before promotion leaves a recoverable `.staging-*` directory that
//! is never referenced by metadata and therefore never becomes current. A crash
//! after promotion but before the metadata commit can leave an orphan immutable
//! version directory; it is never current either, because the current pointer
//! lives in `project.json` and is only updated after the snapshot is complete.

#![forbid(unsafe_code)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use project_core::{
    CoreResult, Creation, CreationId, CreationVersionContent, ProjectCoreError, StoredCreation,
};

use crate::{reject_symlink_path, sync_parent_dir};

const STAGING_PREFIX: &str = ".staging-";

/// Builds one new immutable creation version directory and atomically promotes
/// it. `project_dir` and `outputs_dir` must be validated canonical roots.
///
/// The returned `StoredCreation` reports the primary entry's relative path and
/// its on-disk byte size after promotion.
pub(crate) fn build_version_snapshot(
    project_dir: &Path,
    outputs_dir: &Path,
    new_id: &CreationId,
    base: Option<&Creation>,
    content: &CreationVersionContent,
) -> CoreResult<StoredCreation> {
    let primary = validate_relative_path(&content.primary_relative_path)?;
    let mut overlays: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    for overlay in &content.overlays {
        let segments = validate_relative_path(&overlay.relative_path)?;
        overlays.push((segments, overlay.bytes.clone()));
    }

    let staging = outputs_dir.join(format!("{STAGING_PREFIX}{}", new_id.as_str()));
    if staging.exists() {
        return Err(ProjectCoreError::OperationFailed {
            operation: "version snapshot",
        });
    }

    fs::create_dir(&staging).map_err(|_| ProjectCoreError::WriteFailed)?;
    let result = (|| -> CoreResult<()> {
        if let Some(base) = base {
            let base_dir = outputs_dir.join(base.id.as_str());
            validate_version_source_dir(&base_dir)?;
            copy_regular_tree(&base_dir, &staging)?;
        }
        for (segments, bytes) in &overlays {
            write_overlay(&staging, segments, bytes)?;
        }
        validate_snapshot_tree(&staging)?;
        fsync_tree(&staging)?;
        Ok(())
    })();

    match result {
        Ok(()) => {}
        Err(err) => {
            let _ = remove_owned_dir(&staging);
            return Err(err);
        }
    }

    let target = outputs_dir.join(new_id.as_str());
    if target.exists() {
        let _ = remove_owned_dir(&staging);
        return Err(ProjectCoreError::WriteFailed);
    }
    fs::rename(&staging, &target).map_err(|_| {
        let _ = remove_owned_dir(&staging);
        ProjectCoreError::AtomicWriteFailed
    })?;
    sync_parent_dir(outputs_dir)?;

    let canon_outputs = fs::canonicalize(outputs_dir).map_err(|_| ProjectCoreError::WriteFailed)?;
    let canon_target = fs::canonicalize(&target).map_err(|_| ProjectCoreError::WriteFailed)?;
    if !canon_target.starts_with(&canon_outputs) {
        return Err(ProjectCoreError::PathEscape);
    }
    reject_symlink_path(&target, project_dir)?;

    let primary_file = segments_path(&target, &primary);
    let meta = fs::symlink_metadata(&primary_file).map_err(|_| ProjectCoreError::WriteFailed)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(ProjectCoreError::WriteFailed);
    }
    let relative = format!(
        "outputs/{}/{}",
        new_id.as_str(),
        content.primary_relative_path
    );
    let relative = project_core::RelativeProjectPath::parse(relative)?;
    Ok(StoredCreation {
        relative_path: relative,
        byte_size: meta.len(),
    })
}

/// Removes stale `outputs/.staging-*` directories for a project. Staging
/// directories are never referenced by metadata, so removing them is always
/// safe. Already-promoted version directories are never touched.
pub(crate) fn recover_stale_staging(project_dir: &Path, outputs_dir: &Path) -> CoreResult<()> {
    for entry in fs::read_dir(outputs_dir).map_err(|_| ProjectCoreError::WriteFailed)? {
        let entry = entry.map_err(|_| ProjectCoreError::WriteFailed)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with(STAGING_PREFIX) {
            continue;
        }
        let suffix = &name[STAGING_PREFIX.len()..];
        if CreationId::parse(suffix.to_owned()).is_err() {
            // Unknown staging name: leave it, never guess.
            continue;
        }
        remove_owned_dir(&entry.path())?;
    }
    let _ = project_dir;
    Ok(())
}

/// Validate a forward-slash, version-relative path into its safe components.
/// Rejects absolute paths, backslashes, traversal, empty segments, hidden
/// components, control/NUL bytes, and Windows-reserved stems.
fn validate_relative_path(relative: &str) -> CoreResult<Vec<String>> {
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.bytes().any(|b| b == 0 || b.is_ascii_control())
    {
        return Err(ProjectCoreError::InvalidPath(relative.into()));
    }
    let mut segments = Vec::new();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." || segment.starts_with('.') {
            return Err(ProjectCoreError::InvalidPath(relative.into()));
        }
        if is_windows_reserved_stem(segment) {
            return Err(ProjectCoreError::InvalidPath(relative.into()));
        }
        segments.push(segment.to_owned());
    }
    Ok(segments)
}

fn validate_version_source_dir(dir: &Path) -> CoreResult<()> {
    let meta = fs::symlink_metadata(dir).map_err(|_| ProjectCoreError::SourceUnreadable)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    validate_snapshot_tree(dir)
}

/// Recursively copy an existing version tree (base snapshot) into the staging
/// directory, rejecting symlinks, non-regular entries, and unsafe component
/// names.
fn copy_regular_tree(source: &Path, destination: &Path) -> CoreResult<()> {
    for entry in fs::read_dir(source).map_err(|_| ProjectCoreError::SourceUnreadable)? {
        let entry = entry.map_err(|_| ProjectCoreError::SourceUnreadable)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_component(&name)?;
        let from = entry.path();
        let to = destination.join(&name);
        let meta = fs::symlink_metadata(&from).map_err(|_| ProjectCoreError::SourceUnreadable)?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if meta.is_dir() {
            fs::create_dir(&to).map_err(|_| ProjectCoreError::WriteFailed)?;
            copy_regular_tree(&from, &to)?;
        } else if meta.is_file() {
            copy_file(&from, &to)?;
        } else {
            return Err(ProjectCoreError::SourceUnreadable);
        }
    }
    Ok(())
}

fn write_overlay(staging: &Path, segments: &[String], bytes: &[u8]) -> CoreResult<()> {
    let mut target = staging.to_path_buf();
    for (idx, segment) in segments.iter().enumerate() {
        target.push(segment);
        if idx < segments.len() - 1 {
            ensure_dir(&target)?;
        }
    }
    write_file(&target, bytes)
}

/// Ensure an intermediate overlay directory exists, creating it only when
/// missing. A directory already present in the copied base version is left
/// untouched (nested overlays like `css/estilos.css` overlay into an existing
/// `css/`). Any symlink or non-directory at that path is rejected.
fn ensure_dir(path: &Path) -> CoreResult<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.is_dir() {
                return Err(ProjectCoreError::SymlinkRejected);
            }
            Ok(())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|_| ProjectCoreError::WriteFailed)
        }
        Err(_) => Err(ProjectCoreError::WriteFailed),
    }
}

fn segments_path(root: &Path, segments: &[String]) -> PathBuf {
    let mut target = root.to_path_buf();
    for segment in segments {
        target.push(segment);
    }
    target
}

/// Validate a fully staged snapshot: every entry is a regular file or
/// directory, never a symlink, with safe component names.
fn validate_snapshot_tree(path: &Path) -> CoreResult<()> {
    for entry in fs::read_dir(path).map_err(|_| ProjectCoreError::WriteFailed)? {
        let entry = entry.map_err(|_| ProjectCoreError::WriteFailed)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_component(&name)?;
        let child = entry.path();
        let meta = fs::symlink_metadata(&child).map_err(|_| ProjectCoreError::WriteFailed)?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if meta.is_dir() {
            validate_snapshot_tree(&child)?;
        } else if !meta.is_file() {
            return Err(ProjectCoreError::WriteFailed);
        }
    }
    Ok(())
}

fn validate_component(name: &str) -> CoreResult<()> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || name.bytes().any(|b| b.is_ascii_control())
        || is_windows_reserved_stem(name)
    {
        Err(ProjectCoreError::InvalidName(name.into()))
    } else {
        Ok(())
    }
}

fn is_windows_reserved_stem(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

fn copy_file(source: &Path, destination: &Path) -> CoreResult<()> {
    let bytes = fs::read(source).map_err(|_| ProjectCoreError::SourceUnreadable)?;
    write_file(destination, &bytes)
}

fn write_file(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let mut file = fs::File::create(path).map_err(|_| ProjectCoreError::WriteFailed)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ProjectCoreError::WriteFailed)
}

fn fsync_tree(path: &Path) -> CoreResult<()> {
    for entry in fs::read_dir(path).map_err(|_| ProjectCoreError::WriteFailed)? {
        let p = entry.map_err(|_| ProjectCoreError::WriteFailed)?.path();
        if p.is_dir() {
            fsync_tree(&p)?;
        } else {
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&p)
                .and_then(|f| f.sync_all())
                .map_err(|_| ProjectCoreError::WriteFailed)?;
        }
    }
    sync_parent_dir(path)
}

fn remove_owned_dir(path: &Path) -> CoreResult<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| ProjectCoreError::OperationFailed {
        operation: "recover",
    })?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(ProjectCoreError::OperationFailed {
            operation: "recover",
        });
    }
    fs::remove_dir_all(path).map_err(|_| ProjectCoreError::OperationFailed {
        operation: "recover",
    })
}
