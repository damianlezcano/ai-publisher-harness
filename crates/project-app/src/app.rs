//! Application facade: the Tauri-free application core that wires M1-M5.
//!
//! `AppState` composes `ProjectService` (project/material/creation CRUD), the
//! `PublicationManager` (publish/unpublish), and `AgentService` (agent tasks),
//! and exposes high-level, UI-oriented operations returning serializable DTOs
//! plus human-facing errors. The Tauri command layer is a thin adapter over
//! this facade; no domain logic lives in the frontend.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use project_agent::model::{ModelRef, TaskStatus};
use project_agent::{
    AgentAttachment, AgentEngine, AgentPrompt, AgentRequest, AgentRunResult, AgentService,
    AgentStatus, FilesystemCreationRegistrar, OpenCodeAgentEngine,
};
use project_core::{
    AddMaterial, ContentType, Creation, CreationId, CreationKind, CreationVisibility, Material,
    MaterialContent, MaterialId, Message, MessageId, MessageRole, MessageStatus,
    ProjectContentStore, ProjectId, ProjectService, SystemClock, UuidV7IdGenerator,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, ProjectPublishRootProvider,
    PublicationSnapshotStore,
};
use project_opencode::OpenCodeBackend;
use project_preview::PreviewServer;
use project_provider::{
    BackendRestarter, ConnectionTest, ConnectionView, ModelSummary, OAuthAttempt, OAuthStatus,
    OpenCodeProviderConnector, ProviderConnector, ProviderDetail, ProviderError, ProviderResult,
    ProviderService, ProviderSummary, SecretString,
};
use project_publication::{OsRouteEntropy, PublicationManager};
use project_publisher::AxumLocalPublisher;
use project_tunnel::{
    BinaryResolver, CloudflareQuickTunnel, FixedBinaryResolver, PathBinaryResolver, TunnelProvider,
};
use sha2::{Digest, Sha256};

use crate::dtos::*;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::sidecar::SidecarLocation;

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

/// Maps resolved sidecar locations onto the [`AppConfig`] binary fields (M10
/// T2). `Bundled(p)` yields the absolute bundled path; `OnPath(name)` yields a
/// bare name for `opencode` (the dev/`PATH` fallback) and `None` for
/// `cloudflared` (preserving the existing lazy `BinaryNotFound` failure at
/// first use). Pure; the Tauri shell supplies the resolutions.
pub fn apply_sidecar_locations(
    opencode: SidecarLocation,
    cloudflared: SidecarLocation,
) -> (PathBuf, Option<PathBuf>) {
    let opencode_binary = match opencode {
        SidecarLocation::Bundled(path) => path,
        SidecarLocation::OnPath(name) => PathBuf::from(name),
    };
    let cloudflared_binary = match cloudflared {
        SidecarLocation::Bundled(path) => Some(path),
        SidecarLocation::OnPath(_) => None,
    };
    (opencode_binary, cloudflared_binary)
}

/// Shuts down the shared `opencode serve` backend after a credential mutation.
/// The agent engine lazily respawns it (and drops stale sessions) on next use.
pub struct SharedBackendRestarter {
    backend: Arc<OpenCodeBackend>,
}

impl SharedBackendRestarter {
    pub fn new(backend: Arc<OpenCodeBackend>) -> Self {
        Self { backend }
    }
}

impl BackendRestarter for SharedBackendRestarter {
    fn restart(&self) -> ProviderResult<()> {
        self.backend
            .shutdown()
            .map_err(|err| ProviderError::Internal(err.to_string()))
    }
}

pub struct AppState<
    E = OpenCodeAgentEngine,
    T = CloudflareQuickTunnel,
    P = OpenCodeProviderConnector,
    R = SharedBackendRestarter,
> where
    E: AgentEngine,
    T: TunnelProvider,
    P: ProviderConnector,
    R: BackendRestarter,
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
    provider: ProviderService<P, R>,
    /// Live isolated web-preview servers keyed by their single-use token. Each
    /// entry serves one immutable copy of a creation's `outputs/<id>` tree on a
    /// loopback-only, token-guarded endpoint (ADR-0010). Removed (and torn down)
    /// by `preview_close`.
    previews: Mutex<std::collections::HashMap<String, LivePreview>>,
}

/// A running isolated web preview: the loopback token server plus the immutable
/// snapshot it serves. Dropping this stops the server, invalidates the token,
/// and removes the snapshot directory.
#[expect(dead_code)]
struct LivePreview {
    server: PreviewServer,
    snapshot: tempfile::TempDir,
}

impl
    AppState<
        OpenCodeAgentEngine,
        CloudflareQuickTunnel,
        OpenCodeProviderConnector,
        SharedBackendRestarter,
    >
{
    /// Production constructor: real shared OpenCode backend (one `opencode
    /// serve` for the agent engine and the provider connector), Cloudflare
    /// Quick Tunnel, and an app-managed data dir with owner-only permissions.
    /// Fails closed when the app data dir cannot be prepared.
    pub fn new(config: AppConfig) -> AppResult<Self> {
        ensure_app_data_dir(&config.data_dir)?;
        let backend = Arc::new(OpenCodeBackend::new(
            config.opencode_binary,
            config.opencode_config_dir,
            config.opencode_port,
        ));
        let engine = OpenCodeAgentEngine::from_backend(Arc::clone(&backend));
        let scratch = config.data_dir.join("opencode-scratch");
        fs::create_dir_all(&scratch)
            .map_err(|_| AppError::internal("No se pudo inicializar el directorio de datos."))?;
        let connector =
            OpenCodeProviderConnector::new(Arc::clone(&backend)).with_scratch_root(scratch);
        let restarter = SharedBackendRestarter::new(Arc::clone(&backend));
        let resolver: Box<dyn BinaryResolver> = match config.cloudflared_binary {
            Some(path) => Box::new(FixedBinaryResolver::new(path)),
            None => Box::new(PathBinaryResolver::new("cloudflared")),
        };
        let tunnel = CloudflareQuickTunnel::new(resolver);
        Ok(Self::with_components(
            config.data_dir,
            engine,
            tunnel,
            connector,
            restarter,
        ))
    }
}

/// Validated inputs for one agent run. The raw prompt is preserved so the
/// durable user message contains exactly what the user typed.
pub struct AgentRunInputs {
    project_id: ProjectId,
    /// The durable user message is the logical identity of this run. It avoids
    /// relying on assistant-message position or a session-wide count.
    turn_id: Option<MessageId>,
    prompt: String,
    model: Option<ModelRef>,
    attachments: Vec<AgentAttachment>,
}

