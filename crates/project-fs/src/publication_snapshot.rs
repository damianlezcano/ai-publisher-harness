//! Validated, journaled publication snapshots for M3.
//!
//! This adapter receives already-persisted project metadata and never decides
//! visibility.  It only copies creations explicitly marked public.

use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use project_core::{
    CoreResult, Creation, CreationKind, CreationVisibility, Project, ProjectCoreError, ProjectId,
};
use project_publisher::PublishRoot;
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::{
    PROJECTS_DIR, ProjectPublishRootProvider, canon_project_dir, fsync_dir, reject_symlink_path,
};

const STAGING_PREFIX: &str = ".publish-staging-";
const PREVIOUS_PREFIX: &str = ".publish-previous-";
const JOURNAL_PREFIX: &str = ".publish-swap-";
const JOURNAL_SUFFIX: &str = ".json";
const RESERVED_ROOTS: &[&str] = &["index.html", "materials.html", "files"];

/// A successfully installed immutable `publish/` tree.
#[derive(Clone, Debug)]
pub struct PublicationSnapshot {
    project_id: ProjectId,
    publish_root: PublishRoot,
}

impl PublicationSnapshot {
    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }
    pub fn publish_root(&self) -> &PublishRoot {
        &self.publish_root
    }
}

/// Test-only deterministic failure points. They model failures at each
/// pre-registration swap step and ensure the old snapshot remains usable,
/// rolling back any move of the previous tree before it is observable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotFault {
    AfterStaging,
    AfterJournal,
    AfterRenamePrevious,
}

#[derive(Clone, Debug)]
pub struct PublicationSnapshotStore {
    base: PathBuf,
    fault: Option<SnapshotFault>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SwapJournal {
    operation_id: String,
    project_id: String,
    staging_name: String,
    previous_name: String,
}

impl PublicationSnapshotStore {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self {
            base: base.into(),
            fault: None,
        }
    }

    pub fn with_fault(base: impl Into<PathBuf>, fault: SnapshotFault) -> Self {
        Self {
            base: base.into(),
            fault: Some(fault),
        }
    }

    /// Builds, validates, and installs a fixed publication tree. Preparation
    /// never writes to the existing `publish/` directory.
    pub fn prepare(&self, project: &Project) -> CoreResult<PublicationSnapshot> {
        project.validate()?;
        self.recover(&project.id)?;
        let project_dir = self.project_dir(&project.id)?;
        let public: Vec<&Creation> = project
            .creations
            .iter()
            .filter(|creation| creation.visibility == CreationVisibility::Public)
            .collect();
        let web: Vec<&Creation> = public
            .iter()
            .copied()
            .filter(|creation| creation.kind == CreationKind::Web)
            .collect();
        if web.len() > 1 {
            return Err(ProjectCoreError::InvalidCreation(
                "more than one public web creation".into(),
            ));
        }

        let staging_temp = Builder::new()
            .prefix(STAGING_PREFIX)
            .tempdir_in(&project_dir)
            .map_err(|_| ProjectCoreError::WriteFailed)?;
        let staging = staging_temp.keep();
        let result = self
            .prepare_staging(project, &public, web.first().copied(), &staging)
            .and_then(|_| {
                if self.fault == Some(SnapshotFault::AfterStaging) {
                    return Err(ProjectCoreError::OperationFailed {
                        operation: "prepare",
                    });
                }
                self.install(project, &project_dir, &staging)
            });
        if result.is_err() && staging.exists() {
            let _ = remove_owned_dir(&staging);
        }
        result
    }

