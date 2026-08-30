//! Application facade: the Tauri-free application core that wires M1-M5.
//!
//! `AppState` composes `ProjectService` (project/material/creation CRUD), the
//! `PublicationManager` (publish/unpublish), and `AgentService` (agent tasks),
//! and exposes high-level, UI-oriented operations returning serializable DTOs
//! plus human-facing errors. The Tauri command layer is a thin adapter over
//! this facade; no domain logic lives in the frontend.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use project_agent::model::TaskStatus;
use project_agent::{
    AgentEngine, AgentPrompt, AgentRequest, AgentRunResult, AgentService, AgentStatus, AgentTask,
    FilesystemCreationRegistrar, OpenCodeAgentEngine,
};
use project_core::{
    AddMaterial, ContentType, Creation, CreationId, CreationKind, CreationVisibility, Material,
    MaterialContent, ProjectId, ProjectService, SystemClock, UuidV7IdGenerator,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, ProjectPublishRootProvider,
    PublicationSnapshotStore,
};
use project_publication::{OsRouteEntropy, PublicationManager};
use project_publisher::AxumLocalPublisher;
use project_tunnel::{
    BinaryResolver, CloudflareQuickTunnel, FixedBinaryResolver, PathBinaryResolver, TunnelProvider,
};

use crate::dtos::*;
use crate::error::{AppError, AppResult, ErrorCode};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Production wiring inputs. Paths are resolved once at startup; the frontend
/// never supplies them.
pub struct AppConfig {
    pub data_dir: PathBuf,
    pub opencode_binary: PathBuf,
    pub opencode_config_dir: PathBuf,
    pub opencode_port: u16,
    pub cloudflared_binary: Option<PathBuf>,
}

pub struct AppState<E = OpenCodeAgentEngine, T = CloudflareQuickTunnel>
where
    E: AgentEngine,
    T: TunnelProvider,
{
    base: PathBuf,
    projects: Mutex<
        ProjectService<
            FilesystemProjectRepository,
            FilesystemProjectContentStore,
            SystemClock,
            UuidV7IdGenerator,
        >,
    >,
    content: FilesystemProjectContentStore,
    publication: PublicationManager<
        FilesystemProjectRepository,
        AxumLocalPublisher,
        PublicationSnapshotStore,
        OsRouteEntropy,
        T,
    >,
    agent: AgentService<E, FilesystemCreationRegistrar>,
}

impl AppState<OpenCodeAgentEngine, CloudflareQuickTunnel> {
    /// Production constructor: real OpenCode engine + Cloudflare Quick Tunnel.
    pub fn new(config: AppConfig) -> Self {
        let engine = OpenCodeAgentEngine::new(
            config.opencode_binary,
            config.opencode_config_dir,
            config.opencode_port,
        );
        let resolver: Box<dyn BinaryResolver> = match config.cloudflared_binary {
            Some(path) => Box::new(FixedBinaryResolver::new(path)),
            None => Box::new(PathBinaryResolver::new("cloudflared")),
        };
        let tunnel = CloudflareQuickTunnel::new(resolver);
        Self::with_components(config.data_dir, engine, tunnel)
    }
}

