//! Registers agent artifacts as private Creations.
//!
//! A web artifact (`index.html` or any `.html`) is stored as the generic
//! `index.html` publication entry. Sibling assets in the same workspace
//! directory (CSS/JS/images, not documents) are copied into the creation
//! output so Abrir/Compartir serve the same interactive artifact.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use project_core::{
    CreateCreation, CreationContent, CreationId, CreationKind, CreationVisibility, ProjectId,
    ProjectService, SystemClock, UuidV7IdGenerator, safe_file_name,
};
use project_fs::{FilesystemProjectContentStore, FilesystemProjectRepository};

use crate::AgentResult;
use crate::error::AgentError;
use crate::model::{Artifact, ArtifactKind};

pub trait CreationRegistrar: Send + Sync {
    /// Registers a single-file artifact as a PRIVATE Creation. `artifact.path` is
    /// workspace-relative. Returns the new creation id.
    fn register(
        &self,
        project_id: &str,
        artifact: &Artifact,
        bytes: Vec<u8>,
    ) -> AgentResult<String>;
}

pub struct FilesystemCreationRegistrar {
    base: PathBuf,
    service: Mutex<
        ProjectService<
            FilesystemProjectRepository,
            FilesystemProjectContentStore,
            SystemClock,
            UuidV7IdGenerator,
        >,
    >,
}

impl FilesystemCreationRegistrar {
    pub fn new(base: PathBuf) -> Self {
        let service = ProjectService::new(
            FilesystemProjectRepository::new(base.clone()),
            FilesystemProjectContentStore::new(base.clone()),
            SystemClock,
            UuidV7IdGenerator,
        );
        Self {
            base,
            service: Mutex::new(service),
        }
    }
}

impl CreationRegistrar for FilesystemCreationRegistrar {
    fn register(
        &self,
        project_id: &str,
        artifact: &Artifact,
        bytes: Vec<u8>,
    ) -> AgentResult<String> {
        let pid = ProjectId::parse(project_id)
            .map_err(|_| AgentError::SessionNotFound(project_id.to_owned()))?;
        let file_name = std::path::Path::new(&artifact.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let kind = creation_kind(artifact.kind);
        let stored_file_name = if kind == CreationKind::Web {
            "index.html".to_owned()
        } else {
            safe_file_name(file_name)
        };
        let display_name = web_display_name(&artifact.path)
            .unwrap_or_else(|| fallback_display_name(file_name, kind));
        let mut service = self
            .service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let existing_id: Option<CreationId> =
            service.list_creations(&pid).ok().and_then(|creations| {
                creations
                    .into_iter()
                    .rev()
                    .find(|c| c.kind == kind && c.display_name == display_name)
                    .map(|c| c.id)
            });
        let created = if let Some(id) = existing_id {
            let updated = service
                .replace_creation_content(
                    &pid,
                    &id,
                    CreationContent {
                        bytes,
                        file_name: stored_file_name.clone(),
                    },
                )
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
            // Prune leftovers only after the new bytes are stored so a CAS
            // reject cannot leave project.json pointing at an empty tree.
            prune_stale_creation_outputs(&self.base, project_id, id.as_str(), &stored_file_name)?;
            updated
        } else {
            let request = CreateCreation {
                display_name,
                kind,
                visibility: CreationVisibility::Private,
                content_type: None,
                content: CreationContent {
                    bytes,
                    file_name: stored_file_name,
                },
                parent_creation_id: None,
            };
            service
                .create_creation(&pid, request)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?
        };
        drop(service);
        if kind == CreationKind::Web {
            // Best-effort: the primary file is already a Creation. A sidecar
            // copy failure must not hide it from Abrir.
            let _ = copy_web_sidecars(
                &self.base,
                project_id,
                created.id.as_str(),
                &artifact.path,
                file_name,
            );
        }
        Ok(created.id.as_str().to_owned())
    }
}

fn creation_kind(kind: ArtifactKind) -> CreationKind {
    match kind {
        ArtifactKind::Web => CreationKind::Web,
        ArtifactKind::Document => CreationKind::Document,
        ArtifactKind::Image => CreationKind::Image,
        ArtifactKind::Spreadsheet
        | ArtifactKind::Presentation
        | ArtifactKind::Pdf
        | ArtifactKind::Text
        | ArtifactKind::Other => CreationKind::File,
    }
}

const DEFAULT_WEB_DISPLAY_NAME: &str = "Actividad";
const SKIP_SIDECAR_DIR_NAMES: &[&str] = &[
    "materials",
    "node_modules",
    "dist",
    "build",
    "target",
    "vendor",
    "venv",
    "__pycache__",
    "coverage",
    "bower_components",
];
const RESERVED_ROOT_SIDECARS: &[&str] = &["materials.html", "files"];
const MAX_SIDECAR_DEPTH: usize = 8;
const MAX_SIDECAR_FILES: usize = 500;
const MAX_SIDECAR_BYTES: u64 = 32 * 1024 * 1024;

fn web_display_name(artifact_path: &str) -> Option<String> {
    let relative = artifact_path
        .replace('\\', "/")
        .trim_start_matches('/')
        .strip_prefix("workspace/")
        .unwrap_or(artifact_path)
        .to_owned();
    let parent = Path::new(&relative)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty() && *n != "." && *n != "workspace")?;
    Some(safe_file_name(parent))
}