    /// Cleans only controlled snapshot transients. An ambiguous journal is
    /// deliberately left in place and fails closed.
    pub fn recover(&self, project_id: &ProjectId) -> CoreResult<()> {
        let project_dir = self.project_dir(project_id)?;
        let mut journals = Vec::new();
        for entry in fs::read_dir(&project_dir).map_err(|_| ProjectCoreError::StorageUnavailable)? {
            let entry = entry.map_err(|_| ProjectCoreError::StorageUnavailable)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(JOURNAL_PREFIX) && name.ends_with(JOURNAL_SUFFIX) {
                journals.push((name, entry.path()));
            }
        }
        if journals.len() > 1 {
            return Err(ProjectCoreError::OperationFailed {
                operation: "recover",
            });
        }
        if let Some((name, path)) = journals.pop() {
            let journal = read_journal(&path)?;
            validate_journal(&journal, &name, project_id)?;
            let staging = project_dir.join(&journal.staging_name);
            let previous = project_dir.join(&journal.previous_name);
            let publish = project_dir.join("publish");
            if publish.is_dir() && !is_symlink(&publish)? {
                validate_publish(&self.base, project_id)?;
                remove_if_present(&staging)?;
                remove_if_present(&previous)?;
            } else if !publish.exists() && previous.is_dir() && !is_symlink(&previous)? {
                remove_if_present(&staging)?;
                fs::rename(&previous, &publish).map_err(|_| ProjectCoreError::OperationFailed {
                    operation: "recover",
                })?;
                fsync_dir(&project_dir)?;
                validate_publish(&self.base, project_id)?;
            } else {
                return Err(ProjectCoreError::OperationFailed {
                    operation: "recover",
                });
            }
            fs::remove_file(path).map_err(|_| ProjectCoreError::OperationFailed {
                operation: "recover",
            })?;
            fsync_dir(&project_dir)?;
        }
        // A staging sibling has never been installed and can be safely removed.
        for entry in fs::read_dir(&project_dir).map_err(|_| ProjectCoreError::StorageUnavailable)? {
            let entry = entry.map_err(|_| ProjectCoreError::StorageUnavailable)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name
                .strip_prefix(STAGING_PREFIX)
                .is_some_and(valid_operation_id)
            {
                remove_owned_dir(&entry.path())?;
            }
        }
        validate_publish(&self.base, project_id).map(|_| ())
    }

    fn project_dir(&self, id: &ProjectId) -> CoreResult<PathBuf> {
        let project_dir = self.base.join(PROJECTS_DIR).join(id.as_str());
        reject_symlink_path(&project_dir, &self.base)?;
        let canonical = canon_project_dir(&self.base, id)?;
        if canonical != project_dir {
            return Err(ProjectCoreError::PathEscape);
        }
        Ok(project_dir)
    }

    fn prepare_staging(
        &self,
        project: &Project,
        public: &[&Creation],
        web: Option<&Creation>,
        staging: &Path,
    ) -> CoreResult<()> {
        let project_dir = self.project_dir(&project.id)?;
        let mut materials = Vec::new();
        if let Some(web) = web {
            let source_root = creation_root(&project_dir, web)?;
            validate_source_tree(&source_root, true)?;
            let entry = declared_source(&source_root, web)?;
            if entry.file_name().and_then(|n| n.to_str()) != Some("index.html")
                || !is_regular(&entry)?
            {
                return Err(ProjectCoreError::InvalidCreation(
                    "web entry must be index.html".into(),
                ));
            }
            copy_tree(&source_root, staging, true)?;
        }
        for creation in public
            .iter()
            .copied()
            .filter(|c| c.kind != CreationKind::Web)
        {
            let source_root = creation_root(&project_dir, creation)?;
            validate_source_tree(&source_root, true)?;
            let source = declared_source(&source_root, creation)?;
            if !is_regular(&source)? {
                return Err(ProjectCoreError::SourceUnreadable);
            }
            let file_name = source
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or(ProjectCoreError::InvalidName("non-utf8 filename".into()))?;
            validate_component(file_name, false)?;
            let relative = PathBuf::from("files")
                .join(creation.id.as_str())
                .join(file_name);
            copy_file(&source, &staging.join(&relative))?;
            materials.push((creation, relative));
        }
        let html = landing_html(&materials);
        let landing = if web.is_some() && !materials.is_empty() {
            "materials.html"
        } else {
            "index.html"
        };
        if web.is_some() && materials.is_empty() { /* untrusted web owns index */
        } else {
            write_file(&staging.join(landing), html.as_bytes())?;
        }
        // Generated `index.html`, `materials.html`, and `files/` are valid
        // snapshot roots; source-copy validation above reserves them from an
        // untrusted web creation.
        validate_tree(staging, false)?;
        fsync_tree(staging)?;
        Ok(())
    }