impl<E, T> AppState<E, T>
where
    E: AgentEngine,
    T: TunnelProvider,
{
    /// Dependency-injection constructor (tests inject `FakeAgentEngine` /
    /// `FakeTunnel`); all filesystem components still target the real base dir.
    pub fn with_components(base: PathBuf, engine: E, tunnel: T) -> Self {
        let projects = ProjectService::new(
            FilesystemProjectRepository::new(base.clone()),
            FilesystemProjectContentStore::new(base.clone()),
            SystemClock,
            UuidV7IdGenerator,
        );
        let snapshots = PublicationSnapshotStore::new(base.clone());
        let roots = ProjectPublishRootProvider::new(base.clone());
        let publisher = AxumLocalPublisher::new();
        let publication = PublicationManager::with_tunnel(
            FilesystemProjectRepository::new(base.clone()),
            snapshots,
            roots,
            publisher,
            OsRouteEntropy,
            tunnel,
        );
        let registrar = FilesystemCreationRegistrar::new(base.clone());
        let agent = AgentService::new(engine, registrar, base.clone());
        let content = FilesystemProjectContentStore::new(base.clone());
        Self {
            base,
            projects: Mutex::new(projects),
            content,
            publication,
            agent,
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base
    }

    // -- Projects ----------------------------------------------------------

    pub fn list_projects(&self) -> AppResult<Vec<ProjectSummary>> {
        let projects = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .list_projects()
            .map_err(AppError::from_core)?;
        Ok(projects
            .into_iter()
            .map(|p| ProjectSummary {
                id: p.id.as_str().to_owned(),
                name: p.name.as_str().to_owned(),
            })
            .collect())
    }

    pub fn create_project(&self, name: &str) -> AppResult<ProjectSummary> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .create_project(name)
            .map_err(AppError::from_core)?;
        Ok(ProjectSummary {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
        })
    }

    pub fn rename_project(&self, id: &str, name: &str) -> AppResult<ProjectSummary> {
        let pid = parse_project_id(id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .rename_project(&pid, name)
            .map_err(AppError::from_core)?;
        Ok(ProjectSummary {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
        })
    }

    pub fn delete_project(&self, id: &str) -> AppResult<()> {
        let pid = parse_project_id(id)?;
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .delete_project(&pid)
            .map_err(AppError::from_core)
    }

    pub fn open_project(&self, id: &str) -> AppResult<ProjectView> {
        let pid = parse_project_id(id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let materials = project.materials.iter().map(material_view).collect();
        let creations = project.creations.iter().map(creation_view).collect();
        let publication = self.publication_status(id)?;
        Ok(ProjectView {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
            materials,
            creations,
            publication,
        })
    }

    // -- Materials ---------------------------------------------------------

    pub fn add_material_from_path(
        &self,
        project_id: &str,
        source_path: &str,
    ) -> AppResult<MaterialView> {
        let pid = parse_project_id(project_id)?;
        let (file_name, bytes, content_type) = read_source_file(Path::new(source_path))?;
        let request = AddMaterial {
            display_name: file_name.clone(),
            original_file_name: file_name,
            content_type,
            source: MaterialContent { bytes },
        };
        let material = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_material(&pid, request)
            .map_err(AppError::from_material)?;
        Ok(material_view(&material))
    }

    // -- Creations ---------------------------------------------------------

    pub fn set_creation_visibility(
        &self,
        project_id: &str,
        creation_id: &str,
        public: bool,
    ) -> AppResult<CreationView> {
        let pid = parse_project_id(project_id)?;
        let cid = parse_creation_id(creation_id)?;
        let visibility = if public {
            CreationVisibility::Public
        } else {
            CreationVisibility::Private
        };
        let creation = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_creation_visibility(&pid, &cid, visibility)
            .map_err(AppError::from_core)?;
        Ok(creation_view(&creation))
    }

    /// Resolves the validated, canonical on-disk path for a creation. Used by
    /// `open_creation` and never exposes an arbitrary-path open capability.
    pub fn creation_path(&self, project_id: &str, creation_id: &str) -> AppResult<PathBuf> {
        let pid = parse_project_id(project_id)?;
        let cid = parse_creation_id(creation_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let creation = project
            .creations
            .iter()
            .find(|c| c.id == cid)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró esa creación."))?;
        self.content
            .creation_path(&pid, creation)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir ese recurso."))
    }

    /// Opens a creation with the host system default handler (documents via
    /// their native app; `index.html` web creations via the default browser).
    pub fn open_creation(&self, project_id: &str, creation_id: &str) -> AppResult<()> {
        let path = self.creation_path(project_id, creation_id)?;
        opener::open(path)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir ese recurso."))
    }

    // -- Agent -------------------------------------------------------------

    pub fn run_agent(&self, project_id: &str, prompt: &str) -> AppResult<AgentRunView> {
        if project_id.trim().is_empty() {
            return Err(AppError::invalid("Ese proyecto no es válido."));
        }
        if prompt.trim().is_empty() {
            return Err(AppError::invalid("Escribí qué querés crear."));
        }
        let result = self
            .agent
            .run(AgentRequest {
                project_id: project_id.to_owned(),
                prompt: AgentPrompt {
                    text: prompt.to_owned(),
                    model: None,
                },
            })
            .or_else(|error| match error {
                project_agent::AgentError::Cancelled => Ok(AgentRunResult {
                    task: AgentTask {
                        id: "cancelled".to_owned(),
                        status: TaskStatus::Cancelled,
                        artifacts: Vec::new(),
                        message: Some("La creación se canceló.".to_owned()),
                    },
                    registered: Vec::new(),
                }),
                other => Err(AppError::from_agent(other)),
            })?;
        Ok(AgentRunView {
            status: match result.task.status {
                TaskStatus::Completed => "completed".to_owned(),
                TaskStatus::Cancelled => "cancelled".to_owned(),
                _ => "failed".to_owned(),
            },
            registered_creation_ids: result.registered,
            message: result.task.message,
        })
    }

    pub fn cancel_agent(&self, project_id: &str) -> AppResult<()> {
        self.agent.cancel(project_id).map_err(AppError::from_agent)
    }

    pub fn agent_status(&self) -> &'static str {
        match self.agent.engine_status() {
            AgentStatus::Stopped => "stopped",
            AgentStatus::Starting => "starting",
            AgentStatus::Ready => "ready",
            AgentStatus::Failed => "failed",
        }
    }

    // -- Publication -------------------------------------------------------

    pub fn publish(&self, project_id: &str) -> AppResult<PublicationView> {
        let pid = parse_project_id(project_id)?;
        let publication = self
            .publication
            .publish(&pid)
            .map_err(AppError::from_publication)?;
        Ok(PublicationView {
            state: "published".to_owned(),
            public_url: publication.public_url,
        })
    }

    pub fn unpublish(&self, project_id: &str) -> AppResult<PublicationView> {
        let pid = parse_project_id(project_id)?;
        self.publication
            .unpublish(&pid)
            .map_err(AppError::from_publication)?;
        Ok(PublicationView {
            state: "local".to_owned(),
            public_url: None,
        })
    }

    pub fn publication_status(&self, project_id: &str) -> AppResult<PublicationView> {
        let pid = parse_project_id(project_id)?;
        let published = self
            .publication
            .list_published()
            .map_err(AppError::from_publication)?;
        match published.into_iter().find(|p| p.project_id == pid) {
            Some(p) => Ok(PublicationView {
                state: "published".to_owned(),
                public_url: p.public_url,
            }),
            None => Ok(PublicationView {
                state: "local".to_owned(),
                public_url: None,
            }),
        }
    }

    /// Opens the currently published public URL in the system browser. The URL
    /// is resolved backend-side; the frontend never supplies an arbitrary URL.
    pub fn open_public_url(&self, project_id: &str) -> AppResult<()> {
        let status = self.publication_status(project_id)?;
        let url = status
            .public_url
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "El proyecto no está publicado."))?;
        opener::open_browser(url)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir el enlace."))
    }

    // -- Status ------------------------------------------------------------

    pub fn app_status(&self) -> AppStatusView {
        AppStatusView {
            version: APP_VERSION.to_owned(),
            agent: self.agent_status().to_owned(),
        }
    }
}