impl AgentRunInputs {
    pub fn turn_id(&self) -> Option<&str> {
        self.turn_id.as_ref().map(MessageId::as_str)
    }
}

impl<E, T, P, R> AppState<E, T, P, R>
where
    E: AgentEngine,
    T: TunnelProvider,
    P: ProviderConnector,
    R: BackendRestarter,
{
    /// Dependency-injection constructor (tests inject `FakeAgentEngine` /
    /// `FakeTunnel` / `FakeProviderConnector` / `FakeRestarter`); all
    /// filesystem components still target the real base dir.
    pub fn with_components(
        base: PathBuf,
        engine: E,
        tunnel: T,
        connector: P,
        restarter: R,
    ) -> Self {
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
        let provider = ProviderService::new(connector, restarter, base.join("settings.json"));
        Self {
            base,
            projects: Mutex::new(projects),
            content,
            publication,
            agent,
            provider,
            previews: Mutex::new(std::collections::HashMap::new()),
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
        let published: std::collections::HashSet<ProjectId> = self
            .publication
            .list_published()
            .map_err(AppError::from_publication)?
            .into_iter()
            .map(|p| p.project_id)
            .collect();
        Ok(projects
            .into_iter()
            .map(|p| ProjectSummary {
                id: p.id.as_str().to_owned(),
                name: p.name.as_str().to_owned(),
                created_at: p.created_at.as_str().to_owned(),
                updated_at: p.updated_at.as_str().to_owned(),
                shared: published.contains(&p.id),
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
        crate::session_log::record("INFO", format!("conversation created id={}", project.id));
        Ok(ProjectSummary {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
            created_at: project.created_at.as_str().to_owned(),
            updated_at: project.updated_at.as_str().to_owned(),
            shared: false,
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
        crate::session_log::record("INFO", format!("conversation renamed id={id}"));
        Ok(ProjectSummary {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
            created_at: project.created_at.as_str().to_owned(),
            updated_at: project.updated_at.as_str().to_owned(),
            shared: self
                .publication_status(id)
                .map(|p| p.state == "published")
                .unwrap_or(false),
        })
    }

    pub fn delete_project(&self, id: &str) -> AppResult<()> {
        // Unpublish first so a shared project never survives as a stale entry
        // in PublicationManager. `unpublish` is idempotent, so this is safe
        // when the project is already local. If unpublish fails we fail closed
        // and do not begin removing project data.
        self.unpublish(id)?;

        // Serialize with the agent for this project: cancel any in-flight run
        // and hold the per-project lock so a run that starts after this point
        // sees the missing project and aborts before recreating files.
        let agent_lock = self.agent.project_lock(id);
        let _agent_guard = agent_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(err) = self.agent.cancel(id) {
            // No active session is fine; anything else must stop the delete.
            if !matches!(err, project_agent::AgentError::SessionNotFound(_)) {
                return Err(AppError::from_agent(err));
            }
        }

        let pid = parse_project_id(id)?;
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .delete_project(&pid)
            .map_err(AppError::from_core)?;
        crate::session_log::record("INFO", format!("conversation deleted id={id}"));
        Ok(())
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
        let messages = project.messages.iter().map(message_view).collect();
        let publication = self.publication_status(id)?;
        Ok(ProjectView {
            id: project.id.as_str().to_owned(),
            name: project.name.as_str().to_owned(),
            materials,
            creations,
            messages,
            publication,
            model: project.model.as_ref().map(|model| ConversationModelView {
                provider_id: model.provider_id.clone(),
                model_id: model.model_id.clone(),
            }),
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
        crate::session_log::record(
            "INFO",
            format!(
                "attachment associated conversation_id={project_id} material_id={} name={} bytes={}",
                material.id,
                project_core::safe_file_name(&material.original_file_name),
                material.byte_size
            ),
        );
        Ok(material_view(&material))
    }

    /// Clipboard image paste (M8 §4). The image bytes are validated fail-closed
    /// (allowed type, magic-byte sniff, 25 MB cap, empty rejection), the file
    /// name is deterministically synthesized, and a content SHA-256 duplicate of
    /// an existing project material returns the existing material (`duplicate:
    /// true`) instead of storing a second copy. The original clipboard bytes are
    /// never modified and no new clipboard privilege is granted.
    pub fn add_material_image(
        &self,
        project_id: &str,
        file_name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> AppResult<MaterialAddImageView> {
        let pid = parse_project_id(project_id)?;
        let image = validate_clipboard_image(file_name, content_type, &bytes)?;
        let sha = sha256_hex(&bytes);
        if let Some(existing) = self.find_material_by_sha(&pid, &sha)? {
            return Ok(MaterialAddImageView {
                material: material_view(&existing),
                duplicate: true,
            });
        }
        let request = AddMaterial {
            display_name: "Captura".to_owned(),
            original_file_name: image.synthesized_name,
            content_type: Some(ContentType::parse(image.content_type).expect("validated type")),
            source: MaterialContent { bytes },
        };
        let material = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_material(&pid, request)
            .map_err(AppError::from_material)?;
        Ok(MaterialAddImageView {
            material: material_view(&material),
            duplicate: false,
        })
    }

    /// Multi-file import (M8 §5). Each input file is processed independently and
    /// reported in input order with a deterministic per-file status
    /// (`added` / `duplicate` / `unsupported` / `failed`); one bad file never
    /// aborts the batch. Sources are only ever read; originals are never
    /// modified. Dedup uses content SHA-256 against existing project materials
    /// and earlier entries in the same batch.
    pub fn import_materials(
        &self,
        project_id: &str,
        paths: Vec<String>,
    ) -> AppResult<MaterialsImportReport> {
        let pid = parse_project_id(project_id)?;
        let mut items = Vec::with_capacity(paths.len());
        for path in paths {
            items.push(self.import_one_material(&pid, &path)?);
        }
        Ok(MaterialsImportReport { items })
    }

    fn import_one_material(&self, pid: &ProjectId, path: &str) -> AppResult<MaterialImportResult> {
        let source_name = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .map(project_core::safe_file_name)
            .unwrap_or_default();
        let source_name = if source_name.is_empty() {
            "archivo".to_owned()
        } else {
            source_name
        };
        // Reject oversize and non-regular sources BEFORE reading any bytes so
        // the backend never buffers an unbounded frontend-supplied path.
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_file() && meta.len() <= MAX_IMPORT_FILE_BYTES => {}
            Ok(meta) if meta.file_type().is_file() => {
                return Ok(MaterialImportResult {
                    source_name,
                    status: "unsupported".to_owned(),
                    material_id: None,
                    reason: Some("Ese archivo es demasiado grande.".to_owned()),
                    material: None,
                });
            }
            Ok(_) => {
                return Ok(MaterialImportResult {
                    source_name,
                    status: "unsupported".to_owned(),
                    material_id: None,
                    reason: Some("Ese archivo no es válido.".to_owned()),
                    material: None,
                });
            }
            Err(_) => {
                return Ok(MaterialImportResult {
                    source_name,
                    status: "failed".to_owned(),
                    material_id: None,
                    reason: Some("No pudimos agregar ese archivo.".to_owned()),
                    material: None,
                });
            }
        }
        match read_source_file(Path::new(path)) {
            Err(AppError {
                code: ErrorCode::MaterialFailed,
                ..
            }) => Ok(MaterialImportResult {
                source_name,
                status: "failed".to_owned(),
                material_id: None,
                reason: Some("No pudimos agregar ese archivo.".to_owned()),
                material: None,
            }),
            Err(_) => Ok(MaterialImportResult {
                source_name,
                status: "unsupported".to_owned(),
                material_id: None,
                reason: Some("Ese archivo no es válido.".to_owned()),
                material: None,
            }),
            Ok((file_name, bytes, content_type)) => {
                let sha = sha256_hex(&bytes);
                match self.find_material_by_sha(pid, &sha) {
                    Ok(Some(existing)) => {
                        return Ok(MaterialImportResult {
                            source_name,
                            status: "duplicate".to_owned(),
                            material_id: Some(existing.id.as_str().to_owned()),
                            reason: Some("Ese archivo ya está en el proyecto.".to_owned()),
                            material: Some(material_view(&existing)),
                        });
                    }
                    // A read error on the project metadata marks this item
                    // failed rather than aborting the whole batch.
                    Err(_) => {
                        return Ok(MaterialImportResult {
                            source_name,
                            status: "failed".to_owned(),
                            material_id: None,
                            reason: Some("No pudimos agregar ese archivo.".to_owned()),
                            material: None,
                        });
                    }
                    Ok(None) => {}
                }
                let request = AddMaterial {
                    display_name: file_name.clone(),
                    original_file_name: file_name,
                    content_type,
                    source: MaterialContent { bytes },
                };
                match self
                    .projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .add_material(pid, request)
                {
                    Ok(material) => Ok(MaterialImportResult {
                        source_name,
                        status: "added".to_owned(),
                        material_id: Some(material.id.as_str().to_owned()),
                        reason: None,
                        material: Some(material_view(&material)),
                    }),
                    Err(_) => Ok(MaterialImportResult {
                        source_name,
                        status: "failed".to_owned(),
                        material_id: None,
                        reason: Some("No pudimos agregar ese archivo.".to_owned()),
                        material: None,
                    }),
                }
            }
        }
    }

    fn find_material_by_sha(&self, pid: &ProjectId, sha: &str) -> AppResult<Option<Material>> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(pid)
            .map_err(AppError::from_core)?;
        Ok(project
            .materials
            .iter()
            .find(|m| m.sha256.as_str() == sha)
            .cloned())
    }

    /// Removes a material: the metadata reference is removed under optimistic
    /// concurrency and the app-managed `inputs/<id>` copy is deleted. The user's
    /// original source file is never touched.
    pub fn remove_material(&self, project_id: &str, material_id: &str) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let mid = parse_material_id(material_id)?;
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove_material(&pid, &mid)
            .map_err(AppError::from_material)
    }

    /// Resolves the validated, canonical on-disk path for a material. Used by
    /// `open_material`; never exposes an arbitrary-path open capability.
    pub fn material_path(&self, project_id: &str, material_id: &str) -> AppResult<PathBuf> {
        let pid = parse_project_id(project_id)?;
        let mid = parse_material_id(material_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let material = project
            .materials
            .iter()
            .find(|m| m.id == mid)
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese material."))?;
        self.content
            .material_path(&pid, material)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir ese recurso."))
    }

    /// Opens a material with the host system default handler (read-only).
    pub fn open_material(&self, project_id: &str, material_id: &str) -> AppResult<()> {
        let path = self.material_path(project_id, material_id)?;
        opener::open(path)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir ese recurso."))
    }

    pub fn open_material_folder(&self, project_id: &str, material_id: &str) -> AppResult<()> {
        let path = self.material_path(project_id, material_id)?;
        let directory = path
            .parent()
            .ok_or_else(|| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))?;
        opener::open(directory)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))
    }

    /// Opens the project's canonical `inputs/` folder (the shared container of
    /// all uploaded materials) with the host system default handler. Resolves
    /// the folder through the content store with the same symlink/containment
    /// validation as any material path.
    pub fn open_materials_folder(&self, project_id: &str) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let directory = self
            .content
            .materials_dir(&pid)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))?;
        opener::open(directory)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))
    }

    /// Opens the project's canonical `outputs/` folder (the shared container of
    /// all generated creations) with the host system default handler. Resolves
    /// the folder through the content store with the same symlink/containment
    /// validation as any creation path.
    pub fn open_creations_folder(&self, project_id: &str) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let directory = self
            .content
            .creations_dir(&pid)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))?;
        opener::open(directory)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))
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

    pub fn open_creation_folder(&self, project_id: &str, creation_id: &str) -> AppResult<()> {
        let path = self.creation_path(project_id, creation_id)?;
        let directory = path
            .parent()
            .ok_or_else(|| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))?;
        opener::open(directory)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir la carpeta."))
    }

    // -- Preview ------------------------------------------------------------

    /// In-app preview bytes for images and text/Markdown (M8 §10). Resolves the
    /// resource against the current project (authorization), reads bytes through
    /// the content store, and never returns a path. Resources above the 2 MB
    /// preview cap fall back to the system handler (`PreviewTooLarge`).
    pub fn preview_data(
        &self,
        project_id: &str,
        resource_kind: &str,
        resource_id: &str,
    ) -> AppResult<PreviewData> {
        let pid = parse_project_id(project_id)?;
        let (bytes, content_type) = match resource_kind {
            "material" => {
                let mid = parse_material_id(resource_id)?;
                let material = self.find_material(&pid, &mid)?;
                (
                    self.read_material_bytes(&pid, &material)?,
                    material.content_type,
                )
            }
            "creation" => {
                let cid = parse_creation_id(resource_id)?;
                let creation = self.find_creation(&pid, &cid)?;
                (
                    self.read_creation_bytes(&pid, &creation)?,
                    creation.content_type,
                )
            }
            _ => return Err(AppError::invalid("Ese recurso no es válido.")),
        };
        if bytes.len() as u64 > PREVIEW_MAX_BYTES {
            return Err(AppError::new(
                ErrorCode::PreviewTooLarge,
                "Este recurso es grande; abrilo con la aplicación.",
            ));
        }
        let content_type = content_type
            .map(|c| c.as_str().to_owned())
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        Ok(PreviewData {
            content_type,
            data_base64: encode_base64(&bytes),
        })
    }

    /// Isolated web preview (M8 §11 / ADR-0010). Resolves the creation within
    /// the project, copies its `outputs/<id>` tree into an immutable snapshot,
    /// and starts a loopback-only, token-guarded preview server for that single
    /// copy. Returns the backend-created URL and the single-use teardown token.
    /// The generated content never gains Tauri IPC (empty preview capability).
    pub fn preview_open_web(&self, project_id: &str, creation_id: &str) -> AppResult<WebPreview> {
        let pid = parse_project_id(project_id)?;
        let cid = parse_creation_id(creation_id)?;
        let creation = self.find_creation(&pid, &cid)?;
        let src_dir = self.content.creation_dir(&pid, &creation).map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })?;

        let snapshot = tempfile::Builder::new()
            .prefix("m8-preview-")
            .tempdir()
            .map_err(|_| {
                AppError::new(
                    ErrorCode::PreviewUnavailable,
                    "No pudimos mostrar la vista previa.",
                )
            })?;
        copy_tree(&src_dir, snapshot.path()).map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })?;

        let mut server = PreviewServer::new();
        let endpoint = server
            .start(snapshot.path().to_path_buf(), None)
            .map_err(|_| {
                AppError::new(
                    ErrorCode::PreviewUnavailable,
                    "No pudimos mostrar la vista previa.",
                )
            })?;
        let token = endpoint.token().to_string();
        self.previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(token.clone(), LivePreview { server, snapshot });
        Ok(WebPreview {
            url: endpoint.url().to_owned(),
            token,
        })
    }

    /// Tears down a live web preview: the loopback server is stopped, its
    /// single-use token invalidated, and the immutable snapshot removed.
    pub fn preview_close(&self, token: &str) -> AppResult<()> {
        let mut previews = self.previews.lock().unwrap_or_else(|e| e.into_inner());
        if previews.remove(token).is_none() {
            return Err(AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos cerrar la vista previa.",
            ));
        }
        Ok(())
    }

    fn find_material(&self, pid: &ProjectId, mid: &MaterialId) -> AppResult<Material> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(pid)
            .map_err(AppError::from_core)?;
        project
            .materials
            .iter()
            .find(|m| m.id == *mid)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese material."))
    }

    /// Resolves prompt attachments (M8 §6 / ADR-0011). Each opaque material ID
    /// is validated, authorized against the CURRENT project's materials (a
    /// foreign/unknown ID is rejected), and its bytes are read through the
    /// content store (which re-checks SHA-256). Only sanitized names, stable
    /// kind labels, and bytes reach the agent; no paths cross the boundary.
    fn resolve_attachments(
        &self,
        project_id: &str,
        attachment_ids: &[String],
    ) -> AppResult<Vec<AgentAttachment>> {
        if attachment_ids.is_empty() {
            return Ok(Vec::new());
        }
        let pid = parse_project_id(project_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let mut attachments = Vec::with_capacity(attachment_ids.len());
        for id in attachment_ids {
            let mid = MaterialId::parse(id).map_err(|_| {
                AppError::new(
                    ErrorCode::AttachmentInvalid,
                    "No pudimos adjuntar ese material.",
                )
            })?;
            let material = project
                .materials
                .iter()
                .find(|m| m.id == mid)
                .ok_or_else(|| {
                    AppError::new(
                        ErrorCode::AttachmentInvalid,
                        "No pudimos adjuntar ese material.",
                    )
                })?;
            let bytes = self.content.read_material(&pid, material).map_err(|_| {
                AppError::new(
                    ErrorCode::AttachmentInvalid,
                    "No pudimos adjuntar ese material.",
                )
            })?;
            attachments.push(AgentAttachment {
                // Sanitized name only (never a path); the agent re-sanitizes
                // defensively for the prompt (ADR-0011).
                display_name: project_core::safe_file_name(&material.original_file_name),
                kind: material_kind(&material.original_file_name).to_owned(),
                bytes,
            });
        }
        Ok(attachments)
    }

    fn find_creation(&self, pid: &ProjectId, cid: &CreationId) -> AppResult<Creation> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(pid)
            .map_err(AppError::from_core)?;
        project
            .creations
            .iter()
            .find(|c| c.id == *cid)
            .cloned()
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró esa creación."))
    }

    fn read_material_bytes(&self, pid: &ProjectId, material: &Material) -> AppResult<Vec<u8>> {
        self.content.read_material(pid, material).map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })
    }

    fn read_creation_bytes(&self, pid: &ProjectId, creation: &Creation) -> AppResult<Vec<u8>> {
        self.content.read_creation(pid, creation).map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })
    }

    // -- Agent -------------------------------------------------------------

    /// Durable send: persists the raw user message, runs the agent, then
    /// persists the assistant outcome. The whole flow is exposed for tests and
    /// direct callers; the Tauri layer uses the split `send_message_persist` /
    /// `send_message_run` so it can emit `agent://task/working` only after the
    /// user message is durably appended.
    pub fn send_message(
        &self,
        project_id: &str,
        prompt: &str,
        attachment_ids: &[String],
    ) -> AppResult<AgentRunView> {
        let inputs = self.send_message_persist(project_id, prompt, attachment_ids)?;
        self.send_message_run(inputs)
    }

    /// Validates the prompt/model/attachments and appends the raw user message
    /// to the project. Returns the prepared inputs needed to run the agent.
    /// The user message is persisted before this returns, even if the caller
    /// never invokes `send_message_run`.
    pub fn send_message_persist(
        &self,
        project_id: &str,
        prompt: &str,
        attachment_ids: &[String],
    ) -> AppResult<AgentRunInputs> {
        let mut inputs = self.resolve_agent_inputs(project_id, prompt, attachment_ids)?;
        let material_ids: Vec<MaterialId> = attachment_ids
            .iter()
            .map(|id| parse_material_id(id))
            .collect::<AppResult<Vec<_>>>()?;
        let user_message = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_user_message(&inputs.project_id, prompt, &material_ids)
            .map_err(AppError::from_core)?;
        inputs.turn_id = Some(user_message.id);
        crate::session_log::record(
            "INFO",
            format!(
                "turn accepted conversation_id={} message_id={} chars={}",
                inputs.project_id,
                inputs.turn_id.as_ref().expect("turn id"),
                prompt.chars().count()
            ),
        );
        Ok(inputs)
    }

    /// Runs the agent using the prepared inputs and appends an assistant
    /// message reflecting the outcome (`ok`, `failed`, or `cancelled`).
    pub fn send_message_run(&self, inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let model = inputs
            .model
            .as_ref()
            .map(|model| format!("{}/{}", model.provider_id, model.model_id))
            .unwrap_or_else(|| "default".to_owned());
        let started = std::time::Instant::now();
        crate::session_log::record(
            "INFO",
            format!(
                "turn started conversation_id={} turn_id={} model={model}",
                project_id,
                turn_id.as_deref().unwrap_or("none")
            ),
        );
        match self.run_agent_with_inputs(inputs) {
            Ok(result) => {
                let creation_ids: Vec<CreationId> = result
                    .registered
                    .iter()
                    .map(|id| parse_creation_id(id))
                    .collect::<AppResult<Vec<_>>>()?;
                let mut text = assistant_reply_text(
                    result.task.message.as_deref(),
                    !result.registered.is_empty(),
                );
                let missing_response = result
                    .task
                    .message
                    .as_deref()
                    .is_none_or(|message| message.trim().is_empty());
                let status = if missing_response && result.registered.is_empty() {
                    MessageStatus::Failed
                } else {
                    MessageStatus::Ok
                };
                if self
                    .refresh_published_snapshot(project_id.as_str(), &result.registered)
                    .is_err()
                {
                    text.push_str(
                        "\n\nEl recurso local se actualizó, pero el enlace compartido no. Volvé a pulsar Compartir.",
                    );
                }
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(&project_id, &text, status, &creation_ids)
                    .map_err(AppError::from_core)?;
                crate::session_log::record(
                    "INFO",
                    format!(
                        "turn terminal conversation_id={} turn_id={} status={} creations={} duration_ms={}",
                        project_id,
                        turn_id.as_deref().unwrap_or("none"),
                        if status == MessageStatus::Ok {
                            "completed"
                        } else {
                            "failed"
                        },
                        creation_ids.len(),
                        started.elapsed().as_millis()
                    ),
                );
                Ok(AgentRunView {
                    status: if status == MessageStatus::Ok {
                        "completed".to_owned()
                    } else {
                        "failed".to_owned()
                    },
                    turn_id: turn_id.clone(),
                    registered_creation_ids: result.registered,
                    message: Some(text),
                })
            }
            Err(project_agent::AgentError::Cancelled) => {
                let text = "La creación se canceló.".to_owned();
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(&project_id, &text, MessageStatus::Cancelled, &[])
                    .map_err(AppError::from_core)?;
                crate::session_log::record(
                    "WARN",
                    format!(
                        "turn terminal conversation_id={} turn_id={} status=cancelled duration_ms={}",
                        project_id,
                        turn_id.as_deref().unwrap_or("none"),
                        started.elapsed().as_millis()
                    ),
                );
                Ok(AgentRunView {
                    status: "cancelled".to_owned(),
                    turn_id: turn_id.clone(),
                    registered_creation_ids: Vec::new(),
                    message: Some(text),
                })
            }
            Err(other) => {
                let err = AppError::from_agent(other);
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(&project_id, &err.message, MessageStatus::Failed, &[])
                    .map_err(AppError::from_core)?;
                crate::session_log::record(
                    "ERROR",
                    format!(
                        "turn terminal conversation_id={} turn_id={} status=failed duration_ms={}",
                        project_id,
                        turn_id.as_deref().unwrap_or("none"),
                        started.elapsed().as_millis()
                    ),
                );
                Ok(AgentRunView {
                    status: "failed".to_owned(),
                    turn_id,
                    registered_creation_ids: Vec::new(),
                    message: Some(err.message),
                })
            }
        }
    }

    pub fn run_agent(
        &self,
        project_id: &str,
        prompt: &str,
        attachment_ids: &[String],
    ) -> AppResult<AgentRunView> {
        let inputs = self.resolve_agent_inputs(project_id, prompt, attachment_ids)?;
        let result = self.run_agent_with_inputs(inputs);
        match result {
            Ok(run) => Ok(AgentRunView {
                turn_id: None,
                status: match run.task.status {
                    TaskStatus::Completed => "completed".to_owned(),
                    TaskStatus::Cancelled => "cancelled".to_owned(),
                    _ => "failed".to_owned(),
                },
                registered_creation_ids: run.registered,
                message: run.task.message,
            }),
            Err(project_agent::AgentError::Cancelled) => Ok(AgentRunView {
                turn_id: None,
                status: "cancelled".to_owned(),
                registered_creation_ids: Vec::new(),
                message: Some("La creación se canceló.".to_owned()),
            }),
            Err(other) => Err(AppError::from_agent(other)),
        }
    }

    /// Validates prompt, global model, and attachments. Does not persist any
    /// message; callers decide when to append the user/assistant messages.
    fn resolve_agent_inputs(
        &self,
        project_id: &str,
        prompt: &str,
        attachment_ids: &[String],
    ) -> AppResult<AgentRunInputs> {
        if project_id.trim().is_empty() {
            return Err(AppError::invalid("Ese proyecto no es válido."));
        }
        if prompt.trim().is_empty() {
            return Err(AppError::invalid("Escribí qué querés crear."));
        }
        let project_id = parse_project_id(project_id)?;
        // An explicit conversation model is captured before the user message
        // is persisted; otherwise retain the established global default.
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&project_id)
            .ok();
        let model = match project.and_then(|project| project.model) {
            Some(model)
                if self
                    .model_list()
                    .unwrap_or_default()
                    .iter()
                    .any(|available| {
                        available.provider_id == model.provider_id
                            && available.model_id == model.model_id
                    }) =>
            {
                Some(ModelRef {
                    provider_id: model.provider_id,
                    model_id: model.model_id,
                })
            }
            Some(model) => {
                crate::session_log::record(
                    "WARN",
                    format!(
                        "conversation model unavailable conversation_id={} model={}/{} falling_back=global",
                        project_id, model.provider_id, model.model_id
                    ),
                );
                self.selected_model_ref()?
            }
            None => self.selected_model_ref()?,
        };
        let attachments = self.resolve_attachments(project_id.as_str(), attachment_ids)?;
        Ok(AgentRunInputs {
            project_id,
            turn_id: None,
            prompt: prompt.to_owned(),
            model,
            attachments,
        })
    }

    /// Runs the agent without holding the projects mutex. The long agent task
    /// is serialized per project by `AgentService`.
    fn run_agent_with_inputs(
        &self,
        inputs: AgentRunInputs,
    ) -> project_agent::AgentResult<AgentRunResult> {
        // Fail fast if the project was deleted before the run could start. The
        // agent lock serialization in `delete_project` stops runs that are
        // already in flight; this guards the gap before `AgentService::run`
        // acquires that lock.
        if self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&inputs.project_id)
            .is_err()
        {
            return Err(project_agent::AgentError::TaskFailed(
                "project no longer exists".into(),
            ));
        }

        self.agent.run(AgentRequest {
            project_id: inputs.project_id.as_str().to_owned(),
            prompt: AgentPrompt {
                text: inputs.prompt,
                model: inputs.model,
            },
            attachments: inputs.attachments,
        })
    }

    /// Resolves the global model to send with a prompt. When no usable free or
    /// recommended model exists the product stops and asks instead of silently
    /// switching provider or to a paid model (ADR-0009).
    fn selected_model_ref(&self) -> AppResult<Option<ModelRef>> {
        let selected = self
            .provider
            .get_selected_model()
            .map_err(AppError::from_provider)?;
        if selected.requires_choice {
            return Err(AppError::new(
                ErrorCode::ModelUnavailable,
                "No encontramos un modelo para usar. Elegí uno en los detalles de la conversación.",
            ));
        }
        Ok(Some(ModelRef {
            provider_id: selected.model.provider_id,
            model_id: selected.model.model_id,
        }))
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

    pub fn session_logs(&self) -> Vec<crate::session_log::SessionLogEntry> {
        crate::session_log::list()
    }

    pub fn clear_session_logs(&self) {
        crate::session_log::clear();
    }

    // -- Provider ------------------------------------------------------------

    pub fn provider_list(&self) -> AppResult<Vec<ProviderSummary>> {
        self.provider
            .list_providers()
            .map_err(AppError::from_provider)
    }

    pub fn provider_detail(&self, provider_id: &str) -> AppResult<ProviderDetail> {
        self.provider
            .provider_detail(provider_id)
            .map_err(AppError::from_provider)
    }

    /// Stores a credential once. The key enters here and is never returned,
    /// persisted, or logged; the frontend only receives an opaque reference.
    pub fn provider_connect_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> AppResult<ConnectionView> {
        self.provider
            .connect_api_key(provider_id, key, label)
            .map_err(AppError::from_provider)
    }

    pub fn provider_oauth_begin(
        &self,
        provider_id: &str,
        method_id: &str,
    ) -> AppResult<OAuthAttempt> {
        self.provider
            .begin_oauth(provider_id, method_id)
            .map_err(AppError::from_provider)
    }

    pub fn provider_oauth_status(&self, attempt_id: &str) -> AppResult<OAuthStatus> {
        self.provider
            .oauth_status(attempt_id)
            .map_err(AppError::from_provider)
    }

    pub fn provider_oauth_complete(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> AppResult<ConnectionView> {
        self.provider
            .complete_oauth(attempt_id, code)
            .map_err(AppError::from_provider)
    }

    pub fn provider_oauth_cancel(&self, attempt_id: &str) -> AppResult<()> {
        self.provider
            .cancel_oauth(attempt_id)
            .map_err(AppError::from_provider)
    }

    pub fn provider_disconnect(&self, credential_id: &str) -> AppResult<()> {
        self.provider
            .disconnect(credential_id)
            .map_err(AppError::from_provider)
    }

    pub fn provider_test_connection(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
    ) -> AppResult<ConnectionTest> {
        self.provider
            .test_connection(provider_id, model_id)
            .map_err(AppError::from_provider)
    }

    /// Opens an OAuth authorization URL in the system browser. The URL comes
    /// from a backend-generated `provider_oauth_begin`; only https URLs are
    /// opened (the frontend never invokes an arbitrary browser URL itself).
    pub fn provider_oauth_open(&self, url: &str) -> AppResult<()> {
        let url = url.trim();
        if !url.starts_with("https://") || url.len() < 12 {
            return Err(AppError::invalid("Ese enlace no es válido."));
        }
        opener::open_browser(url)
            .map_err(|_| AppError::new(ErrorCode::OpenFailed, "No pudimos abrir el enlace."))
    }

    // -- Models --------------------------------------------------------------

    pub fn model_list(&self) -> AppResult<Vec<ModelSummary>> {
        self.provider.list_models().map_err(AppError::from_provider)
    }

    pub fn model_select(&self, provider_id: &str, model_id: &str) -> AppResult<ModelSummary> {
        self.provider
            .select_model(provider_id, model_id)
            .map_err(AppError::from_provider)
    }

    /// Selects a model for exactly one conversation. This does not mutate the
    /// global fallback and is rejected while that conversation has a live run.
    pub fn conversation_model_select(
        &self,
        project_id: &str,
        provider_id: &str,
        model_id: &str,
    ) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let models = self.model_list()?;
        if !models
            .iter()
            .any(|model| model.provider_id == provider_id && model.model_id == model_id)
        {
            return Err(AppError::new(
                ErrorCode::ModelUnavailable,
                "Ese modelo ya no está disponible.",
            ));
        }
        let lock = self.agent.project_lock(project_id);
        let _guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::new(
                    ErrorCode::Conflict,
                    "Esperá a que termine la solicitud antes de cambiar el modelo.",
                ));
            }
        };
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_project_model(
                &pid,
                Some(project_core::ConversationModel {
                    provider_id: provider_id.to_owned(),
                    model_id: model_id.to_owned(),
                }),
            )
            .map_err(AppError::from_core)?;
        crate::session_log::record(
            "INFO",
            format!("conversation model changed id={project_id} model={provider_id}/{model_id}"),
        );
        Ok(())
    }

    /// Clears this conversation's explicit model so future turns use the
    /// global configured default. Existing terminal turns remain unchanged.
    pub fn conversation_model_clear(&self, project_id: &str) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let lock = self.agent.project_lock(project_id);
        let _guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::new(
                    ErrorCode::Conflict,
                    "Esperá a que termine la solicitud antes de cambiar el modelo.",
                ));
            }
        };
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_project_model(&pid, None)
            .map_err(AppError::from_core)?;
        crate::session_log::record(
            "INFO",
            format!("conversation model cleared id={project_id} fallback=global"),
        );
        Ok(())
    }

    pub fn model_get_selected(&self) -> AppResult<SelectedModelView> {
        let selected = self
            .provider
            .get_selected_model()
            .map_err(AppError::from_provider)?;
        Ok(SelectedModelView {
            model: selected.model,
            notice: selected.notice,
            requires_choice: selected.requires_choice,
        })
    }

    // -- Publication -------------------------------------------------------

    pub fn publish(&self, project_id: &str) -> AppResult<PublicationView> {
        self.publish_creation(project_id, None)
    }

    /// Publishes the project. When `creation_id` is set, that Creation is the
    /// share target (marked public; other public webs are demoted). When it is
    /// omitted, the latest web Creation (else the latest Creation) is promoted
    /// so Compartir does not snapshot an empty "Material del proyecto" page.
    pub fn publish_creation(
        &self,
        project_id: &str,
        creation_id: Option<&str>,
    ) -> AppResult<PublicationView> {
        self.prepare_share_visibility(project_id, creation_id)?;
        let pid = parse_project_id(project_id)?;
        let publication = self
            .publication
            .publish(&pid)
            .map_err(AppError::from_publication)?;
        crate::session_log::record(
            "INFO",
            format!(
                "creation shared conversation_id={project_id} creation_id={}",
                creation_id.unwrap_or("latest")
            ),
        );
        Ok(PublicationView {
            state: "published".to_owned(),
            public_url: publication.public_url,
        })
    }

    /// When the project is already shared, rebuild the publish snapshot so the
    /// existing public URL serves the updated Creation (ADR-0004 replace,
    /// same route). No-op if the project is local or nothing was registered.
    fn refresh_published_snapshot(
        &self,
        project_id: &str,
        registered_ids: &[String],
    ) -> AppResult<()> {
        if registered_ids.is_empty() {
            return Ok(());
        }
        let status = self.publication_status(project_id)?;
        if status.state != "published" {
            return Ok(());
        }
        let pid = parse_project_id(project_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        // Only rebuild the live URL when this turn updated a Creation that is
        // already public. A new distinct activity stays private and must not
        // hijack the existing shared snapshot.
        let target = registered_ids.iter().rev().find(|id| {
            project.creations.iter().any(|c| {
                c.id.as_str() == id.as_str()
                    && c.kind == CreationKind::Web
                    && c.visibility == CreationVisibility::Public
            })
        });
        let Some(target) = target else {
            return Ok(());
        };
        self.publish_creation(project_id, Some(target))?;
        Ok(())
    }

    pub fn unpublish(&self, project_id: &str) -> AppResult<PublicationView> {
        let pid = parse_project_id(project_id)?;
        self.publication
            .unpublish(&pid)
            .map_err(AppError::from_publication)?;
        crate::session_log::record("INFO", format!("conversation unshared id={project_id}"));
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

    /// Explicit product-layer visibility decision for Compartir (ADR-0004).
    /// M3 still copies only public creations; this is the higher-layer mark.
    fn prepare_share_visibility(
        &self,
        project_id: &str,
        preferred_creation_id: Option<&str>,
    ) -> AppResult<()> {
        let pid = parse_project_id(project_id)?;
        let preferred = match preferred_creation_id {
            Some(id) => Some(parse_creation_id(id)?),
            None => None,
        };
        let mut projects = self.projects.lock().unwrap_or_else(|e| e.into_inner());
        let project = projects.open_project(&pid).map_err(AppError::from_core)?;
        if project.creations.is_empty() {
            return Ok(());
        }
        let target_id = if let Some(cid) = preferred {
            if !project.creations.iter().any(|c| c.id == cid) {
                return Err(AppError::new(
                    ErrorCode::NotFound,
                    "No se encontró esa creación.",
                ));
            }
            cid
        } else if let Some(web) = project
            .creations
            .iter()
            .rev()
            .find(|c| c.kind == CreationKind::Web)
        {
            web.id.clone()
        } else {
            project.creations.last().expect("non-empty").id.clone()
        };
        let target_is_web = project
            .creations
            .iter()
            .find(|c| c.id == target_id)
            .is_some_and(|c| c.kind == CreationKind::Web);

        let mut changes: Vec<(CreationId, CreationVisibility)> = Vec::new();
        for creation in &project.creations {
            if creation.id == target_id {
                if creation.visibility != CreationVisibility::Public {
                    changes.push((creation.id.clone(), CreationVisibility::Public));
                }
            } else if target_is_web
                && creation.kind == CreationKind::Web
                && creation.visibility == CreationVisibility::Public
            {
                changes.push((creation.id.clone(), CreationVisibility::Private));
            }
        }
        for (id, visibility) in changes {
            projects
                .set_creation_visibility(&pid, &id, visibility)
                .map_err(AppError::from_core)?;
        }
        Ok(())
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

/// Creates the app-managed data dir with owner-only permissions before first
/// use (design §7). Fails closed: an uncreatable or world-accessible app data
/// dir aborts startup rather than silently storing under weak permissions.
/// Credentials never live here; they live under OpenCode's isolated `data/`
/// subtree inside it.
fn ensure_app_data_dir(data_dir: &Path) -> AppResult<()> {
    fs::create_dir_all(data_dir).map_err(|_| {
        AppError::internal("No se pudo inicializar el directorio de datos de la aplicación.")
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700)).map_err(|_| {
            AppError::internal("No se pudo proteger el directorio de datos de la aplicación.")
        })?;
    }
    Ok(())
}

fn parse_creation_id(id: &str) -> AppResult<CreationId> {
    CreationId::parse(id).map_err(|_| AppError::invalid("Esa creación no es válida."))
}

fn assistant_reply_text(message: Option<&str>, has_creation: bool) -> String {
    let trimmed = message.unwrap_or("").trim();
    if trimmed.is_empty() {
        if has_creation {
            "La creación se completó, pero no recibimos una explicación.".to_owned()
        } else {
            "No recibimos una respuesta. Probá de nuevo.".to_owned()
        }
    } else {
        trimmed.to_owned()
    }
}

fn parse_material_id(id: &str) -> AppResult<MaterialId> {
    MaterialId::parse(id).map_err(|_| AppError::invalid("Ese material no es válido."))
}

fn material_view(m: &Material) -> MaterialView {
    MaterialView {
        id: m.id.as_str().to_owned(),
        display_name: m.display_name.clone(),
        original_file_name: m.original_file_name.clone(),
        kind: material_kind(&m.original_file_name).to_owned(),
        byte_size: m.byte_size,
        created_at: m.created_at.as_str().to_owned(),
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
        created_at: c.created_at.as_str().to_owned(),
        revision: c.revision,
    }
}

fn message_view(m: &Message) -> MessageView {
    MessageView {
        id: m.id.as_str().to_owned(),
        role: match m.role {
            MessageRole::User => "user".to_owned(),
            MessageRole::Assistant => "assistant".to_owned(),
        },
        text: m.text.clone(),
        status: match m.status {
            MessageStatus::Ok => "ok".to_owned(),
            MessageStatus::Failed => "failed".to_owned(),
            MessageStatus::Cancelled => "cancelled".to_owned(),
        },
        created_at: m.created_at.as_str().to_owned(),
        material_ids: m
            .material_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
        creation_ids: m
            .creation_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect(),
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

/// Per-file size cap for batch imports (M8 §5). Clipboard images use a stricter
/// cap ([`CLIPBOARD_IMAGE_MAX_BYTES`]).
const MAX_IMPORT_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Per-image size cap for clipboard paste (M8 §4).
const CLIPBOARD_IMAGE_MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Preview cap for in-app image/text previews (M8 §10). Larger resources fall
/// back to the system handler.
const PREVIEW_MAX_BYTES: u64 = 2 * 1024 * 1024;

/// Allowed clipboard image content types (M8 §4).
const ALLOWED_IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/gif",
    "image/bmp",
    "image/svg+xml",
];

/// Validated clipboard image: detected content type plus a deterministic,
/// sanitized file name synthesized from the format.
struct ValidatedClipboardImage {
    content_type: &'static str,
    synthesized_name: String,
}

/// Fail-closed clipboard image validation (M8 §4): allowed declared type,
/// non-empty bytes, 25 MB cap, and a magic-byte sniff that must match the
/// declared type. SVG is validated for the `svg` root element only (it is text;
/// the renderer never executes it). The original bytes are never modified.
fn validate_clipboard_image(
    file_name: &str,
    content_type: &str,
    bytes: &[u8],
) -> AppResult<ValidatedClipboardImage> {
    let declared = content_type.trim().to_ascii_lowercase();
    if !ALLOWED_IMAGE_TYPES.contains(&declared.as_str()) {
        return Err(AppError::new(
            ErrorCode::MaterialImageInvalid,
            "Esa imagen no es válida.",
        ));
    }
    if bytes.is_empty() {
        return Err(AppError::new(
            ErrorCode::MaterialImageInvalid,
            "Esa imagen no es válida.",
        ));
    }
    if bytes.len() as u64 > CLIPBOARD_IMAGE_MAX_BYTES {
        return Err(AppError::new(
            ErrorCode::MaterialTooLarge,
            "Esa imagen es demasiado grande.",
        ));
    }
    let detected = sniff_image_type(bytes).ok_or_else(|| {
        AppError::new(ErrorCode::MaterialImageInvalid, "Esa imagen no es válida.")
    })?;
    if detected != declared {
        return Err(AppError::new(
            ErrorCode::MaterialImageInvalid,
            "Esa imagen no es válida.",
        ));
    }
    let extension = match detected {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => "png",
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let _ = file_name; // name is synthesized deterministically; the pasted name is ignored
    let synthesized_name = project_core::safe_file_name(&format!("captura-{stamp}.{extension}"));
    Ok(ValidatedClipboardImage {
        content_type: match detected {
            "image/jpeg" => "image/jpeg",
            other => other,
        },
        synthesized_name,
    })
}

/// Magic-byte sniff for the allowed clipboard image formats (M8 §4).
/// Returns the detected content type, or `None` when the bytes do not match a
/// known signature.
fn sniff_image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if is_svg_root(bytes) {
        return Some("image/svg+xml");
    }
    None
}

/// SVG is validated for the `svg` root element only: the bytes are text and must
/// contain an `<svg` opening tag within a small leading window (after an optional
/// XML prolog, BOM, and whitespace). The renderer never executes it (rendered via
/// `<img>` only). An XML prolog without an actual `<svg` root is rejected.
fn is_svg_root(bytes: &[u8]) -> bool {
    let window = &bytes[..bytes.len().min(1024)];
    let Ok(text) = std::str::from_utf8(window) else {
        return false;
    };
    let text = text.trim_start_matches('\u{feff}').trim_start();
    let text = match text.strip_prefix("<?xml") {
        Some(rest) => {
            // Skip the prolog up to its closing '?>' (bounded window).
            match rest.find("?>") {
                Some(end) => rest[end + 2..].trim_start(),
                None => return false,
            }
        }
        None => text,
    };
    text.starts_with("<svg") || text.starts_with("<svg:")
}

fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut hex = String::with_capacity(64);
    for b in digest {
        hex.push_str(&format!("{b:02x}"));
    }
    hex
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Recursively copies a validated creation tree into an immutable snapshot.
/// Rejects symlinks and non-regular files fail-closed so the copy can never
/// escape the creation's own `outputs/<id>` tree.
fn copy_tree(src: &Path, dst: &Path) -> AppResult<()> {
    for entry in fs::read_dir(src).map_err(|_| {
        AppError::new(
            ErrorCode::PreviewUnavailable,
            "No pudimos mostrar la vista previa.",
        )
    })? {
        let entry = entry.map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })?;
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| {
            AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            )
        })?;
        if meta.file_type().is_symlink() {
            return Err(AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            ));
        }
        let file_name = entry.file_name();
        let target = dst.join(&file_name);
        if meta.is_dir() {
            fs::create_dir(&target).map_err(|_| {
                AppError::new(
                    ErrorCode::PreviewUnavailable,
                    "No pudimos mostrar la vista previa.",
                )
            })?;
            copy_tree(&entry.path(), &target)?;
        } else if meta.is_file() {
            fs::copy(entry.path(), &target).map_err(|_| {
                AppError::new(
                    ErrorCode::PreviewUnavailable,
                    "No pudimos mostrar la vista previa.",
                )
            })?;
        } else {
            return Err(AppError::new(
                ErrorCode::PreviewUnavailable,
                "No pudimos mostrar la vista previa.",
            ));
        }
    }
    Ok(())
}