    fn install(
        &self,
        project: &Project,
        project_dir: &Path,
        staging: &Path,
    ) -> CoreResult<PublicationSnapshot> {
        let staging_name = staging
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(ProjectCoreError::WriteFailed)?
            .to_owned();
        let operation_id = staging_name
            .strip_prefix(STAGING_PREFIX)
            .filter(|s| valid_operation_id(s))
            .ok_or(ProjectCoreError::WriteFailed)?;
        let previous_name = format!("{PREVIOUS_PREFIX}{operation_id}");
        let journal_name = format!("{JOURNAL_PREFIX}{operation_id}{JOURNAL_SUFFIX}");
        let journal_path = project_dir.join(&journal_name);
        let journal = SwapJournal {
            operation_id: operation_id.into(),
            project_id: project.id.as_str().into(),
            staging_name,
            previous_name: previous_name.clone(),
        };
        write_file(
            &journal_path,
            serde_json::to_vec(&journal)
                .map_err(|_| ProjectCoreError::WriteFailed)?
                .as_slice(),
        )?;
        fsync_dir(project_dir)?;
        if self.fault == Some(SnapshotFault::AfterJournal) {
            let _ = fs::remove_file(&journal_path);
            return Err(ProjectCoreError::OperationFailed { operation: "swap" });
        }
        let publish = project_dir.join("publish");
        let previous = project_dir.join(&previous_name);
        for entry in fs::read_dir(project_dir)
            .map_err(|_| ProjectCoreError::OperationFailed { operation: "swap" })?
        {
            let entry =
                entry.map_err(|_| ProjectCoreError::OperationFailed { operation: "swap" })?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(PREVIOUS_PREFIX) {
                remove_owned_dir(&entry.path())?;
            }
        }
        if publish.exists() {
            fs::rename(&publish, &previous)
                .map_err(|_| ProjectCoreError::OperationFailed { operation: "swap" })?;
        }
        if self.fault == Some(SnapshotFault::AfterRenamePrevious) {
            if !publish.exists() && previous.exists() {
                let _ = fs::rename(&previous, &publish);
            }
            let _ = fs::remove_file(&journal_path);
            return Err(ProjectCoreError::OperationFailed { operation: "swap" });
        }
        if fs::rename(staging, &publish).is_err() {
            if !publish.exists() && previous.exists() {
                let _ = fs::rename(&previous, &publish);
            }
            return Err(ProjectCoreError::OperationFailed { operation: "swap" });
        }
        fsync_dir(project_dir)?;
        let root = validate_publish(&self.base, &project.id)?;
        fs::remove_file(&journal_path)
            .map_err(|_| ProjectCoreError::OperationFailed { operation: "swap" })?;
        fsync_dir(project_dir)?;
        Ok(PublicationSnapshot {
            project_id: project.id.clone(),
            publish_root: root,
        })
    }
}