fn fallback_display_name(file_name: &str, kind: CreationKind) -> String {
    let stem = Path::new(file_name)
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or(file_name);
    if kind == CreationKind::Web && stem.eq_ignore_ascii_case("index") {
        return DEFAULT_WEB_DISPLAY_NAME.to_owned();
    }
    safe_file_name(stem)
}

fn prune_stale_creation_outputs(
    base: &Path,
    project_id: &str,
    creation_id: &str,
    keep_file_name: &str,
) -> AgentResult<()> {
    if creation_id.is_empty()
        || creation_id.contains(['/', '\\', '\0'])
        || creation_id == "."
        || creation_id == ".."
    {
        return Err(AgentError::RegistrationFailed("unsafe creation id".into()));
    }
    let dest = base
        .join("projects")
        .join(project_id)
        .join("outputs")
        .join(creation_id);
    if !dest.is_dir() {
        return Ok(());
    }
    let entries = match fs::read_dir(&dest) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some(keep_file_name) {
            continue;
        }
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            let _ = fs::remove_file(&path);
            continue;
        }
        if meta.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

fn is_document_sidecar(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "pdf" | "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "doc" | "xls" | "ppt"
    )
}

fn copy_web_sidecars(
    base: &Path,
    project_id: &str,
    creation_id: &str,
    artifact_path: &str,
    primary_file_name: &str,
) -> AgentResult<()> {
    let relative = artifact_path.replace('\\', "/");
    let relative = relative.trim_start_matches('/');
    let relative = relative.strip_prefix("workspace/").unwrap_or(relative);
    if relative.is_empty()
        || Path::new(relative).is_absolute()
        || relative
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(AgentError::RegistrationFailed("unsafe sidecar path".into()));
    }
    let source_file = base
        .join("projects")
        .join(project_id)
        .join("workspace")
        .join(relative);
    let source_dir = source_file.parent().ok_or_else(|| {
        AgentError::RegistrationFailed("web artifact has no parent directory".into())
    })?;
    if !source_dir.is_dir() {
        return Ok(());
    }
    let dest_dir = base
        .join("projects")
        .join(project_id)
        .join("outputs")
        .join(creation_id);
    let mut limits = SidecarCopyLimits::default();
    copy_tree_sidecars(
        source_dir,
        &dest_dir,
        source_dir,
        primary_file_name,
        0,
        &mut limits,
    )
}

#[derive(Default)]
struct SidecarCopyLimits {
    files: usize,
    bytes: u64,
}

fn is_skipped_sidecar_dir(name: &str) -> bool {
    SKIP_SIDECAR_DIR_NAMES
        .iter()
        .any(|skip| name.eq_ignore_ascii_case(skip))
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

/// Mirrors `project-fs` snapshot `validate_component` so a copied sidecar cannot
/// make Compartir reject the bundle.
fn sidecar_component_ok(name: &str, at_root: bool) -> bool {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', '\0']) {
        return false;
    }
    if is_windows_reserved_stem(name) {
        return false;
    }
    if at_root && RESERVED_ROOT_SIDECARS.contains(&name) {
        return false;
    }
    true
}

