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
    CreateCreation, CreationContent, CreationKind, CreationVisibility, ProjectId, ProjectService,
    SystemClock, UuidV7IdGenerator, safe_file_name,
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
        let display_name = web_display_name(&artifact.path).unwrap_or_else(|| {
            let stem = std::path::Path::new(file_name)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or(file_name);
            safe_file_name(stem)
        });
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
        let mut service = self
            .service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let created = service
            .create_creation(&pid, request)
            .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        drop(service);
        if kind == CreationKind::Web {
            copy_web_sidecars(
                &self.base,
                project_id,
                created.id.as_str(),
                &artifact.path,
                file_name,
            )?;
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
    copy_tree_sidecars(source_dir, &dest_dir, source_dir, primary_file_name)
}

fn copy_tree_sidecars(
    source_root: &Path,
    dest_root: &Path,
    current: &Path,
    primary_file_name: &str,
) -> AgentResult<()> {
    let entries = match fs::read_dir(current) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries.flatten() {
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
        if name_str.starts_with('.') || name_str == "materials" {
            continue;
        }
        if meta.is_dir() {
            let dest = dest_root.join(path.strip_prefix(source_root).unwrap_or(&path));
            fs::create_dir_all(&dest)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
            copy_tree_sidecars(source_root, dest_root, &path, primary_file_name)?;
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if current == source_root && name_str == primary_file_name {
            continue;
        }
        if is_document_sidecar(&name_str) {
            continue;
        }
        let safe = safe_file_name(&name_str);
        if safe.is_empty() {
            continue;
        }
        let relative = path.strip_prefix(source_root).unwrap_or(&path);
        let mut dest = dest_root.to_path_buf();
        for component in relative.iter().take_while(|_| true) {
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
        if dest.file_name().and_then(|n| n.to_str()) == Some("index.html") {
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        }
        let bytes =
            fs::read(&path).map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
        fs::write(&dest, bytes).map_err(|err| AgentError::RegistrationFailed(err.to_string()))?;
    }
    Ok(())
}
