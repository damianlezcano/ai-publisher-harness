//! Registers single-file agent artifacts as private Creations.
//!
//! Multi-file web directories (`index.html` plus sibling assets) are deferred:
//! M5 only promotes a single file (including a lone `index.html` as `Web`).

use std::path::PathBuf;
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
            FilesystemProjectContentStore::new(base),
            SystemClock,
            UuidV7IdGenerator,
        );
        Self {
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
        let stem = std::path::Path::new(file_name)
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or(file_name);
        let request = CreateCreation {
            display_name: safe_file_name(stem),
            kind: creation_kind(artifact.kind),
            visibility: CreationVisibility::Private,
            content_type: None,
            content: CreationContent {
                bytes,
                file_name: safe_file_name(file_name),
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