fn copy_tree_sidecars(
    source_root: &Path,
    dest_root: &Path,
    current: &Path,
    primary_file_name: &str,
    depth: usize,
    limits: &mut SidecarCopyLimits,
) -> AgentResult<()> {
    if depth > MAX_SIDECAR_DEPTH
        || limits.files >= MAX_SIDECAR_FILES
        || limits.bytes >= MAX_SIDECAR_BYTES
    {
        return Ok(());
    }
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
        if limits.files >= MAX_SIDECAR_FILES || limits.bytes >= MAX_SIDECAR_BYTES {
            return Ok(());
        }
        let path = entry.path();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let at_root = current == source_root;
        if meta.is_dir() {
            if is_skipped_sidecar_dir(&name_str) {
                continue;
            }
            if !sidecar_component_ok(&name_str, at_root) {
                continue;
            }
            copy_tree_sidecars(
                source_root,
                dest_root,
                &path,
                primary_file_name,
                depth + 1,
                limits,
            )?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if at_root && name_str == primary_file_name {
            continue;
        }
        if is_document_sidecar(&name_str) {
            continue;
        }
        let safe = safe_file_name(&name_str);
        if safe.is_empty() || !sidecar_component_ok(&safe, at_root) {
            continue;
        }
        let relative = path.strip_prefix(source_root).unwrap_or(&path);
        let mut dest = dest_root.to_path_buf();
        for component in relative.iter() {
            let part = component.to_string_lossy();
            if part == ".." || part == "." {
                return Err(AgentError::RegistrationFailed("unsafe sidecar path".into()));
            }
        }
        let parent_rel = relative.parent().unwrap_or_else(|| Path::new(""));
        if !parent_rel.as_os_str().is_empty() {
            dest.push(parent_rel);
        }
        dest.push(&safe);
        // Root `index.html` is the primary stored file; nested copies must stay.
        if dest == dest_root.join("index.html") {
            continue;
        }
        if limits.files >= MAX_SIDECAR_FILES
            || limits.bytes.saturating_add(meta.len()) > MAX_SIDECAR_BYTES
        {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        }
        let bytes =
            fs::read(&path).map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        fs::write(&dest, bytes).map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        limits.files += 1;
        limits.bytes = limits.bytes.saturating_add(meta.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use project_core::CreationKind;
    use std::fs;

    #[test]
    fn root_index_html_uses_human_display_name() {
        assert_eq!(web_display_name("workspace/index.html"), None);
        assert_eq!(
            fallback_display_name("index.html", CreationKind::Web),
            "Actividad"
        );
        assert_eq!(
            fallback_display_name("index.htm", CreationKind::Web),
            "Actividad"
        );
        assert_eq!(
            web_display_name("workspace/actividad-2/index.html").as_deref(),
            Some("actividad-2")
        );
    }

    #[test]
    fn copy_skips_unpublishable_roots_and_keeps_nested_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let source = tmp.path().join("src");
        let dest = tmp.path().join("dest");
        fs::create_dir_all(source.join("slides")).expect("slides");
        fs::create_dir_all(source.join("node_modules/pkg")).expect("deps");
        fs::create_dir_all(source.join("files")).expect("files");
        fs::write(source.join("index.html"), b"<h1>root</h1>").expect("primary");
        fs::write(source.join("app.js"), b"console.log(1)").expect("js");
        fs::write(source.join("slides/index.html"), b"<h1>slide</h1>").expect("nested");
        fs::write(source.join("materials.html"), b"nope").expect("reserved");
        fs::write(source.join("aux.js"), b"reserved").expect("aux");
        fs::write(source.join("files/secret.txt"), b"nope").expect("files child");
        fs::write(source.join("node_modules/pkg/index.js"), b"dep").expect("dep");

        let mut limits = SidecarCopyLimits::default();
        copy_tree_sidecars(&source, &dest, &source, "index.html", 0, &mut limits).expect("copy");

        assert_eq!(
            fs::read_to_string(dest.join("app.js")).expect("js"),
            "console.log(1)"
        );
        assert_eq!(
            fs::read_to_string(dest.join("slides/index.html")).expect("nested"),
            "<h1>slide</h1>"
        );
        assert!(!dest.join("index.html").exists());
        assert!(!dest.join("materials.html").exists());
        assert!(!dest.join("aux.js").exists());
        assert!(!dest.join("files").exists());
        assert!(!dest.join("node_modules").exists());
    }

    #[test]
    fn prune_after_replace_keeps_the_new_primary_and_drops_stale_sidecars() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let creation_id = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
        let dest = tmp
            .path()
            .join("projects")
            .join("proj")
            .join("outputs")
            .join(creation_id);
        fs::create_dir_all(&dest).expect("outputs");
        fs::write(dest.join("index.html"), b"new").expect("primary");
        fs::write(dest.join("stale.css"), b"old").expect("stale");
        fs::create_dir_all(dest.join("old-assets")).expect("stale dir");
        fs::write(dest.join("old-assets/x.js"), b"x").expect("stale nested");

        prune_stale_creation_outputs(tmp.path(), "proj", creation_id, "index.html").expect("prune");

        assert_eq!(
            fs::read_to_string(dest.join("index.html")).expect("kept"),
            "new"
        );
        assert!(!dest.join("stale.css").exists());
        assert!(!dest.join("old-assets").exists());
    }
}