fn parse_project_id(id: &str) -> AppResult<ProjectId> {
    ProjectId::parse(id).map_err(|_| AppError::invalid("Ese proyecto no es válido."))
}

fn parse_creation_id(id: &str) -> AppResult<CreationId> {
    CreationId::parse(id).map_err(|_| AppError::invalid("Esa creación no es válida."))
}

fn material_view(m: &Material) -> MaterialView {
    MaterialView {
        id: m.id.as_str().to_owned(),
        display_name: m.display_name.clone(),
        original_file_name: m.original_file_name.clone(),
        kind: material_kind(&m.original_file_name).to_owned(),
        byte_size: m.byte_size,
    }
}

fn creation_view(c: &Creation) -> CreationView {
    CreationView {
        id: c.id.as_str().to_owned(),
        display_name: c.display_name.clone(),
        kind: creation_kind_code(c.kind).to_owned(),
        visibility: match c.visibility {
            CreationVisibility::Public => "public".to_owned(),
            CreationVisibility::Private => "private".to_owned(),
        },
        byte_size: c.byte_size,
    }
}

fn creation_kind_code(kind: CreationKind) -> &'static str {
    match kind {
        CreationKind::Web => "web",
        CreationKind::Document => "document",
        CreationKind::Image => "image",
        CreationKind::File => "file",
    }
}

fn material_kind(file_name: &str) -> &'static str {
    match ext(file_name).as_str() {
        "pdf" => "pdf",
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => "image",
        "doc" | "docx" | "odt" | "rtf" => "document",
        "xls" | "xlsx" | "ods" | "csv" => "spreadsheet",
        "ppt" | "pptx" | "odp" => "presentation",
        "md" | "txt" => "text",
        _ => "other",
    }
}

fn content_type_from_name(file_name: &str) -> Option<ContentType> {
    let ct = match ext(file_name).as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "html" => "text/html",
        "csv" => "text/csv",
        _ => return None,
    };
    ContentType::parse(ct).ok()
}

fn ext(file_name: &str) -> String {
    file_name
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Reads a user-supplied source file for material ingestion, rejecting
/// symlinks, directories, and other non-regular files before reading bytes.
/// The original is never moved or modified.
fn read_source_file(path: &Path) -> AppResult<(String, Vec<u8>, Option<ContentType>)> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| AppError::new(ErrorCode::MaterialFailed, "No pudimos agregar ese archivo."))?;
    if meta.file_type().is_symlink() {
        return Err(AppError::new(
            ErrorCode::MaterialFailed,
            "No pudimos agregar ese archivo.",
        ));
    }
    if !meta.is_file() {
        return Err(AppError::invalid("Ese archivo no es válido."));
    }
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::invalid("Ese archivo no es válido."))?
        .to_owned();
    let bytes = std::fs::read(path)
        .map_err(|_| AppError::new(ErrorCode::MaterialFailed, "No pudimos agregar ese archivo."))?;
    let content_type = content_type_from_name(&file_name);
    Ok((file_name, bytes, content_type))
}