fn creation_root(project_dir: &Path, creation: &Creation) -> CoreResult<PathBuf> {
    let root = project_dir.join("outputs").join(creation.id.as_str());
    if is_symlink(&root)? || !root.is_dir() {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    Ok(root)
}
fn declared_source(root: &Path, creation: &Creation) -> CoreResult<PathBuf> {
    let expected = format!("outputs/{}/", creation.id.as_str());
    let rest = creation
        .relative_path
        .as_str()
        .strip_prefix(&expected)
        .ok_or(ProjectCoreError::PathEscape)?;
    let relative = Path::new(rest);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(ProjectCoreError::PathEscape);
    }
    let source = root.join(relative);
    reject_symlink_path(&source, root)?;
    Ok(source)
}
fn copy_tree(source: &Path, destination: &Path, root: bool) -> CoreResult<()> {
    if is_symlink(source)? {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    for entry in fs::read_dir(source).map_err(|_| ProjectCoreError::SourceUnreadable)? {
        let entry = entry.map_err(|_| ProjectCoreError::SourceUnreadable)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_component(&name, root)?;
        let from = entry.path();
        let to = destination.join(&name);
        let meta = fs::symlink_metadata(&from).map_err(|_| ProjectCoreError::SourceUnreadable)?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if meta.is_dir() {
            fs::create_dir_all(&to).map_err(|_| ProjectCoreError::WriteFailed)?;
            copy_tree(&from, &to, false)?;
        } else if meta.is_file() {
            copy_file(&from, &to)?;
        } else {
            return Err(ProjectCoreError::SourceUnreadable);
        }
    }
    Ok(())
}

fn validate_source_tree(path: &Path, root: bool) -> CoreResult<()> {
    if is_symlink(path)? || !path.is_dir() {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    for entry in fs::read_dir(path).map_err(|_| ProjectCoreError::SourceUnreadable)? {
        let entry = entry.map_err(|_| ProjectCoreError::SourceUnreadable)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_component(&name, root)?;
        let child = entry.path();
        let meta = fs::symlink_metadata(&child).map_err(|_| ProjectCoreError::SourceUnreadable)?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if meta.is_dir() {
            validate_source_tree(&child, false)?;
        } else if !meta.is_file() {
            return Err(ProjectCoreError::SourceUnreadable);
        }
    }
    Ok(())
}
fn copy_file(source: &Path, destination: &Path) -> CoreResult<()> {
    if is_symlink(source)? || !is_regular(source)? {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    let bytes = fs::read(source).map_err(|_| ProjectCoreError::SourceUnreadable)?;
    if is_symlink(source)? || !is_regular(source)? {
        return Err(ProjectCoreError::SymlinkRejected);
    }
    write_file(destination, &bytes)
}
fn write_file(path: &Path, bytes: &[u8]) -> CoreResult<()> {
    let parent = path.parent().ok_or(ProjectCoreError::WriteFailed)?;
    fs::create_dir_all(parent).map_err(|_| ProjectCoreError::WriteFailed)?;
    let mut file = fs::File::create(path).map_err(|_| ProjectCoreError::WriteFailed)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| ProjectCoreError::WriteFailed)
}
fn validate_tree(path: &Path, root: bool) -> CoreResult<()> {
    for entry in fs::read_dir(path).map_err(|_| ProjectCoreError::WriteFailed)? {
        let entry = entry.map_err(|_| ProjectCoreError::WriteFailed)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        validate_component(&name, root)?;
        let child = entry.path();
        let meta = fs::symlink_metadata(&child).map_err(|_| ProjectCoreError::WriteFailed)?;
        if meta.file_type().is_symlink() {
            return Err(ProjectCoreError::SymlinkRejected);
        }
        if meta.is_dir() {
            validate_tree(&child, false)?;
        } else if !meta.is_file() {
            return Err(ProjectCoreError::WriteFailed);
        }
    }
    Ok(())
}
fn fsync_tree(path: &Path) -> CoreResult<()> {
    for entry in fs::read_dir(path).map_err(|_| ProjectCoreError::WriteFailed)? {
        let p = entry.map_err(|_| ProjectCoreError::WriteFailed)?.path();
        if p.is_dir() {
            fsync_tree(&p)?;
        } else {
            fs::File::open(&p)
                .and_then(|f| f.sync_all())
                .map_err(|_| ProjectCoreError::WriteFailed)?;
        }
    }
    fsync_dir(path)
}
fn validate_component(name: &str, root: bool) -> CoreResult<()> {
    if name.is_empty()
        || name.starts_with('.')
        || name.contains(['/', '\\', '\0'])
        || is_windows_reserved(name)
        || (root && name != "index.html" && RESERVED_ROOTS.contains(&name))
    {
        Err(ProjectCoreError::InvalidName(name.into()))
    } else {
        Ok(())
    }
}
fn is_windows_reserved(name: &str) -> bool {
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
fn is_symlink(path: &Path) -> CoreResult<bool> {
    Ok(fs::symlink_metadata(path)
        .map_err(|_| ProjectCoreError::SourceUnreadable)?
        .file_type()
        .is_symlink())
}
fn is_regular(path: &Path) -> CoreResult<bool> {
    Ok(fs::symlink_metadata(path)
        .map_err(|_| ProjectCoreError::SourceUnreadable)?
        .is_file())
}
fn landing_html(materials: &[(&Creation, PathBuf)]) -> String {
    let mut entries = materials.to_vec();
    entries.sort_by(|(a, _), (b, _)| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Material del proyecto</title></head><body><h1>Material del proyecto</h1><ul>",
    );
    for (creation, path) in entries {
        let href = escape_html(&path.to_string_lossy());
        let name = escape_html(&creation.display_name);
        html.push_str("<li>");
        html.push_str(&name);
        html.push_str(" — ");
        if creation
            .content_type
            .as_ref()
            .is_some_and(|c| c.as_str() == "application/pdf")
        {
            html.push_str(&format!("<a href=\"{href}\">Abrir</a> / "));
        }
        html.push_str(&format!("<a href=\"{href}\" download>Descargar</a></li>"));
    }
    html.push_str("</ul></body></html>");
    html
}
fn escape_html(value: &str) -> String {
    value
        .chars()
        .flat_map(|c| match c {
            '&' => "&amp;".chars().collect::<Vec<_>>(),
            '<' => "&lt;".chars().collect(),
            '>' => "&gt;".chars().collect(),
            '\"' => "&quot;".chars().collect(),
            '\'' => "&#39;".chars().collect(),
            _ => vec![c],
        })
        .collect()
}
fn read_journal(path: &Path) -> CoreResult<SwapJournal> {
    let bytes = fs::read(path).map_err(|_| ProjectCoreError::OperationFailed {
        operation: "recover",
    })?;
    serde_json::from_slice(&bytes).map_err(|_| ProjectCoreError::OperationFailed {
        operation: "recover",
    })
}
fn validate_journal(
    journal: &SwapJournal,
    file_name: &str,
    project_id: &ProjectId,
) -> CoreResult<()> {
    if !valid_operation_id(&journal.operation_id)
        || journal.project_id != project_id.as_str()
        || journal.staging_name != format!("{STAGING_PREFIX}{}", journal.operation_id)
        || journal.previous_name != format!("{PREVIOUS_PREFIX}{}", journal.operation_id)
        || file_name != format!("{JOURNAL_PREFIX}{}{JOURNAL_SUFFIX}", journal.operation_id)
    {
        return Err(ProjectCoreError::OperationFailed {
            operation: "recover",
        });
    }
    Ok(())
}
fn valid_operation_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn remove_if_present(path: &Path) -> CoreResult<()> {
    if path.exists() {
        remove_owned_dir(path)?;
    }
    Ok(())
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
fn validate_publish(base: &Path, id: &ProjectId) -> CoreResult<PublishRoot> {
    ProjectPublishRootProvider::new(base).publish_root(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_core::{CreationId, RelativeProjectPath, Timestamp};

    fn creation(display_name: &str) -> Creation {
        Creation {
            id: CreationId::parse("0198e4a6-86d6-7c16-b4c4-3197b3550001").unwrap(),
            display_name: display_name.into(),
            kind: CreationKind::Document,
            visibility: CreationVisibility::Public,
            relative_path: RelativeProjectPath::parse(
                "outputs/0198e4a6-86d6-7c16-b4c4-3197b3550001/notes.pdf",
            )
            .unwrap(),
            content_type: None,
            byte_size: 1,
            revision: 1,
            parent_creation_id: None,
            created_at: Timestamp::parse("2026-08-29T00:00:00Z").unwrap(),
        }
    }

    #[test]
    fn landing_html_escapes_href_path_attribute() {
        let c = creation("Doc");
        let hostile = PathBuf::from("files").join(c.id.as_str()).join("a\"b.pdf");
        let html = landing_html(&[(&c, hostile)]);
        assert!(html.contains("a&quot;b.pdf"));
        assert!(!html.contains("a\"b.pdf\""));
    }
}
