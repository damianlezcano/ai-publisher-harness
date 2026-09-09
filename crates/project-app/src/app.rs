//! Application facade: the Tauri-free application core that wires M1-M5.
//!
//! `AppState` composes `ProjectService` (project/material/creation CRUD), the
//! `PublicationManager` (publish/unpublish), and `AgentService` (agent tasks),
//! and exposes high-level, UI-oriented operations returning serializable DTOs
//! plus human-facing errors. The Tauri command layer is a thin adapter over
//! this facade; no domain logic lives in the frontend.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use project_agent::model::{ModelRef, TaskStatus};
use project_agent::{
    AgentAttachment, AgentEngine, AgentEvidenceProvenance, AgentKnowledgeContext,
    AgentKnowledgeEntry, AgentPrompt, AgentRequest, AgentRunResult, AgentService, AgentStatus,
    FilesystemCreationRegistrar, OpenCodeAgentEngine, RemoteUsage, UsageSource,
};
use project_core::{
    AddMaterial, ContentType, Creation, CreationId, CreationKind, CreationVisibility, Material,
    MaterialContent, MaterialId, Message, MessageId, MessageRole, MessageStatus,
    ProjectContentStore, ProjectId, ProjectService, SystemClock, TurnMetrics, UuidV7IdGenerator,
};
use project_fs::{
    FilesystemProjectContentStore, FilesystemProjectRepository, ProjectPublishRootProvider,
    PublicationSnapshotStore,
};
use project_knowledge::{
    ContextAssemblyOptions, EmbeddingProvider, ExhaustiveCoverage, HybridMatchSignals,
    HybridSearchOptions, KnowledgeStore, MaterialSource, ModelInstallState, ModelManager,
    OrtEmbeddingProvider, RetrievalMode, SemanticProviderState, runtime_library_from_executable,
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
    /// One process-local, strictly local E5 provider. It is initialized only
    /// when both the verified model and bundled runtime are available.
    knowledge_provider: Mutex<Option<OrtEmbeddingProvider>>,
    /// Shared OpenCode backend, used by the K6 remote summarizer to run
    /// bounded synthesis in a dedicated scratch session. `None` in DI test
    /// construction (`with_components`), which uses injected fake components
    /// and never runs remote summarization.
    summarizer_backend: Option<Arc<OpenCodeBackend>>,
    /// Live isolated web-preview servers keyed by their single-use token. Each
    /// entry serves one immutable copy of a creation's `outputs/<id>` tree on a
    /// loopback-only, token-guarded endpoint (ADR-0010). Removed (and torn down)
    /// by `preview_close`.
    previews: Mutex<std::collections::HashMap<String, LivePreview>>,
    #[cfg(test)]
    test_activity: Mutex<TestActivity>,
    #[cfg(test)]
    fail_next_turn_metrics_persistence: Mutex<bool>,
}

/// Narrow test-only observability for the zero-cost durable-read contract.
/// These counters never exist in production builds and are incremented only
/// at the real work boundaries, not by tests themselves.
#[cfg(test)]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct TestActivity {
    provider_calls: usize,
    k6_calls: usize,
    indexing: usize,
    embedding_inference: usize,
    embedding_persistence: usize,
    retrieval: usize,
    raw_attachment_forwarding: usize,
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
        let mut state =
            Self::with_components(config.data_dir, engine, tunnel, connector, restarter);
        state.summarizer_backend = Some(backend);
        Ok(state)
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
    /// Opaque material identities selected in this exact composer turn. These
    /// are deliberately retained separately from `attachments`: supported text
    /// is indexed and must not be raw-forwarded merely to preserve coverage.
    selected_material_ids: Vec<String>,
    knowledge: Option<AgentKnowledgeContext>,
    /// Local structural facts observed while preparing this exact request.
    /// They stay local until the owning user turn reaches a successful terminal
    /// state, when they are written with provider accounting.
    knowledge_metrics: Option<TurnKnowledgeMetrics>,
}

#[derive(Clone, Debug)]
struct TurnKnowledgeMetrics {
    material_count: usize,
    corpus_bytes: u64,
    corpus_utf8_chars: usize,
    corpus_est_tokens: usize,
    retrieval_candidate_count: Option<usize>,
    selected_evidence_count: Option<usize>,
    selected_evidence_bytes: Option<usize>,
    selected_evidence_utf8_chars: Option<usize>,
    evidence_est_tokens: Option<usize>,
    context_reduction_pct: Option<usize>,
    semantic_provider_state: String,
    request_preparation_ms: Option<u64>,
    retrieval_mode: Option<String>,
    eligible_materials: Option<usize>,
    materials_inspected: Option<usize>,
    chunks_inspected: Option<usize>,
    exhaustive_coverage: Option<String>,
    lexical_hits: Option<usize>,
    semantic_hits: Option<usize>,
}

/// One source explicitly selected in the current composer turn. This is kept
/// application-local: material ids never cross the provider boundary.
struct SelectedSummarySource {
    source_label: String,
    selected_order: usize,
    document_id: Option<String>,
}

/// Durable hand-off between the fast acceptance boundary and its derived
/// Knowledge work.  It intentionally contains opaque ids only: source paths
/// and document content never escape the acceptance call.
pub struct AcceptedStagedTurn {
    inputs: AgentRunInputs,
    operation_id: String,
    material_ids: Vec<String>,
}

/// A clipboard image held by the composer until its user turn is accepted.
/// This type is deliberately process-local input to the send boundary: it is
/// never written to a project staging directory or exposed in a view/log.
#[derive(Clone, Debug)]
pub struct StagedImage {
    pub staging_id: String,
    pub file_name: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

impl AcceptedStagedTurn {
    pub fn turn_id(&self) -> Option<&str> {
        self.inputs.turn_id()
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
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
            knowledge_provider: Mutex::new(None),
            summarizer_backend: None,
            previews: Mutex::new(std::collections::HashMap::new()),
            #[cfg(test)]
            test_activity: Mutex::new(TestActivity::default()),
            #[cfg(test)]
            fail_next_turn_metrics_persistence: Mutex::new(false),
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
        let accepted_import =
            KnowledgeStore::open(self.base.join("projects").join(pid.as_str()), &pid)
                .ok()
                .and_then(|store| store.latest_accepted_import_operation().ok().flatten())
                .map(accepted_import_progress_view);
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
            accepted_import,
        })
    }

    /// Returns the most recent user message with non-empty turn_metrics for
    /// the given conversation, or None when no completed turn exists.
    ///
    /// This is the durable replacement for the process-local session_log
    /// buffer: metrics persist in project.json alongside the message that
    /// owns them, survive restarts, and are isolated per conversation.
    pub fn last_turn_metrics(&self, project_id: &str) -> AppResult<Option<TurnMetricsView>> {
        let pid = parse_project_id(project_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        // "Último turno" is the newest relevant completed user turn with
        // metrics. A later accepted/incomplete message must not hide it.
        for msg in project.messages.iter().rev() {
            if msg.role == MessageRole::User
                && let Some(metrics) = &msg.turn_metrics
            {
                return Ok(Some(TurnMetricsView::from(metrics.clone())));
            }
        }
        Ok(None)
    }

    fn persist_completed_turn_metrics(
        &self,
        project_id: &ProjectId,
        turn_id: Option<&MessageId>,
        metrics: TurnMetrics,
    ) -> AppResult<()> {
        #[cfg(test)]
        if std::mem::replace(
            &mut *self
                .fail_next_turn_metrics_persistence
                .lock()
                .unwrap_or_else(|e| e.into_inner()),
            false,
        ) {
            return Err(AppError::new(
                ErrorCode::Internal,
                "test-only metrics persistence interruption",
            ));
        }
        let turn_id = turn_id.ok_or_else(|| {
            AppError::new(ErrorCode::Internal, "No pudimos guardar el uso del turno.")
        })?;
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .set_turn_metrics(project_id, turn_id, metrics)
            .map_err(AppError::from_core)?;
        Ok(())
    }

    #[cfg(test)]
    fn fail_next_turn_metrics_persistence(&self) {
        *self
            .fail_next_turn_metrics_persistence
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = true;
    }

    #[cfg(test)]
    fn test_activity(&self) -> TestActivity {
        self.test_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
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
        let material = self.add_material_and_index(&pid, request)?;
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
        let material = self.add_material_and_index(&pid, request)?;
        Ok(MaterialAddImageView {
            material: material_view(&material),
            duplicate: false,
        })
    }

    /// Multi-file import (M8 §5). Each input file is processed independently and
    /// reported in input order with a deterministic per-file status
    /// (`added` / `duplicate` / `duplicate_in_batch` / `unsupported` / `failed`);
    /// one bad file never aborts the batch. `duplicate` means the content was
    /// already a project material BEFORE this batch; `duplicate_in_batch` means
    /// the same content appeared earlier in THIS batch (truthful copy, never
    /// conflated with "already in the project"). Sources are only ever read;
    /// originals are never modified. Dedup uses content SHA-256.
    pub fn import_materials(
        &self,
        project_id: &str,
        paths: Vec<String>,
    ) -> AppResult<MaterialsImportReport> {
        let pid = parse_project_id(project_id)?;
        let pre_existing = self.project_material_hashes(&pid)?;
        let mut items = Vec::with_capacity(paths.len());
        let mut accepted = Vec::new();
        for path in paths {
            let item = self.import_one_material(&pid, &path, &pre_existing, false)?;
            if item.status == "added"
                && let Some(material) = &item.material
            {
                accepted.push(material.id.clone());
            }
            items.push(item);
        }
        self.index_accepted_material_batch(&pid, &accepted, None, false);
        Ok(MaterialsImportReport { items })
    }

    /// Commits a staged selection as one logical Material batch. Material
    /// storage happens before any Knowledge work; a later derivation failure
    /// never removes an accepted source. This is called only from the send
    /// acceptance path, never from drag/drop or file selection.
    fn accept_staged_materials(
        &self,
        pid: &ProjectId,
        paths: &[String],
        images: &[StagedImage],
        operation_id: Option<&str>,
    ) -> AppResult<MaterialsImportReport> {
        let pre_existing = self.project_material_hashes(pid)?;
        let mut items = Vec::with_capacity(paths.len());
        let mut accepted = Vec::new();
        for path in paths {
            let item = self.import_one_material(pid, path, &pre_existing, false)?;
            if item.status == "added"
                && let Some(material) = item.material.clone()
            {
                accepted.push(material.id);
            }
            items.push(item);
        }
        for image in images {
            let item = self.import_staged_image(pid, image, &pre_existing)?;
            if item.status == "added"
                && let Some(material) = item.material.clone()
            {
                accepted.push(material.id);
            }
            items.push(item);
        }
        if let Some(operation_id) = operation_id
            && let Ok(mut store) =
                KnowledgeStore::open(self.base.join("projects").join(pid.as_str()), pid)
        {
            for id in &accepted {
                if let Ok(material_id) = MaterialId::parse(id) {
                    let _ = store.bind_accepted_import_material(operation_id, &material_id);
                }
            }
            // The `copied` (prepared) counter is deliberately NOT advanced here.
            // `prepared > 0` must never be durable before the owning user turn is
            // persisted, so the caller publishes the first `copied > 0` state
            // together with the turn link in one ledger update.
        }
        Ok(MaterialsImportReport { items })
    }

    /// Converts a renderer-owned pasted image into the same accepted Material
    /// store used by staged paths. This is called only after the operation
    /// ledger exists; clipboard bytes cannot create an eager Material.
    fn import_staged_image(
        &self,
        pid: &ProjectId,
        image: &StagedImage,
        pre_existing: &std::collections::HashSet<String>,
    ) -> AppResult<MaterialImportResult> {
        // The id is intentionally opaque and is not persisted or logged. Its
        // presence prevents a malformed caller from treating an unnamed byte
        // payload as a project attachment.
        if image.staging_id.trim().is_empty() {
            return Err(AppError::invalid("Esa imagen no es válida."));
        }
        let validated =
            validate_clipboard_image(&image.file_name, &image.content_type, &image.bytes)?;
        let source_name = validated.synthesized_name.clone();
        let sha = sha256_hex(&image.bytes);
        if let Some(existing) = self.find_material_by_sha(pid, &sha)? {
            let (status, reason) = if pre_existing.contains(&sha) {
                ("duplicate", "Ese archivo ya está en el proyecto.")
            } else {
                (
                    "duplicate_in_batch",
                    "Ese archivo ya estaba en esta selección.",
                )
            };
            return Ok(MaterialImportResult {
                source_name,
                status: status.to_owned(),
                material_id: Some(existing.id.as_str().to_owned()),
                reason: Some(reason.to_owned()),
                material: Some(material_view(&existing)),
            });
        }
        let request = AddMaterial {
            display_name: "Captura".to_owned(),
            original_file_name: validated.synthesized_name,
            content_type: Some(ContentType::parse(validated.content_type).expect("validated type")),
            source: MaterialContent {
                bytes: image.bytes.clone(),
            },
        };
        let material = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_material(pid, request)
            .map_err(AppError::from_material)?;
        Ok(MaterialImportResult {
            source_name,
            status: "added".to_owned(),
            material_id: Some(material.id.as_str().to_owned()),
            reason: None,
            material: Some(material_view(&material)),
        })
    }

    /// Validates a prospective composer selection without accepting it. This
    /// read-only boundary is the only file inspection allowed before Send: it
    /// creates no Material, copies no source into the project, opens no
    /// Knowledge store, and never initializes the embedding provider.
    pub fn stage_attachment_paths(&self, paths: &[String]) -> StagedAttachmentsReport {
        let mut seen = std::collections::HashSet::new();
        let items = paths
            .iter()
            .map(|path| {
                let source_name = Path::new(path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(project_core::safe_file_name)
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| "archivo".to_owned());
                match std::fs::symlink_metadata(path) {
                    Ok(meta) if !meta.file_type().is_file() => StagedAttachmentView {
                        source_name,
                        status: "unsupported".to_owned(),
                        reason: Some("Ese archivo no es válido.".to_owned()),
                    },
                    Ok(meta) if meta.len() > MAX_IMPORT_FILE_BYTES => StagedAttachmentView {
                        source_name,
                        status: "unsupported".to_owned(),
                        reason: Some("Ese archivo es demasiado grande.".to_owned()),
                    },
                    Err(_) => StagedAttachmentView {
                        source_name,
                        status: "failed".to_owned(),
                        reason: Some("No pudimos preparar ese archivo.".to_owned()),
                    },
                    Ok(_) => match read_source_file(Path::new(path)) {
                        Ok((_, bytes, _)) => {
                            let hash = sha256_hex(&bytes);
                            let duplicate = !seen.insert(hash);
                            StagedAttachmentView {
                                source_name,
                                status: if duplicate {
                                    "duplicate_in_selection".to_owned()
                                } else {
                                    "ready".to_owned()
                                },
                                reason: duplicate
                                    .then(|| "Ese archivo ya estaba en esta selección.".to_owned()),
                            }
                        }
                        Err(_) => StagedAttachmentView {
                            source_name,
                            status: "failed".to_owned(),
                            reason: Some("No pudimos preparar ese archivo.".to_owned()),
                        },
                    },
                }
            })
            .collect();
        StagedAttachmentsReport { items }
    }

    fn project_material_hashes(
        &self,
        pid: &ProjectId,
    ) -> AppResult<std::collections::HashSet<String>> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(pid)
            .map_err(AppError::from_core)?;
        Ok(project
            .materials
            .iter()
            .map(|m| m.sha256.as_str().to_owned())
            .collect())
    }

    fn import_one_material(
        &self,
        pid: &ProjectId,
        path: &str,
        pre_existing: &std::collections::HashSet<String>,
        index_immediately: bool,
    ) -> AppResult<MaterialImportResult> {
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
                        // Truthful cause classification: a hash present before
                        // this batch is "already in the project"; a hash we
                        // added earlier in THIS batch is a same-batch duplicate.
                        let (status, reason) = if pre_existing.contains(&sha) {
                            (
                                "duplicate",
                                "Ese archivo ya está en el proyecto.".to_owned(),
                            )
                        } else {
                            (
                                "duplicate_in_batch",
                                "Ese archivo ya estaba en esta selección.".to_owned(),
                            )
                        };
                        return Ok(MaterialImportResult {
                            source_name,
                            status: status.to_owned(),
                            material_id: Some(existing.id.as_str().to_owned()),
                            reason: Some(reason),
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
                let persisted = if index_immediately {
                    self.add_material_and_index(pid, request)
                } else {
                    self.projects
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .add_material(pid, request)
                        .map_err(AppError::from_material)
                };
                match persisted {
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
            .map_err(AppError::from_material)?;
        self.remove_knowledge_material(&pid, &mid);
        Ok(())
    }

    /// Stores an accepted Material before attempting the independent local
    /// Knowledge derivation. A Knowledge failure never rolls back or rejects
    /// the user's accepted source Material.
    fn add_material_and_index(&self, pid: &ProjectId, request: AddMaterial) -> AppResult<Material> {
        let bytes = request.source.bytes.clone();
        let material = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .add_material(pid, request)
            .map_err(AppError::from_material)?;
        self.index_material_locally(pid, &material, &bytes);
        Ok(material)
    }

    fn index_material_locally(&self, project_id: &ProjectId, material: &Material, bytes: &[u8]) {
        let root = self.base.join("projects").join(project_id.as_str());
        let mut store = match KnowledgeStore::open(&root, project_id) {
            Ok(store) => store,
            Err(_) => {
                crate::session_log::record(
                    "WARN",
                    format!("[knowledge] index_open_failed material_id={}", material.id),
                );
                return;
            }
        };
        let source = material_source(material);
        if store.index(&source, bytes).is_err() {
            crate::session_log::record(
                "WARN",
                format!("[knowledge] index_failed material_id={}", material.id),
            );
            return;
        }
        let (embedded, reused, failure_state) =
            match self.with_local_embedding_provider(|provider| {
                #[cfg(test)]
                {
                    self.test_activity
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .embedding_persistence += 1;
                }
                store.index_embeddings(provider, 8)
            }) {
                (Some(Ok(outcome)), _) => (Some(outcome.embedded), Some(outcome.reused), None),
                (Some(Err(error)), _) => (None, None, Some(embedding_index_failure_state(&error))),
                (None, state) => (None, None, Some(state)),
            };
        if embedded.is_none() {
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge] embedding_index_failed material_id={} failure_class={}",
                    material.id,
                    failure_state
                        .unwrap_or(SemanticProviderState::OtherTypedLocalFailure)
                        .as_str(),
                ),
            );
        } else {
            crate::session_log::record(
                "DEBUG",
                format!(
                    "[knowledge] indexed material_id={} embeddings_created={} embeddings_reused={}",
                    material.id,
                    embedded.unwrap_or(0),
                    reused.unwrap_or(0)
                ),
            );
        }
    }

    fn index_accepted_material_batch(
        &self,
        project_id: &ProjectId,
        material_ids: &[String],
        operation_id: Option<&str>,
        recovering: bool,
    ) {
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .indexing += 1;
        }
        if material_ids.is_empty() {
            return;
        }
        let root = self.base.join("projects").join(project_id.as_str());
        let mut store = match KnowledgeStore::open(&root, project_id) {
            Ok(store) => store,
            Err(_) => {
                crate::session_log::record(
                    "WARN",
                    "[knowledge] batch_index_open_failed".to_owned(),
                );
                return;
            }
        };
        let project = match self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
        {
            Ok(project) => project,
            Err(_) => return,
        };
        if let Some(operation_id) = operation_id {
            let _ = store.update_accepted_import_operation(
                operation_id,
                None,
                project_knowledge::AcceptedImportState::IndexingLexical,
                material_ids.len(),
                0,
                0,
                0,
                0,
                0,
            );
        }
        let mut indexed = 0usize;
        let mut failed = 0usize;
        for id in material_ids {
            let Some(material) = project
                .materials
                .iter()
                .find(|material| material.id.as_str() == id)
            else {
                continue;
            };
            let bytes = match self.content.read_material(project_id, material) {
                Ok(bytes) => bytes,
                Err(_) => continue,
            };
            if store.index(&material_source(material), &bytes).is_ok() {
                indexed += 1;
            } else {
                failed += 1;
            }
        }
        if recovering && let Some(operation_id) = operation_id {
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge][recovery] lexical_progress operation_id={operation_id} completed={indexed} total={}",
                    material_ids.len()
                ),
            );
        }
        let material_ids = material_ids
            .iter()
            .filter_map(|id| MaterialId::parse(id).ok())
            .collect::<Vec<_>>();
        if let Some(operation_id) = operation_id {
            let _ = store.update_accepted_import_operation(
                operation_id,
                None,
                project_knowledge::AcceptedImportState::IndexingEmbeddings,
                material_ids.len(),
                indexed,
                0,
                failed,
                0,
                0,
            );
        }
        let (embedded, reused, failure_state, persist_failure) = match self
            .with_local_embedding_provider(|provider| {
                #[cfg(test)]
                {
                    self.test_activity
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .embedding_persistence += 1;
                }
                store.index_embeddings_for_materials(provider, 8, &material_ids)
            }) {
            (Some(Ok(outcome)), _) => (Some(outcome.embedded), Some(outcome.reused), None, None),
            (Some(Err(error)), _) => {
                let state = embedding_index_failure_state(&error);
                let detail = error.embedding_persist_class("persist");
                (None, None, Some(state), Some(detail))
            }
            (None, state) => (None, None, Some(state), None),
        };
        crate::session_log::record(
            if embedded.is_some() { "INFO" } else { "WARN" },
            format!(
                "[knowledge] operation=material_batch materials_total={} lexical_ready={} embeddings_created={} embeddings_reused={} embedding_failure_class={}",
                material_ids.len(),
                indexed,
                embedded.unwrap_or(0),
                reused.unwrap_or(0),
                failure_state.map(|state| state.as_str()).unwrap_or("none")
            ),
        );
        if let Some(failure) = persist_failure {
            // Sanitized stage/class + SQLite primary code only: never SQL text,
            // a path, a vector, or a document body.
            let sqlite_code = failure
                .sqlite_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "none".to_owned());
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge][embedding] stage={} failure_class={} sqlite_code={}",
                    failure.stage,
                    failure.class.as_str(),
                    sqlite_code
                ),
            );
        }
        if recovering && let Some(operation_id) = operation_id {
            crate::session_log::record(
                if embedded.is_some() { "INFO" } else { "WARN" },
                format!(
                    "[knowledge][recovery] embedding_progress operation_id={operation_id} created={} reused={}",
                    embedded.unwrap_or(0),
                    reused.unwrap_or(0)
                ),
            );
        }
        if let Some(operation_id) = operation_id {
            let embedding_completed = if embedded.is_some() { indexed } else { 0 };
            let final_failed = failed + usize::from(embedded.is_none() && indexed > 0);
            // Indexing completion is not operation completion: the same
            // accepted turn still has its bounded retrieval/agent step. Keep
            // the ledger incomplete until that terminal step succeeds so a
            // restart can never claim a fake completed turn.
            let state = if final_failed == 0 {
                project_knowledge::AcceptedImportState::IndexingEmbeddings
            } else {
                project_knowledge::AcceptedImportState::PendingRetry
            };
            let _ = store.update_accepted_import_operation(
                operation_id,
                None,
                state,
                material_ids.len(),
                indexed,
                embedding_completed,
                final_failed,
                embedded.unwrap_or(0),
                reused.unwrap_or(0),
            );
        }
    }

    fn remove_knowledge_material(&self, project_id: &ProjectId, material_id: &MaterialId) {
        let root = self.base.join("projects").join(project_id.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return;
        }
        if KnowledgeStore::open(&root, project_id)
            .and_then(|mut store| store.remove(material_id))
            .is_err()
        {
            crate::session_log::record(
                "WARN",
                format!("[knowledge] remove_failed material_id={material_id}"),
            );
        }
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

    /// K5 activation is deterministic: an existing project-local index runs
    /// K3 then K4 against this current user turn. No remote call is made.
    fn prepare_knowledge_context(
        &self,
        project_id: &ProjectId,
        user_query: &str,
    ) -> AppResult<(Option<AgentKnowledgeContext>, Option<TurnKnowledgeMetrics>)> {
        let root = self.base.join("projects").join(project_id.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return Ok((None, None));
        }
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retrieval += 1;
        }
        let started = std::time::Instant::now();
        let store = KnowledgeStore::open(&root, project_id).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el material de apoyo.",
            )
        })?;
        let corpus = store
            .corpus_stats()
            .unwrap_or_else(|_| project_knowledge::KnowledgeCorpusStats::default());

        if crate::retrieval_intent::detect_retrieval_intent(user_query)
            == crate::RetrievalIntent::CorpusExhaustive
        {
            return self.prepare_exhaustive_knowledge_context(
                project_id, user_query, &store, &corpus, started,
            );
        }

        let (search, mut semantic_state) = self.with_local_embedding_provider(|provider| {
            store.hybrid_search_metrics(user_query, Some(provider), HybridSearchOptions::default())
        });
        let (candidates, metrics) = match search {
            Some(Ok((results, metrics))) => (results, metrics),
            Some(Err(_)) => {
                semantic_state = project_knowledge::SemanticProviderState::SemanticQueryFailed;
                crate::session_log::record(
                    "WARN",
                    "[knowledge] semantic_query_failed fallback=lexical",
                );
                store
                    .hybrid_search_metrics(user_query, None, HybridSearchOptions::default())
                    .map_err(|_| {
                        AppError::new(
                            ErrorCode::Internal,
                            "No pudimos buscar el material de apoyo.",
                        )
                    })?
            }
            None => store
                .hybrid_search_metrics(user_query, None, HybridSearchOptions::default())
                .map_err(|_| {
                    AppError::new(
                        ErrorCode::Internal,
                        "No pudimos buscar el material de apoyo.",
                    )
                })?,
        };
        let package = store
            .assemble_context(user_query, &candidates, ContextAssemblyOptions::default())
            .map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos preparar el material de apoyo.",
                )
            })?;
        let evidence_bytes: usize = package.entries.iter().map(|entry| entry.text.len()).sum();
        let evidence_utf8_chars: usize = package
            .entries
            .iter()
            .map(|entry| entry.text.chars().count())
            .sum();
        let evidence_est_tokens = package.totals.estimated_budget_used;
        let naive_corpus_est_tokens = corpus.naive_corpus_est_tokens;
        let context_reduction_pct_vs_naive_corpus = if naive_corpus_est_tokens == 0 {
            100
        } else {
            let saved = naive_corpus_est_tokens.saturating_sub(evidence_est_tokens);
            saved * 100 / naive_corpus_est_tokens
        };
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: naive_corpus_est_tokens,
            retrieval_candidate_count: Some(candidates.len()),
            selected_evidence_count: Some(package.entries.len()),
            selected_evidence_bytes: Some(evidence_bytes),
            selected_evidence_utf8_chars: Some(evidence_utf8_chars),
            evidence_est_tokens: Some(evidence_est_tokens),
            context_reduction_pct: Some(context_reduction_pct_vs_naive_corpus),
            semantic_provider_state: semantic_state.as_str().to_owned(),
            request_preparation_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            retrieval_mode: Some(RetrievalMode::Normal.as_str().to_owned()),
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            lexical_hits: None,
            semantic_hits: None,
        };
        crate::session_log::record_knowledge(
            crate::session_log::SessionKnowledgeMetrics {
                conversation_id: project_id.as_str().to_owned(),
                material_count: knowledge_metrics.material_count,
                corpus_bytes: knowledge_metrics.corpus_bytes,
                corpus_utf8_chars: knowledge_metrics.corpus_utf8_chars,
                corpus_est_tokens: knowledge_metrics.corpus_est_tokens,
                retrieval_candidate_count: knowledge_metrics.retrieval_candidate_count,
                selected_evidence_count: knowledge_metrics.selected_evidence_count,
                selected_evidence_bytes: knowledge_metrics.selected_evidence_bytes,
                selected_evidence_utf8_chars: knowledge_metrics.selected_evidence_utf8_chars,
                evidence_est_tokens: knowledge_metrics.evidence_est_tokens,
                context_reduction_pct: knowledge_metrics.context_reduction_pct,
                semantic_provider_state: knowledge_metrics.semantic_provider_state.clone(),
                request_preparation_ms: knowledge_metrics.request_preparation_ms.map(u128::from),
                retrieval_mode: knowledge_metrics.retrieval_mode.clone(),
                eligible_materials: knowledge_metrics.eligible_materials,
                materials_inspected: knowledge_metrics.materials_inspected,
                chunks_inspected: knowledge_metrics.chunks_inspected,
                exhaustive_coverage: knowledge_metrics.exhaustive_coverage.clone(),
                lexical_hits: knowledge_metrics.lexical_hits,
                semantic_hits: knowledge_metrics.semantic_hits,
            },
            format!(
                "[knowledge] retrieval mode={} candidates={} evidence={} budget_used={} budget_limit={} semantic={} semantic_state={} lexical_candidates={} semantic_candidates={} fused_candidates={} evidence_bytes={} evidence_utf8_chars={} evidence_est_tokens={} naive_corpus_est_tokens={} context_reduction_pct_vs_naive_corpus={} knowledge_materials={} knowledge_ready={} knowledge_failed={} knowledge_unsupported={} knowledge_pending={} chunks_total={} embeddings_ready={} corpus_bytes={} corpus_utf8_chars={} request_preparation_ms={}",
                if metrics.semantic_availability
                    == project_knowledge::SemanticAvailability::Available
                {
                    "hybrid"
                } else {
                    "lexical"
                },
                candidates.len(),
                package.entries.len(),
                package.totals.estimated_budget_used,
                package.totals.estimated_budget_limit,
                format!("{:?}", metrics.semantic_availability).to_ascii_lowercase(),
                semantic_state.as_str(),
                metrics.lexical_candidates,
                metrics.semantic_candidates,
                metrics.fused_candidates,
                evidence_bytes,
                evidence_utf8_chars,
                evidence_est_tokens,
                naive_corpus_est_tokens,
                context_reduction_pct_vs_naive_corpus,
                corpus.material_count,
                corpus.ready,
                corpus.failed,
                corpus.unsupported,
                corpus.pending,
                corpus.chunks_total,
                corpus.embeddings_ready,
                corpus.corpus_bytes,
                corpus.corpus_utf8_chars,
                started.elapsed().as_millis()
            ),
        );
        // The attachment-provisioner dedup contract keys off EVERY durably
        // READY-indexed source name, not just the small set this turn retrieved.
        // A supported indexed TXT/Markdown the user attaches must be served
        // through bounded Knowledge retrieval, never raw-forwarded as a full
        // workspace attachment — even when this specific query retrieved zero
        // evidence. Unsupported/media attachments are never `ready`, so they
        // keep their raw-forwarding path.
        let indexed_source_names: Vec<String> = store
            .ready_source_names()
            .unwrap_or_default()
            .into_iter()
            .map(|name| project_core::safe_file_name(&name))
            .collect();
        if package.entries.is_empty() {
            // No evidence for this turn, but the dedup contract still applies.
            if indexed_source_names.is_empty() {
                return Ok((None, Some(knowledge_metrics)));
            }
            return Ok((
                Some(AgentKnowledgeContext {
                    indexed_source_names,
                    entries: Vec::new(),
                    evidence_budget_used: 0,
                    evidence_budget_limit: package.totals.estimated_budget_limit,
                    citation_map: Vec::new(),
                    retrieval_mode: Some(RetrievalMode::Normal.as_str().to_owned()),
                    exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
                    structural_note: None,
                    local_answer: None,
                    authorize_negative: false,
                    citation_source_names: Vec::new(),
                }),
                Some(knowledge_metrics),
            ));
        }
        let entries: Vec<AgentKnowledgeEntry> = package
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| AgentKnowledgeEntry {
                label: format!("E{}", index + 1),
                source_label: format!("S{}", index + 1),
                source_name: project_core::safe_file_name(&entry.source_name),
                chunk_label: format!("C{}", index + 1),
                line_start: Some(entry.provenance.start_line),
                line_end: Some(entry.provenance.end_line),
                heading_path: entry.heading_path.clone(),
                text: entry.text.clone(),
                source_id: Some(entry.source_id.clone()),
                evidence_kind: evidence_kind_label(&entry.signals),
            })
            .collect();
        let citation_map = entries
            .iter()
            .map(|entry| AgentEvidenceProvenance {
                label: entry.label.clone(),
                source_label: entry.source_label.clone(),
                chunk_label: entry.chunk_label.clone(),
            })
            .collect();
        Ok((
            Some(AgentKnowledgeContext {
                indexed_source_names,
                entries,
                evidence_budget_used: package.totals.estimated_budget_used,
                evidence_budget_limit: package.totals.estimated_budget_limit,
                citation_map,
                retrieval_mode: Some(RetrievalMode::Normal.as_str().to_owned()),
                exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
                structural_note: None,
                local_answer: None,
                authorize_negative: false,
                citation_source_names: Vec::new(),
            }),
            Some(knowledge_metrics),
        ))
    }

    fn prepare_exhaustive_knowledge_context(
        &self,
        project_id: &ProjectId,
        user_query: &str,
        store: &KnowledgeStore,
        corpus: &project_knowledge::KnowledgeCorpusStats,
        started: std::time::Instant,
    ) -> AppResult<(Option<AgentKnowledgeContext>, Option<TurnKnowledgeMetrics>)> {
        let terms = crate::extract_presence_terms(user_query);
        let (search, semantic_state) = self.with_local_embedding_provider(|provider| {
            store.exhaustive_presence_search(user_query, &terms, Some(provider))
        });
        let report = match search {
            Some(Ok(report)) => report,
            Some(Err(_)) | None => store
                .exhaustive_presence_search(user_query, &terms, None)
                .map_err(|_| {
                    AppError::new(
                        ErrorCode::Internal,
                        "No pudimos buscar el material de apoyo.",
                    )
                })?,
        };
        let package = store
            .assemble_context(
                user_query,
                &report.candidates,
                ContextAssemblyOptions::default(),
            )
            .map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos preparar el material de apoyo.",
                )
            })?;
        let evidence_bytes: usize = package.entries.iter().map(|entry| entry.text.len()).sum();
        let evidence_utf8_chars: usize = package
            .entries
            .iter()
            .map(|entry| entry.text.chars().count())
            .sum();
        let evidence_est_tokens = package.totals.estimated_budget_used;
        let naive_corpus_est_tokens = corpus.naive_corpus_est_tokens;
        let context_reduction_pct_vs_naive_corpus = if naive_corpus_est_tokens == 0 {
            100
        } else {
            let saved = naive_corpus_est_tokens.saturating_sub(evidence_est_tokens);
            saved * 100 / naive_corpus_est_tokens
        };
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: naive_corpus_est_tokens,
            retrieval_candidate_count: Some(report.candidates.len()),
            selected_evidence_count: Some(package.entries.len()),
            selected_evidence_bytes: Some(evidence_bytes),
            selected_evidence_utf8_chars: Some(evidence_utf8_chars),
            evidence_est_tokens: Some(evidence_est_tokens),
            context_reduction_pct: Some(context_reduction_pct_vs_naive_corpus),
            semantic_provider_state: semantic_state.as_str().to_owned(),
            request_preparation_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            retrieval_mode: Some(RetrievalMode::Exhaustive.as_str().to_owned()),
            eligible_materials: Some(report.eligible_materials),
            materials_inspected: Some(report.materials_inspected),
            chunks_inspected: Some(report.chunks_inspected),
            exhaustive_coverage: Some(report.coverage.as_str().to_owned()),
            lexical_hits: Some(report.lexical_hits),
            semantic_hits: Some(report.semantic_hits),
        };
        crate::session_log::record_knowledge(
            crate::session_log::SessionKnowledgeMetrics {
                conversation_id: project_id.as_str().to_owned(),
                material_count: knowledge_metrics.material_count,
                corpus_bytes: knowledge_metrics.corpus_bytes,
                corpus_utf8_chars: knowledge_metrics.corpus_utf8_chars,
                corpus_est_tokens: knowledge_metrics.corpus_est_tokens,
                retrieval_candidate_count: knowledge_metrics.retrieval_candidate_count,
                selected_evidence_count: knowledge_metrics.selected_evidence_count,
                selected_evidence_bytes: knowledge_metrics.selected_evidence_bytes,
                selected_evidence_utf8_chars: knowledge_metrics.selected_evidence_utf8_chars,
                evidence_est_tokens: knowledge_metrics.evidence_est_tokens,
                context_reduction_pct: knowledge_metrics.context_reduction_pct,
                semantic_provider_state: knowledge_metrics.semantic_provider_state.clone(),
                request_preparation_ms: knowledge_metrics.request_preparation_ms.map(u128::from),
                retrieval_mode: knowledge_metrics.retrieval_mode.clone(),
                eligible_materials: knowledge_metrics.eligible_materials,
                materials_inspected: knowledge_metrics.materials_inspected,
                chunks_inspected: knowledge_metrics.chunks_inspected,
                exhaustive_coverage: knowledge_metrics.exhaustive_coverage.clone(),
                lexical_hits: knowledge_metrics.lexical_hits,
                semantic_hits: knowledge_metrics.semantic_hits,
            },
            format!(
                "[knowledge] retrieval mode=exhaustive coverage={} eligible_materials={} materials_inspected={} chunks_inspected={} lexical_hits={} semantic_hits={} candidates={} evidence={} evidence_est_tokens={} naive_corpus_est_tokens={} context_reduction_pct_vs_naive_corpus={} semantic_state={}",
                report.coverage.as_str(),
                report.eligible_materials,
                report.materials_inspected,
                report.chunks_inspected,
                report.lexical_hits,
                report.semantic_hits,
                report.candidates.len(),
                package.entries.len(),
                evidence_est_tokens,
                naive_corpus_est_tokens,
                context_reduction_pct_vs_naive_corpus,
                semantic_state.as_str()
            ),
        );
        let indexed_source_names: Vec<String> = store
            .ready_source_names()
            .unwrap_or_default()
            .into_iter()
            .map(|name| project_core::safe_file_name(&name))
            .collect();
        let entries: Vec<AgentKnowledgeEntry> = package
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| AgentKnowledgeEntry {
                label: format!("E{}", index + 1),
                source_label: format!("S{}", index + 1),
                source_name: project_core::safe_file_name(&entry.source_name),
                chunk_label: format!("C{}", index + 1),
                line_start: Some(entry.provenance.start_line),
                line_end: Some(entry.provenance.end_line),
                heading_path: entry.heading_path.clone(),
                text: entry.text.clone(),
                source_id: Some(entry.source_id.clone()),
                evidence_kind: evidence_kind_label(&entry.signals),
            })
            .collect();
        let citation_map = entries
            .iter()
            .map(|entry| AgentEvidenceProvenance {
                label: entry.label.clone(),
                source_label: entry.source_label.clone(),
                chunk_label: entry.chunk_label.clone(),
            })
            .collect();
        let structural_note = Some(format!(
            "exhaustive_coverage={} eligible_materials={} materials_inspected={} chunks_inspected={} lexical_hits={} semantic_hits={} matching_sources={}",
            report.coverage.as_str(),
            report.eligible_materials,
            report.materials_inspected,
            report.chunks_inspected,
            report.lexical_hits,
            report.semantic_hits,
            report.matching_source_names.len()
        ));
        let local_answer = exhaustive_local_answer(&report, &terms);
        if indexed_source_names.is_empty() && entries.is_empty() && local_answer.is_none() {
            return Ok((None, Some(knowledge_metrics)));
        }
        Ok((
            Some(AgentKnowledgeContext {
                indexed_source_names,
                entries,
                evidence_budget_used: package.totals.estimated_budget_used,
                evidence_budget_limit: package.totals.estimated_budget_limit,
                citation_map,
                retrieval_mode: Some(RetrievalMode::Exhaustive.as_str().to_owned()),
                exhaustive_coverage: Some(report.coverage.as_str().to_owned()),
                structural_note,
                local_answer,
                authorize_negative: report.coverage == ExhaustiveCoverage::Complete
                    && report.lexical_hits == 0
                    && !terms.is_empty(),
                citation_source_names: report.matching_source_names.clone(),
            }),
            Some(knowledge_metrics),
        ))
    }

    /// Runs an operation against the verified, bundled local E5 provider when
    /// it is available. Missing/corrupt model data or a missing runtime is an
    /// expected lexical-only state; this function never installs or contacts a
    /// remote provider. Returns the sanitized provider state alongside the
    /// operation result so callers can log a structural cause, not an exception
    /// body.
    fn with_local_embedding_provider<Output>(
        &self,
        operation: impl FnOnce(&mut dyn EmbeddingProvider) -> project_knowledge::Result<Output>,
    ) -> (
        Option<project_knowledge::Result<Output>>,
        project_knowledge::SemanticProviderState,
    ) {
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .embedding_inference += 1;
        }
        let mut provider = self
            .knowledge_provider
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if provider.is_none() {
            let (loaded, state) = self.load_local_embedding_provider();
            *provider = loaded;
            if provider.is_none() {
                return (None, state);
            }
        }
        (
            provider
                .as_mut()
                .map(|provider| operation(provider as &mut dyn EmbeddingProvider)),
            project_knowledge::SemanticProviderState::Available,
        )
    }

    /// Probes the local E5 provider and reports a sanitized cause when it is
    /// unavailable. Distinct structural codes (model install state, runtime
    /// resolution/load, tokenizer, session build) are produced without ever
    /// exposing a path or a provider error body.
    fn load_local_embedding_provider(
        &self,
    ) -> (
        Option<OrtEmbeddingProvider>,
        project_knowledge::SemanticProviderState,
    ) {
        use project_knowledge::SemanticProviderState as State;
        let manager = match ModelManager::new(&self.base) {
            Ok(manager) => manager,
            Err(_) => return (None, State::OtherTypedLocalFailure),
        };
        let model_root = match manager.inspect() {
            Ok(ModelInstallState::Verified(path)) => path,
            Ok(ModelInstallState::NotInstalled) => return (None, State::ModelNotInstalled),
            Ok(ModelInstallState::Incomplete) => return (None, State::ModelIncomplete),
            Ok(ModelInstallState::Corrupt(_)) => return (None, State::ModelCorrupt),
            Err(_) => return (None, State::OtherTypedLocalFailure),
        };
        let executable = match std::env::current_exe() {
            Ok(executable) => executable,
            Err(_) => return (None, State::OtherTypedLocalFailure),
        };
        let runtime = match runtime_library_from_executable(&executable) {
            Ok(runtime) => runtime,
            Err(_) => return (None, State::RuntimeNotFound),
        };
        let tokenizer = match manager.load_tokenizer() {
            Ok(tokenizer) => tokenizer,
            Err(_) => return (None, State::TokenizerLoadFailed),
        };
        match OrtEmbeddingProvider::load(
            manager.generation(),
            tokenizer,
            &model_root.join("onnx/model.onnx"),
            &runtime,
        ) {
            Ok(provider) => (Some(provider), State::Available),
            Err(project_knowledge::OrtProviderLoadError::RuntimeInitFailed) => {
                (None, State::RuntimeLoadFailed)
            }
            Err(project_knowledge::OrtProviderLoadError::SessionBuildFailed)
            | Err(project_knowledge::OrtProviderLoadError::InputContractMismatch) => {
                (None, State::ProviderInitializationFailed)
            }
        }
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
        if crate::summarize::detect_summary_intent(prompt, attachment_ids.len())
            != crate::summarize::SummaryIntent::None
            && self.summarizer_backend.is_some()
        {
            return self.send_summary_run(inputs);
        }
        self.send_message_run(inputs)
    }

    /// Whole-project summary turn (K6): the user message is already persisted by
    /// [`Self::send_message_persist`]; this runs the bounded hierarchical
    /// summarization (never sending the corpus) and appends the user-facing
    /// global summary as the assistant message. It never creates a normal agent
    /// run or a scratch chat turn.
    pub fn send_summary_run(&self, inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        use crate::summarize::OpenCodeRemoteSummarizer;
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let mut summarizer =
            OpenCodeRemoteSummarizer::new(Arc::clone(backend), self.base.join("opencode-scratch"));
        if let Some(model) = &inputs.model {
            summarizer = summarizer.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        self.send_summary_run_with(inputs, &summarizer)
    }

    /// Shared K6 terminal lifecycle. Production and deterministic tests use
    /// this same assistant/metrics persistence path.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn send_summary_run_with(
        &self,
        inputs: AgentRunInputs,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<AgentRunView> {
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .k6_calls += 1;
        }
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let durable_turn_id = inputs.turn_id.clone();
        let inputs_model = inputs.model.clone();
        let started = std::time::Instant::now();
        crate::session_log::record(
            "INFO",
            format!(
                "summary turn started conversation_id={} turn_id={}",
                project_id,
                turn_id.as_deref().unwrap_or("none")
            ),
        );
        let summary_intent = crate::summarize::detect_summary_intent(
            &inputs.prompt,
            inputs.selected_material_ids.len(),
        );
        let selected_per_source =
            summary_intent == crate::summarize::SummaryIntent::SelectedPerSource;
        let answer = match if selected_per_source {
            self.summarize_selected_sources(
                project_id.as_str(),
                &inputs.selected_material_ids,
                summarizer,
            )
        } else {
            self.summarize_project_with(project_id.as_str(), summarizer)
        } {
            Ok(answer) => answer,
            Err(error) => {
                let text = error.message.clone();
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(&project_id, &text, MessageStatus::Failed, &[])
                    .map_err(AppError::from_core)?;
                crate::session_log::record(
                    "ERROR",
                    format!(
                        "summary turn terminal conversation_id={} turn_id={} status=failed failure_class=knowledge_failure failure_stage=summary duration_ms={}",
                        project_id,
                        turn_id.as_deref().unwrap_or("none"),
                        started.elapsed().as_millis()
                    ),
                );
                return Ok(AgentRunView {
                    status: "failed".to_owned(),
                    turn_id,
                    registered_creation_ids: Vec::new(),
                    message: Some(text),
                });
            }
        };
        let Some(surface) = answer.summarize_surface_text() else {
            // A zero-source K6 turn is still a completed logical turn. Persist
            // its truthful aggregate (normally zero remote calls) so restart
            // does not fall back to process-local telemetry.
            self.persist_completed_turn_metrics(
                &project_id,
                durable_turn_id.as_ref(),
                completed_summary_turn_metrics_without_corpus(
                    inputs_model.as_ref(),
                    &answer.report,
                    started.elapsed().as_millis(),
                ),
            )?;
            crate::session_log::record(
                "INFO",
                format!(
                    "summary turn terminal conversation_id={} turn_id={} status=completed documents=0 summary_nodes_reused={} summary_cache_hits={} remote_summary_calls={} input_tokens={} output_tokens={} cache_read_tokens={} cache_write_tokens={} cost_usd={} estimated_input_units={} duration_ms={}",
                    project_id,
                    turn_id.as_deref().unwrap_or("none"),
                    answer.report.reused,
                    answer.report.cache_hits,
                    answer.report.remote_calls,
                    answer
                        .report
                        .input_tokens
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    answer
                        .report
                        .output_tokens
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    answer
                        .report
                        .cache_read_tokens
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    answer
                        .report
                        .cache_write_tokens
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    answer
                        .report
                        .cost_usd
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "unavailable".to_owned()),
                    answer.report.estimated_input_units,
                    started.elapsed().as_millis()
                ),
            );
            return Ok(AgentRunView {
                status: "completed".to_owned(),
                turn_id,
                registered_creation_ids: Vec::new(),
                message: None,
            });
        };
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(&project_id, &surface, MessageStatus::Ok, &[])
            .map_err(AppError::from_core)?;
        let duration_ms = started.elapsed().as_millis();
        record_summary_turn_usage(
            project_id.as_str(),
            turn_id.as_deref(),
            inputs_model.as_ref(),
            &answer.report,
            duration_ms,
            if selected_per_source {
                "summary_selected_sources"
            } else {
                "summary_global"
            },
        );
        // Knowledge architecture metrics survive semantic/evidence variation:
        // the corpus/index facts are emitted for the summary turn too, so the
        // Conversation Details "Knowledge" section is truthful rather than
        // uniformly "No disponible".
        let corpus = self
            .open_knowledge_store(project_id.as_str())
            .ok()
            .flatten()
            .and_then(|store| store.corpus_stats().ok());
        if let Some(corpus) = corpus {
            record_summary_knowledge_metrics(
                project_id.as_str(),
                answer.report.source_count,
                &corpus,
            );
            self.persist_completed_turn_metrics(
                &project_id,
                durable_turn_id.as_ref(),
                completed_summary_turn_metrics(
                    inputs_model.as_ref(),
                    &answer.report,
                    duration_ms,
                    &corpus,
                ),
            )?;
        } else {
            self.persist_completed_turn_metrics(
                &project_id,
                durable_turn_id.as_ref(),
                completed_summary_turn_metrics_without_corpus(
                    inputs_model.as_ref(),
                    &answer.report,
                    duration_ms,
                ),
            )?;
        }
        crate::session_log::record(
            "INFO",
            format!(
                "summary turn terminal conversation_id={} turn_id={} status=completed documents={} summary_nodes_reused={} summary_cache_hits={} summary_nodes_generated={} remote_summary_calls={} input_tokens={} output_tokens={} cache_read_tokens={} cache_write_tokens={} cost_usd={} hierarchy_depth={} estimated_input_units={} naive_corpus_est_tokens={} estimated_reduction_pct_vs_naive_full_corpus={} duration_ms={}",
                project_id,
                turn_id.as_deref().unwrap_or("none"),
                answer.report.source_count,
                answer.report.reused,
                answer.report.cache_hits,
                answer.report.regenerated,
                answer.report.remote_calls,
                answer
                    .report
                    .input_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                answer
                    .report
                    .output_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                answer
                    .report
                    .cache_read_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                answer
                    .report
                    .cache_write_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                answer
                    .report
                    .cost_usd
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "unavailable".to_owned()),
                answer.report.hierarchy_depth,
                answer.report.estimated_input_units,
                answer.naive_corpus_est_tokens,
                answer.reduction_pct_vs_naive_full_corpus(),
                started.elapsed().as_millis()
            ),
        );
        Ok(AgentRunView {
            status: "completed".to_owned(),
            turn_id,
            registered_creation_ids: Vec::new(),
            message: Some(surface),
        })
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

    /// Send acceptance boundary for paths held only by the native/frontend
    /// staging mechanism. Validation occurs before any source is accepted;
    /// once this returns successfully the resulting single user message owns
    /// every accepted Material. Knowledge failures are intentionally not part
    /// of acceptance and are reported by their durable local status/logs.
    pub fn send_staged_message_persist(
        &self,
        project_id: &str,
        prompt: &str,
        staged_paths: &[String],
        staged_images: &[StagedImage],
    ) -> AppResult<AcceptedStagedTurn> {
        if project_id.trim().is_empty() || prompt.trim().is_empty() {
            return Err(AppError::invalid("Escribí qué querés crear."));
        }
        let pid = parse_project_id(project_id)?;
        // Validate the project/model before accepting any staged source so a
        // rejected turn leaves the frontend selection intact and residue-free.
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        match project.model {
            Some(model)
                if !self.model_list()?.iter().any(|available| {
                    available.provider_id == model.provider_id
                        && available.model_id == model.model_id
                }) =>
            {
                self.selected_model_ref()?;
            }
            Some(_) => {}
            None => {
                self.selected_model_ref()?;
            }
        }
        // Validate pasted bytes before acceptance, so invalid clipboard data
        // leaves the composer selection retryable and creates no ledger,
        // project Material, Knowledge row, or history message.
        for image in staged_images {
            if image.staging_id.trim().is_empty() {
                return Err(AppError::invalid("Esa imagen no es válida."));
            }
            validate_clipboard_image(&image.file_name, &image.content_type, &image.bytes)?;
        }
        // The operation is durable before the first project-owned input copy.
        // Filesystem and SQLite cannot share one transaction, so every later
        // phase records an explicit state rather than claiming atomicity.
        let operation_id = accepted_operation_id(&pid, prompt);
        let root = self.base.join("projects").join(pid.as_str());
        KnowledgeStore::open(&root, &pid)
            .and_then(|mut store| {
                store.create_accepted_import_operation(
                    &operation_id,
                    staged_paths.len() + staged_images.len(),
                )
            })
            .map_err(|_| AppError::internal("No pudimos confirmar los materiales."))?;
        let report =
            self.accept_staged_materials(&pid, staged_paths, staged_images, Some(&operation_id))?;
        let attachment_ids = report
            .items
            .iter()
            .filter_map(|item| item.material_id.clone())
            .collect::<Vec<_>>();
        // Persist the user turn (the durable user intent required for recovery)
        // before the operation ledger can expose `prepared > 0`. The lifecycle
        // invariant is: `prepared > 0 => turn_id != NULL`. Derived Knowledge
        // work is deliberately deferred to the caller's accepted-turn pipeline,
        // allowing the UI to refresh and render durable progress without a fake
        // percentage.
        let mut inputs =
            self.resolve_agent_inputs_without_knowledge(project_id, prompt, &attachment_ids)?;
        let material_ids = attachment_ids
            .iter()
            .map(|id| parse_material_id(id))
            .collect::<AppResult<Vec<_>>>()?;
        let user_message = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_user_message(&pid, prompt, &material_ids)
            .map_err(AppError::from_core)?;
        inputs.turn_id = Some(user_message.id);
        // Every selected path remains accounted for by the operation. A
        // duplicate can legitimately bind the already durable Material; it is
        // not silently discarded merely because no new copy was needed. The
        // first durable `copied > 0` write and the turn link happen together,
        // so a crash between acceptance and this point leaves `prepared == 0`
        // and can never fabricate a recoverable operation without its turn.
        if let Ok(mut store) = KnowledgeStore::open(&root, &pid) {
            for id in &attachment_ids {
                if let Ok(material_id) = MaterialId::parse(id) {
                    let _ = store.bind_accepted_import_material(&operation_id, &material_id);
                }
            }
            let failures = report
                .items
                .iter()
                .filter(|item| item.status == "failed")
                .count();
            let terminal_without_materials = attachment_ids.is_empty();
            let _ = store.update_accepted_import_operation(
                &operation_id,
                inputs.turn_id(),
                if terminal_without_materials {
                    project_knowledge::AcceptedImportState::PendingRetry
                } else {
                    project_knowledge::AcceptedImportState::Copying
                },
                attachment_ids.len(),
                0,
                0,
                failures,
                0,
                0,
            );
        }
        Ok(AcceptedStagedTurn {
            inputs,
            operation_id,
            material_ids: attachment_ids,
        })
    }

    /// Completes the derived work for an already durable accepted turn.  This
    /// is called by the task thread, never by pre-send staging; recovery may
    /// safely leave the ledger in `pending_retry` if the process stops here.
    pub fn run_accepted_staged_turn(
        &self,
        accepted: AcceptedStagedTurn,
    ) -> AppResult<AgentRunView> {
        self.run_accepted_staged_turn_inner(accepted, false)
    }

    fn run_accepted_staged_turn_inner(
        &self,
        mut accepted: AcceptedStagedTurn,
        recovering: bool,
    ) -> AppResult<AgentRunView> {
        self.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            recovering,
        );
        // A whole-corpus or explicit single-source summarization ("resumime el
        // archivo"/"resumime todos los archivos") routes to K6, not to a normal
        // chat run. This mirrors `send_message` so an accepted staged send with
        // an attached supported document cannot both raw-forward the file AND
        // run a second normal-chat summary for the same logical request.
        let summarize_intent = crate::summarize::detect_summary_intent(
            &accepted.inputs.prompt,
            accepted.inputs.selected_material_ids.len(),
        ) != crate::summarize::SummaryIntent::None
            && self.summarizer_backend.is_some();
        if !summarize_intent {
            let (knowledge, knowledge_metrics) = match self
                .prepare_knowledge_context(&accepted.inputs.project_id, &accepted.inputs.prompt)
            {
                Ok(knowledge) => knowledge,
                Err(error) => {
                    self.finish_accepted_import_operation(
                        &accepted.inputs.project_id,
                        &accepted.operation_id,
                        false,
                    );
                    return Err(error);
                }
            };
            accepted.inputs.knowledge = knowledge;
            accepted.inputs.knowledge_metrics = knowledge_metrics;
        }
        let project_id = accepted.inputs.project_id.clone();
        // The durable ledger now says that the remote terminal outcome is
        // unknown. A process loss after this point must never auto-resend a
        // possibly completed provider request.
        self.update_accepted_import_agent_state(
            &project_id,
            &accepted.operation_id,
            project_knowledge::AcceptedImportAgentState::StartedOutcomeUnknown,
        );
        let run = if summarize_intent {
            self.send_summary_run(accepted.inputs)
        } else {
            self.send_message_run(accepted.inputs)
        };
        match run {
            Ok(run) => {
                let agent_state = if run.status == "completed" {
                    project_knowledge::AcceptedImportAgentState::Completed
                } else {
                    project_knowledge::AcceptedImportAgentState::FailedRetryable
                };
                // This write precedes the operation-completion flag. If the
                // process stops in between, recovery can safely finalize a
                // proven completed turn without a duplicate remote call.
                self.update_accepted_import_agent_state(
                    &project_id,
                    &accepted.operation_id,
                    agent_state,
                );
                self.finish_accepted_import_operation(
                    &project_id,
                    &accepted.operation_id,
                    run.status == "completed",
                );
                Ok(run)
            }
            Err(error) => {
                self.update_accepted_import_agent_state(
                    &project_id,
                    &accepted.operation_id,
                    project_knowledge::AcceptedImportAgentState::FailedRetryable,
                );
                self.finish_accepted_import_operation(&project_id, &accepted.operation_id, false);
                Err(error)
            }
        }
    }

    /// Returns the persisted user-turn id that owns a durable accepted-import
    /// operation, or `NotFound` when the operation does not exist. Used by the
    /// reopen recovery trigger to validate an operation identity before emitting
    /// a `working` event. A `None` turn id is a legitimate pre-turn interruption
    /// (the send was killed before the user turn persisted) and is surfaced as a
    /// typed denial by the resume path, never as a bogus "working" turn.
    pub fn accepted_import_operation_turn_id(
        &self,
        project_id: &str,
        operation_id: &str,
    ) -> AppResult<Option<String>> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        let store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::internal("No pudimos recuperar los materiales."))?;
        let operation = store
            .accepted_import_operation(operation_id)
            .map_err(|_| AppError::internal("No pudimos recuperar los materiales."))?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró esa operación."))?;
        Ok(operation.turn_id)
    }

    /// Explicitly resumes only a durably accepted operation whose remote agent
    /// boundary has not started. All identity comes from the ledger and the
    /// existing user message; no composer proposal, source path, Material, or
    /// user turn is recreated.
    ///
    /// The remote boundary is gated on `agent_state`: `NotStarted` is safe to
    /// continue (the provider request can never have already happened), while
    /// any other state (`StartedOutcomeUnknown`, `Completed`, `FailedRetryable`,
    /// `FailedTerminal`) is refused here so a prior outbound request is never
    /// duplicated. A `PendingRetry` operation with `NotStarted` agent state is
    /// a local-only interruption and remains explicitly resumable.
    pub fn resume_accepted_import_operation(
        &self,
        project_id: &str,
        operation_id: &str,
    ) -> AppResult<AgentRunView> {
        let pid = parse_project_id(project_id)?;
        crate::session_log::record(
            "INFO",
            format!("[knowledge][recovery] resume_requested operation_id={operation_id}"),
        );
        let root = self.base.join("projects").join(pid.as_str());
        let store = KnowledgeStore::open(&root, &pid).map_err(|_| {
            record_recovery_error(operation_id, "durable_state_read_failed");
            AppError::internal("No pudimos recuperar los materiales.")
        })?;
        let operation = store
            .accepted_import_operation(operation_id)
            .map_err(|_| {
                record_recovery_error(operation_id, "durable_state_read_failed");
                AppError::internal("No pudimos recuperar los materiales.")
            })?
            .ok_or_else(|| {
                record_recovery_error(operation_id, "durable_state_read_failed");
                AppError::new(ErrorCode::NotFound, "No se encontró esa operación.")
            })?;
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][recovery] operation_loaded operation_id={operation_id} state={} prepared={} indexed={} ready={} agent_state={}",
                accepted_import_state_str(operation.state),
                operation.copied,
                operation.lexical_completed,
                operation.embedding_completed,
                accepted_import_agent_state_str(operation.agent_state),
            ),
        );
        // A pre-turn interruption (the send was killed between material copy and
        // user-turn persistence) has no message to reuse and no recoverable
        // prompt. Refuse it as a typed, non-retryable denial rather than a
        // generic internal error, so the frontend can tell the person to re-send.
        let turn_id = match operation.turn_id.as_deref() {
            Some(turn_id) => turn_id,
            None => {
                crate::session_log::record(
                    "WARN",
                    format!(
                        "[knowledge][recovery] resume_decision=denied reason=no_turn operation_id={operation_id}"
                    ),
                );
                record_recovery_error(operation_id, "no_turn");
                return Err(AppError::new(
                    ErrorCode::RecoveryNoTurn,
                    "El envío se interrumpió antes de confirmarse; volvé a enviarlo.",
                ));
            }
        };
        if operation.state == project_knowledge::AcceptedImportState::Completed {
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge][recovery] resume_decision=denied reason=already_completed operation_id={operation_id}"
                ),
            );
            return Ok(AgentRunView {
                status: "completed".to_owned(),
                turn_id: Some(turn_id.to_owned()),
                registered_creation_ids: Vec::new(),
                message: None,
            });
        }
        if operation.agent_state == project_knowledge::AcceptedImportAgentState::Completed {
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge][recovery] resume_decision=denied reason=already_completed operation_id={operation_id}"
                ),
            );
            self.finish_accepted_import_operation(&pid, operation_id, true);
            return Ok(AgentRunView {
                status: "completed".to_owned(),
                turn_id: Some(turn_id.to_owned()),
                registered_creation_ids: Vec::new(),
                message: None,
            });
        }
        // The remote boundary must be pristine. Any other agent state means a
        // prior provider request may already have happened; never auto-resend.
        if operation.agent_state != project_knowledge::AcceptedImportAgentState::NotStarted {
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge][recovery] resume_decision=denied reason=remote_outcome_unknown operation_id={operation_id}"
                ),
            );
            record_recovery_error(operation_id, "remote_outcome_unknown");
            return Err(AppError::invalid(
                "El resultado anterior quedó pendiente; no se puede reenviar automáticamente.",
            ));
        }
        let material_ids = store
            .accepted_import_material_ids(operation_id)
            .map_err(|_| {
                record_recovery_error(operation_id, "durable_state_read_failed");
                AppError::internal("No pudimos recuperar los materiales.")
            })?
            .into_iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let message = project
            .messages
            .iter()
            .find(|message| message.id.as_str() == turn_id && message.role == MessageRole::User)
            .ok_or_else(|| {
                record_recovery_error(operation_id, "durable_state_read_failed");
                AppError::internal("No pudimos recuperar el turno aceptado.")
            })?;
        let mut inputs =
            self.resolve_agent_inputs_without_knowledge(project_id, &message.text, &material_ids)?;
        inputs.turn_id = Some(
            MessageId::parse(turn_id)
                .map_err(|_| AppError::internal("No pudimos recuperar el turno aceptado."))?,
        );
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][recovery] resume_decision=allowed reason=safe_local_not_started operation_id={operation_id}"
            ),
        );
        crate::session_log::record(
            "INFO",
            format!("[knowledge][recovery] local_resume_started operation_id={operation_id}"),
        );
        let result = self.run_accepted_staged_turn_inner(
            AcceptedStagedTurn {
                inputs,
                operation_id: operation_id.to_owned(),
                material_ids,
            },
            true,
        );
        match &result {
            Ok(_) => {
                if let Ok(Some(progress)) = store.accepted_import_operation(operation_id) {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[knowledge][recovery] local_resume_completed operation_id={operation_id} ready={} failed={}",
                            progress.embedding_completed, progress.failed,
                        ),
                    );
                }
            }
            Err(error) => {
                crate::session_log::record(
                    "ERROR",
                    format!(
                        "[knowledge][recovery] local_resume_failed operation_id={operation_id} phase=run failure_class={}",
                        recovery_failure_class(error)
                    ),
                );
                record_recovery_error(operation_id, recovery_failure_class(error));
            }
        }
        result
    }

    fn finish_accepted_import_operation(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        completed: bool,
    ) {
        let root = self.base.join("projects").join(project_id.as_str());
        if let Ok(mut store) = KnowledgeStore::open(&root, project_id)
            && let Ok(Some(progress)) = store.accepted_import_operation(operation_id)
        {
            let _ = store.update_accepted_import_operation(
                operation_id,
                progress.turn_id.as_deref(),
                if completed {
                    project_knowledge::AcceptedImportState::Completed
                } else {
                    project_knowledge::AcceptedImportState::PendingRetry
                },
                progress.copied,
                progress.lexical_completed,
                progress.embedding_completed,
                progress.failed + usize::from(!completed),
                progress.embeddings_created,
                progress.embeddings_reused,
            );
        }
    }

    fn update_accepted_import_agent_state(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        agent_state: project_knowledge::AcceptedImportAgentState,
    ) {
        let root = self.base.join("projects").join(project_id.as_str());
        if let Ok(mut store) = KnowledgeStore::open(&root, project_id) {
            let _ = store.update_accepted_import_agent_state(operation_id, agent_state);
        }
    }

    /// Runs the agent using the prepared inputs and appends an assistant
    /// message reflecting the outcome (`ok`, `failed`, or `cancelled`).
    pub fn send_message_run(&self, inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        if let Some(local) = inputs
            .knowledge
            .as_ref()
            .and_then(|context| context.local_answer.clone())
        {
            return self.complete_local_knowledge_turn(inputs, local);
        }
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .provider_calls += 1;
        }
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let durable_turn_id = inputs.turn_id.clone();
        let model_ref = inputs.model.clone();
        let knowledge_metrics = inputs.knowledge_metrics.clone();
        let knowledge_for_citations = inputs.knowledge.clone();
        let model = model_ref
            .as_ref()
            .map(|model| format!("{}/{}", model.provider_id, model.model_id))
            .unwrap_or_else(|| "default".to_owned());
        let started = std::time::Instant::now();
        // Whatever raw attachments survive the Knowledge dedup will be
        // provisioned into `workspace/materials` and reach the remote model
        // through a content route additional to bounded evidence. Mirrors the
        // exact predicate `agent` uses (`provision_attachments`), so the
        // structural warning is truthful, never a false alarm.
        let indexed_names: std::collections::HashSet<String> = inputs
            .knowledge
            .as_ref()
            .map(|ctx| ctx.indexed_source_names.iter().cloned().collect())
            .unwrap_or_default();
        let additional_attachment_route = inputs.attachments.iter().any(|attachment| {
            !indexed_names.contains(&project_core::safe_file_name(&attachment.display_name))
        });
        #[cfg(test)]
        if additional_attachment_route {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .raw_attachment_forwarding += 1;
        }
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
                record_turn_usage(
                    project_id.as_str(),
                    turn_id.as_deref(),
                    model_ref.as_ref(),
                    &result.task.usage,
                    started.elapsed().as_millis(),
                    "normal_chat",
                    additional_attachment_route,
                );
                let creation_ids: Vec<CreationId> = result
                    .registered
                    .iter()
                    .map(|id| parse_creation_id(id))
                    .collect::<AppResult<Vec<_>>>()?;
                let mut text = assistant_reply_text(
                    result.task.message.as_deref(),
                    !result.registered.is_empty(),
                );
                text = append_source_traceability(&text, knowledge_for_citations.as_ref());
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
                if status == MessageStatus::Ok {
                    self.persist_completed_turn_metrics(
                        &project_id,
                        durable_turn_id.as_ref(),
                        completed_normal_turn_metrics(
                            model_ref.as_ref(),
                            &result.task.usage,
                            started.elapsed().as_millis(),
                            knowledge_metrics.as_ref(),
                        ),
                    )?;
                }
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
                let failure_kind = other.task_failure_kind();
                let err = AppError::from_agent(other);
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(&project_id, &err.message, MessageStatus::Failed, &[])
                    .map_err(AppError::from_core)?;
                crate::session_log::record(
                    "ERROR",
                    format!(
                        "turn terminal conversation_id={} turn_id={} status=failed failure_stage=agent failure_class={} duration_ms={}",
                        project_id,
                        turn_id.as_deref().unwrap_or("none"),
                        failure_kind.as_str(),
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

    fn complete_local_knowledge_turn(
        &self,
        inputs: AgentRunInputs,
        local: String,
    ) -> AppResult<AgentRunView> {
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let durable_turn_id = inputs.turn_id.clone();
        let model_ref = inputs.model.clone();
        let knowledge_metrics = inputs.knowledge_metrics.clone();
        let text = append_source_traceability(&local, inputs.knowledge.as_ref());
        let started = std::time::Instant::now();
        let usage = RemoteUsage::default();
        record_turn_usage(
            project_id.as_str(),
            turn_id.as_deref(),
            model_ref.as_ref(),
            &usage,
            started.elapsed().as_millis(),
            "normal_chat",
            false,
        );
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(&project_id, &text, MessageStatus::Ok, &[])
            .map_err(AppError::from_core)?;
        let mut metrics = completed_normal_turn_metrics(
            model_ref.as_ref(),
            &usage,
            started.elapsed().as_millis(),
            knowledge_metrics.as_ref(),
        );
        metrics.remote_calls = Some(0);
        metrics.source = Some("unavailable".to_owned());
        self.persist_completed_turn_metrics(&project_id, durable_turn_id.as_ref(), metrics)?;
        Ok(AgentRunView {
            status: "completed".to_owned(),
            turn_id,
            registered_creation_ids: Vec::new(),
            message: Some(text),
        })
    }

    /// Whole-project hierarchical summary (K6) through the shared OpenCode
    /// backend in a dedicated scratch session. This never creates a chat turn
    /// and never sends the corpus: only bounded per-node labelled evidence
    /// crosses the remote boundary. Returns accounting plus the user-facing
    /// global summary text (`None` when the project has no indexed sources).
    pub fn summarize_project_with_backend(
        &self,
        project_id: &str,
    ) -> AppResult<ProjectSummaryAnswerView> {
        use crate::summarize::OpenCodeRemoteSummarizer;
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let scratch = self.base.join("opencode-scratch");
        let summarizer = OpenCodeRemoteSummarizer::new(Arc::clone(backend), scratch);
        self.summarize_project_with(project_id, &summarizer)
    }

    fn summarize_project_with(
        &self,
        project_id: &str,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<ProjectSummaryAnswerView> {
        let report = self.summarize_project(project_id, summarizer)?;
        let global_summary = self.global_summary_text(project_id)?;
        let naive_corpus_est_tokens = self
            .open_knowledge_store(project_id)?
            .and_then(|store| store.corpus_stats().ok())
            .map(|stats| stats.naive_corpus_est_tokens)
            .unwrap_or(0);
        Ok(ProjectSummaryAnswerView {
            report,
            global_summary,
            naive_corpus_est_tokens,
        })
    }

    /// K6 execution for an explicit composer-selected set. Unlike semantic
    /// retrieval, the planner receives every selected ready source directly;
    /// top-k evidence therefore cannot decide whether a source exists. K6's
    /// document/batch/global nodes and cache semantics are preserved, while the
    /// user-facing answer deliberately surfaces the document nodes one-for-one.
    pub fn summarize_selected_sources(
        &self,
        project_id: &str,
        selected_material_ids: &[String],
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<ProjectSummaryAnswerView> {
        let pid = parse_project_id(project_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let root = self.base.join("projects").join(pid.as_str());
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;

        // Composer ids are already authorized at acceptance. Deduplicate here
        // defensively so repeated ids cannot create duplicate source entries.
        let mut seen = HashSet::new();
        let mut sources = Vec::new();
        for (selected_order, material_id) in selected_material_ids.iter().enumerate() {
            if !seen.insert(material_id.clone()) {
                continue;
            }
            let source_label = project
                .materials
                .iter()
                .find(|material| material.id.as_str() == material_id)
                .map(|material| project_core::safe_file_name(&material.original_file_name))
                .unwrap_or_else(|| "Archivo seleccionado".to_owned());
            let document_id = store.document_for_material(material_id).ok().flatten();
            sources.push(SelectedSummarySource {
                source_label,
                selected_order,
                document_id,
            });
        }

        let chunk_counts: std::collections::BTreeMap<String, usize> = store
            .summary_document_levels()
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?
            .into_iter()
            .collect();
        let mut document_ids = HashSet::new();
        let levels: Vec<(String, usize)> = sources
            .iter()
            .filter_map(|source| source.document_id.as_ref())
            .filter(|document_id| document_ids.insert((*document_id).clone()))
            .filter_map(|document_id| {
                chunk_counts
                    .get(document_id)
                    .copied()
                    .filter(|count| *count > 0)
                    .map(|count| (document_id.clone(), count))
            })
            .collect();

        let mut accounting = project_knowledge::SummaryAccounting {
            source_count: sources.len(),
            ..Default::default()
        };
        if !levels.is_empty() {
            let existing = store.summary_existing().map_err(|_| {
                AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen.")
            })?;
            let plan = project_knowledge::plan_project_summaries(
                &levels,
                &existing,
                project_knowledge::BatchOptions::default(),
                None,
            )
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
            accounting.reused = plan.reused.len();
            accounting.cache_hits = plan.reused.len();
            accounting.hierarchy_depth = plan.hierarchy_depth;
            for node in &plan.pending {
                if let Err(failure) =
                    self.synthesize_node(&mut store, node, summarizer, &mut accounting)
                {
                    crate::session_log::record(
                        "WARN",
                        format!(
                            "[knowledge][summary] node_kind={} error_class={} cache=miss",
                            node.level.as_db(),
                            summary_failure_class(failure)
                        ),
                    );
                    store
                        .mark_summary_failed(&node.summary_id, node.level, failure)
                        .map_err(|_| {
                            AppError::new(ErrorCode::Internal, "No pudimos guardar el resumen.")
                        })?;
                }
            }
        }

        let mut contents = std::collections::BTreeMap::new();
        for summary_id in store.summary_ids().map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos leer el estado del resumen.",
            )
        })? {
            if let Some(node) = store.get_summary(&summary_id).map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos leer el estado del resumen.",
                )
            })? && node.level == project_knowledge::SummaryLevel::Document
                && node.source_ids.len() == 1
            {
                contents.insert(node.source_ids[0].clone(), node.content);
            }
        }
        let surface = render_selected_summary_surface(&sources, &contents);
        let naive_corpus_est_tokens = store
            .corpus_stats()
            .map(|stats| stats.naive_corpus_est_tokens)
            .unwrap_or(0);
        Ok(ProjectSummaryAnswerView {
            report: SummarizationReportView {
                remote_calls: accounting.remote_calls,
                estimated_input_units: accounting.estimated_input_units,
                cache_hits: accounting.cache_hits,
                reused: accounting.reused,
                regenerated: accounting.regenerated,
                source_count: accounting.source_count,
                hierarchy_depth: accounting.hierarchy_depth,
                input_tokens: accounting.input_tokens,
                output_tokens: accounting.output_tokens,
                cache_read_tokens: accounting.cache_read_tokens,
                cache_write_tokens: accounting.cache_write_tokens,
                cost_usd: accounting.cost_usd,
                provider_usage_actual: accounting.provider_usage_actual,
            },
            global_summary: Some(surface),
            naive_corpus_est_tokens,
        })
    }

    /// Reads the `global`-level (or single-document) summary prose, if present.
    fn global_summary_text(&self, project_id: &str) -> AppResult<Option<String>> {
        let nodes = self.summary_status(project_id)?;
        let global = nodes
            .iter()
            .filter(|node| node.level == "global" && node.state == "ready")
            .filter_map(|node| node.content.as_ref().map(|c| c.summary.clone()))
            .next()
            .or_else(|| {
                nodes
                    .iter()
                    .filter(|node| node.level == "document" && node.state == "ready")
                    .filter_map(|node| node.content.as_ref().map(|c| c.summary.clone()))
                    .next()
            });
        Ok(global)
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
        let mut inputs =
            self.resolve_agent_inputs_without_knowledge(project_id, prompt, attachment_ids)?;
        let (knowledge, knowledge_metrics) =
            self.prepare_knowledge_context(&inputs.project_id, prompt)?;
        inputs.knowledge = knowledge;
        inputs.knowledge_metrics = knowledge_metrics;
        Ok(inputs)
    }

    /// Validates a turn and resolves attachments without touching the local
    /// Knowledge store/provider.  Accepted imports use this at the durable
    /// commitment boundary; their indexing/retrieval is performed later by
    /// the explicit accepted-turn pipeline.
    fn resolve_agent_inputs_without_knowledge(
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
            selected_material_ids: attachment_ids.to_vec(),
            knowledge: None,
            knowledge_metrics: None,
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
                knowledge: inputs.knowledge,
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
        let started = std::time::Instant::now();
        crate::session_log::record(
            "DEBUG",
            format!(
                "[share] requested conversation_id={project_id} creation_id={}",
                creation_id.unwrap_or("latest")
            ),
        );
        self.prepare_share_visibility(project_id, creation_id).map_err(|error| {
            crate::session_log::record(
                "ERROR",
                format!(
                    "[share] failed stage=local_publish_prepare conversation_id={project_id} error={error}"
                ),
            );
            error
        })?;
        let pid = parse_project_id(project_id).map_err(|error| {
            crate::session_log::record(
                "ERROR",
                format!(
                    "[share] failed stage=local_publish_prepare conversation_id={project_id} error={error}"
                ),
            );
            error
        })?;
        let publication = self.publication.publish(&pid).map_err(|error| {
            let stage = match &error {
                project_publication::PublicationError::TunnelStart => "tunnel_start",
                project_publication::PublicationError::TunnelStop => "tunnel_stop",
                project_publication::PublicationError::PublisherStart => "publisher_start",
                project_publication::PublicationError::PublisherStop => "publisher_stop",
                _ => "local_publish",
            };
            crate::session_log::record(
                "ERROR",
                format!("[share] failed stage={stage} conversation_id={project_id} error={error}"),
            );
            AppError::from_publication(error)
        })?;
        let origin = self
            .publication
            .endpoint()
            .map(|url| url.as_str().to_owned())
            .unwrap_or_else(|| "none".to_owned());
        crate::session_log::record(
            "DEBUG",
            format!(
                "[publish] prepared conversation_id={project_id} route={} origin={origin}",
                publication.route.as_str()
            ),
        );
        crate::session_log::record(
            "INFO",
            format!(
                "creation shared conversation_id={project_id} creation_id={}",
                creation_id.unwrap_or("latest")
            ),
        );
        crate::session_log::record(
            "DEBUG",
            format!(
                "[share] ready conversation_id={project_id} route={} public_url={} elapsed_ms={}",
                publication.route.as_str(),
                publication.public_url.as_deref().unwrap_or("none"),
                started.elapsed().as_millis()
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
        self.publication.unpublish(&pid).map_err(|error| {
            crate::session_log::record(
                "ERROR",
                format!(
                    "[share] failed stage=unpublish conversation_id={project_id} error={error}"
                ),
            );
            AppError::from_publication(error)
        })?;
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

    // -- Summarization (K6) ------------------------------------------------

    /// Produces (or reuses cached) a hierarchical summary for every indexed
    /// source in the project. Remote calls happen only for nodes whose inputs
    /// changed. Returns accounting only; summary content is read via
    /// [`Self::get_summary`]. This never creates a chat turn.
    pub fn summarize_project(
        &self,
        project_id: &str,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<SummarizationReportView> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return Ok(SummarizationReportView {
                remote_calls: 0,
                estimated_input_units: 0,
                cache_hits: 0,
                reused: 0,
                regenerated: 0,
                source_count: 0,
                hierarchy_depth: 0,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cost_usd: None,
                provider_usage_actual: false,
            });
        }
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        let levels = store
            .summary_document_levels()
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        if levels.is_empty() {
            return Ok(SummarizationReportView {
                remote_calls: 0,
                estimated_input_units: 0,
                cache_hits: 0,
                reused: 0,
                regenerated: 0,
                source_count: 0,
                hierarchy_depth: 0,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cost_usd: None,
                provider_usage_actual: false,
            });
        }
        let existing = store
            .summary_existing()
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        let model_id = None;
        let plan = project_knowledge::plan_project_summaries(
            &levels,
            &existing,
            project_knowledge::BatchOptions::default(),
            model_id,
        )
        .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;

        let mut accounting = project_knowledge::SummaryAccounting {
            reused: plan.reused.len(),
            cache_hits: plan.reused.len(),
            source_count: plan.source_count,
            hierarchy_depth: plan.hierarchy_depth,
            ..Default::default()
        };

        for node in &plan.pending {
            match self.synthesize_node(&mut store, node, summarizer, &mut accounting) {
                Ok(()) => {}
                Err(failure) => {
                    store
                        .mark_summary_failed(&node.summary_id, node.level, failure)
                        .map_err(|_| {
                            AppError::new(ErrorCode::Internal, "No pudimos guardar el resumen.")
                        })?;
                }
            }
        }

        Ok(SummarizationReportView {
            remote_calls: accounting.remote_calls,
            estimated_input_units: accounting.estimated_input_units,
            cache_hits: accounting.cache_hits,
            reused: accounting.reused,
            regenerated: accounting.regenerated,
            source_count: accounting.source_count,
            hierarchy_depth: accounting.hierarchy_depth,
            input_tokens: accounting.input_tokens,
            output_tokens: accounting.output_tokens,
            cache_read_tokens: accounting.cache_read_tokens,
            cache_write_tokens: accounting.cache_write_tokens,
            cost_usd: accounting.cost_usd,
            provider_usage_actual: accounting.provider_usage_actual,
        })
    }

    /// Produces (or reuses) a single document summary. Remote calls happen only
    /// when the document summary is missing or stale.
    pub fn summarize_document(
        &self,
        project_id: &str,
        material_id: &str,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<SummarizationReportView> {
        let pid = parse_project_id(project_id)?;
        let mid = parse_material_id(material_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        if !project.materials.iter().any(|m| m.id == mid) {
            return Err(AppError::new(
                ErrorCode::NotFound,
                "No se encontró ese material.",
            ));
        }
        let root = self.base.join("projects").join(pid.as_str());
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        // Resolve the document id owned by this material's source.
        let Some(document_id) = store.document_for_material(mid.as_str()).ok().flatten() else {
            return Ok(SummarizationReportView {
                remote_calls: 0,
                estimated_input_units: 0,
                cache_hits: 0,
                reused: 0,
                regenerated: 0,
                source_count: 0,
                hierarchy_depth: 0,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cost_usd: None,
                provider_usage_actual: false,
            });
        };
        let existing = store
            .summary_existing()
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        let plan = project_knowledge::plan_project_summaries(
            &[(document_id.clone(), 1)],
            &existing,
            project_knowledge::BatchOptions::default(),
            None,
        )
        .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos preparar el resumen."))?;
        let mut accounting = project_knowledge::SummaryAccounting {
            reused: plan.reused.len(),
            cache_hits: plan.reused.len(),
            source_count: plan.source_count,
            hierarchy_depth: plan.hierarchy_depth,
            ..Default::default()
        };
        for node in &plan.pending {
            if let Err(failure) =
                self.synthesize_node(&mut store, node, summarizer, &mut accounting)
            {
                store
                    .mark_summary_failed(&node.summary_id, node.level, failure)
                    .map_err(|_| {
                        AppError::new(ErrorCode::Internal, "No pudimos guardar el resumen.")
                    })?;
            }
        }
        Ok(SummarizationReportView {
            remote_calls: accounting.remote_calls,
            estimated_input_units: accounting.estimated_input_units,
            cache_hits: accounting.cache_hits,
            reused: accounting.reused,
            regenerated: accounting.regenerated,
            source_count: accounting.source_count,
            hierarchy_depth: accounting.hierarchy_depth,
            input_tokens: accounting.input_tokens,
            output_tokens: accounting.output_tokens,
            cache_read_tokens: accounting.cache_read_tokens,
            cache_write_tokens: accounting.cache_write_tokens,
            cost_usd: accounting.cost_usd,
            provider_usage_actual: accounting.provider_usage_actual,
        })
    }

    /// Synthesizes one planned node: assembles bounded evidence, calls the
    /// summarizer, validates the structured output, and persists `Ready`.
    fn synthesize_node(
        &self,
        store: &mut KnowledgeStore,
        node: &project_knowledge::SummaryNode,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
        accounting: &mut project_knowledge::SummaryAccounting,
    ) -> std::result::Result<(), project_knowledge::SummaryFailure> {
        use project_knowledge::SummaryLevel;

        match node.level {
            SummaryLevel::Document => {
                let document_id = &node.source_ids[0];
                let source_name = store
                    .document_source_name(document_id)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| document_id.clone());
                let chunks = store
                    .document_chunks(document_id, 32)
                    .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
                let labels: Vec<String> = (1..=chunks.len()).map(|i| format!("E{i}")).collect();
                let evidence: Vec<(String, String, String)> =
                    project_knowledge::select_document_evidence(&chunks, &labels);
                let request = project_knowledge::build_document_summary_request(
                    &source_name,
                    &evidence,
                    SummaryLevel::Document,
                );
                accounting.estimated_input_units += request.estimated_input_units;
                let (output, content) =
                    self.summarize_and_validate(summarizer, &request, &labels, accounting)?;
                let mut ready = node.clone();
                ready.content = Some(content.clone());
                ready.source_chunk_ids = evidence
                    .iter()
                    .map(|(_, chunk_id, _)| chunk_id.clone())
                    .collect();
                ready.output_fingerprint = project_knowledge::fingerprint_output(&content);
                ready.model_id = output.model_id;
                ready.provider_id = output.provider_id;
                ready.generation_id = "generation-1".to_owned();
                ready.state = project_knowledge::SummaryState::Ready;
                store
                    .store_summary(&ready)
                    .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
                accounting.regenerated += 1;
                Ok(())
            }
            SummaryLevel::Batch | SummaryLevel::Global => {
                let mut children = Vec::new();
                for child_id in &node.source_ids {
                    match store.get_summary(child_id) {
                        Ok(Some(child))
                            if child.state == project_knowledge::SummaryState::Ready =>
                        {
                            let content = child
                                .content
                                .ok_or(project_knowledge::SummaryFailure::EmptyCorpus)?;
                            children.push((child_id.clone(), content));
                        }
                        _ => {
                            return Err(project_knowledge::SummaryFailure::EmptyCorpus);
                        }
                    }
                }
                if children.is_empty() {
                    return Err(project_knowledge::SummaryFailure::EmptyCorpus);
                }
                let labels: Vec<String> = (1..=children.len()).map(|i| format!("P{i}")).collect();
                let request = project_knowledge::build_synthesis_request(&children, node.level);
                accounting.estimated_input_units += request.estimated_input_units;
                let (output, content) =
                    self.summarize_and_validate(summarizer, &request, &labels, accounting)?;
                let mut ready = node.clone();
                ready.content = Some(content.clone());
                ready.output_fingerprint = project_knowledge::fingerprint_output(&content);
                ready.model_id = output.model_id;
                ready.provider_id = output.provider_id;
                ready.generation_id = "generation-1".to_owned();
                ready.state = project_knowledge::SummaryState::Ready;
                store
                    .store_summary(&ready)
                    .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
                accounting.regenerated += 1;
                Ok(())
            }
        }
    }

    fn summarize_and_validate(
        &self,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
        request: &project_knowledge::SummaryRequest,
        labels: &[String],
        accounting: &mut project_knowledge::SummaryAccounting,
    ) -> std::result::Result<
        (
            project_knowledge::SummaryOutput,
            project_knowledge::SummaryContent,
        ),
        project_knowledge::SummaryFailure,
    > {
        const INVALID_OUTPUT_RETRIES: usize = 1;
        let mut attempt = 0usize;
        loop {
            let output = summarizer.summarize(request)?;
            accounting.add_provider_usage(&output.usage);
            accounting.remote_calls += 1;
            match project_knowledge::validate_summary_output(&output.text, labels) {
                Ok(content) => return Ok((output, content)),
                Err(project_knowledge::SummaryFailure::InvalidOutput)
                    if attempt < INVALID_OUTPUT_RETRIES =>
                {
                    crate::session_log::record(
                        "WARN",
                        format!(
                            "[knowledge][summary] node_kind={} error_class=invalid_output retry=1",
                            request.level.as_db()
                        ),
                    );
                    attempt += 1;
                }
                Err(failure) => return Err(failure),
            }
        }
    }

    /// Durable summary status for a project: every node's id, level, and state.
    pub fn summary_status(&self, project_id: &str) -> AppResult<Vec<SummaryNodeView>> {
        let store = self.open_knowledge_store(project_id)?;
        let Some(store) = store else {
            return Ok(vec![]);
        };
        let ids = store.summary_ids().map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos leer el estado del resumen.",
            )
        })?;
        let mut views = Vec::new();
        for id in ids {
            if let Some(node) = store.get_summary(&id).map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos leer el estado del resumen.",
                )
            })? {
                views.push(summary_node_view(&node));
            }
        }
        Ok(views)
    }

    /// Reads one durable summary node.
    pub fn get_summary(&self, project_id: &str, summary_id: &str) -> AppResult<SummaryNodeView> {
        let store = self.open_knowledge_store(project_id)?;
        let store = store
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese resumen."))?;
        let node = store
            .get_summary(summary_id)
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos leer el resumen."))?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese resumen."))?;
        Ok(summary_node_view(&node))
    }

    /// Invalidates a summary node and everything transitively built from it.
    pub fn invalidate_summary(&self, project_id: &str, summary_id: &str) -> AppResult<()> {
        let mut store = self
            .open_knowledge_store(project_id)?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese resumen."))?;
        store
            .invalidate_summary(summary_id)
            .map_err(|_| AppError::new(ErrorCode::Internal, "No pudimos invalidar el resumen."))
    }

    fn open_knowledge_store(&self, project_id: &str) -> AppResult<Option<KnowledgeStore>> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return Ok(None);
        }
        Ok(Some(KnowledgeStore::open(&root, &pid).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos abrir el material de apoyo.",
            )
        })?))
    }

    // -- Status ------------------------------------------------------------
    /// Explicit owned-child shutdown for application exit. Idempotent and
    /// bounded: stops the shared `opencode serve` backend (via the agent
    /// engine), the local HTTP publisher, the shared `cloudflared` tunnel, and
    /// any isolated preview servers, so no EducAI-owned runtime process
    /// outlives the app. Called from the Tauri exit path and safe to call more
    /// than once.
    pub fn shutdown(&self) {
        crate::session_log::record("INFO", "app shutdown requested");
        if let Err(error) = self.agent.shutdown() {
            crate::session_log::record("WARN", format!("app shutdown agent error={error}"));
        }
        self.publication.shutdown();
        self.previews
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        crate::session_log::record("INFO", "app shutdown complete");
    }

    pub fn app_status(&self) -> AppStatusView {
        AppStatusView {
            version: APP_VERSION.to_owned(),
            agent: self.agent_status().to_owned(),
        }
    }
}

/// Maps an embedding-index operation error to a closed, sanitized local
/// failure class. The error itself can contain runtime or SQLite detail, so it
/// must never be logged. Provider-load failures are classified separately by
/// `load_local_embedding_provider` before an operation is attempted.
fn embedding_index_failure_state(
    error: &project_knowledge::KnowledgeError,
) -> SemanticProviderState {
    match error {
        project_knowledge::KnowledgeError::Inference(_)
        | project_knowledge::KnowledgeError::InvalidEmbedding(_)
        | project_knowledge::KnowledgeError::InputTooLong => SemanticProviderState::InferenceFailed,
        project_knowledge::KnowledgeError::EmbeddingPersistFailed => {
            SemanticProviderState::EmbeddingPersistFailed
        }
        project_knowledge::KnowledgeError::Sql(_) | project_knowledge::KnowledgeError::Io(_) => {
            SemanticProviderState::EmbeddingPersistFailed
        }
        project_knowledge::KnowledgeError::Tokenizer(_) => {
            SemanticProviderState::TokenizerLoadFailed
        }
        _ => SemanticProviderState::OtherTypedLocalFailure,
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

fn accepted_operation_id(project_id: &ProjectId, prompt: &str) -> String {
    // A local opaque correlation ID. It intentionally contains no path or
    // source content; UUID-grade randomness is unnecessary because the
    // nanosecond clock is paired with a prompt digest and this database is
    // scoped to exactly one project.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let digest = sha256_hex(prompt.as_bytes());
    format!("{}-{nanos}-{}", project_id.as_str(), &digest[..16])
}

/// Structural, sanitized label for the remote agent boundary. Never a path,
/// prompt, or payload.
fn accepted_import_agent_state_str(
    state: project_knowledge::AcceptedImportAgentState,
) -> &'static str {
    match state {
        project_knowledge::AcceptedImportAgentState::NotStarted => "not_started",
        project_knowledge::AcceptedImportAgentState::StartedOutcomeUnknown => {
            "started_outcome_unknown"
        }
        project_knowledge::AcceptedImportAgentState::Completed => "completed",
        project_knowledge::AcceptedImportAgentState::FailedRetryable => "failed_retryable",
        project_knowledge::AcceptedImportAgentState::FailedTerminal => "failed_terminal",
    }
}

/// Structural, sanitized label for the accepted-import operation phase. Never
/// a path, prompt, or payload.
fn accepted_import_state_str(state: project_knowledge::AcceptedImportState) -> &'static str {
    match state {
        project_knowledge::AcceptedImportState::Accepted => "accepted",
        project_knowledge::AcceptedImportState::Copying => "copying",
        project_knowledge::AcceptedImportState::IndexingLexical => "indexing_lexical",
        project_knowledge::AcceptedImportState::IndexingEmbeddings => "indexing_embeddings",
        project_knowledge::AcceptedImportState::PendingRetry => "pending_retry",
        project_knowledge::AcceptedImportState::Completed => "completed",
    }
}

/// Sanitized recovery error log. Only the opaque operation id, the typed
/// failure class, and structural counters are emitted — never document bodies,
/// chunk text, prompts, assistant text, vectors, or absolute paths.
fn record_recovery_error(operation_id: &str, failure_class: &str) {
    crate::session_log::record(
        "ERROR",
        format!(
            "[knowledge][ERROR] operation_id={operation_id} phase=recovery failure_class={failure_class}"
        ),
    );
}

/// Maps a resume failure onto the recovery failure taxonomy without inventing a
/// parallel enum. Agent/inference failures reuse the existing inference class;
/// everything else is a sanitized typed local failure.
fn recovery_failure_class(error: &AppError) -> &'static str {
    match error.code {
        ErrorCode::AiUnavailable | ErrorCode::AiTaskFailed => "inference_failed",
        ErrorCode::StorageUnavailable | ErrorCode::NotFound => "durable_state_read_failed",
        ErrorCode::InvalidInput => "remote_outcome_unknown",
        ErrorCode::RecoveryNoTurn => "no_turn",
        _ => "other_typed_local_failure",
    }
}

fn summary_failure_class(failure: project_knowledge::SummaryFailure) -> &'static str {
    match failure {
        project_knowledge::SummaryFailure::ProviderUnavailable => "provider_unavailable",
        project_knowledge::SummaryFailure::ExecutionFailed => "execution_failed",
        project_knowledge::SummaryFailure::InvalidOutput => "invalid_output",
        project_knowledge::SummaryFailure::EmptyCorpus => "empty_corpus",
    }
}

fn summary_node_view(node: &project_knowledge::SummaryNode) -> SummaryNodeView {
    SummaryNodeView {
        summary_id: node.summary_id.clone(),
        level: match node.level {
            project_knowledge::SummaryLevel::Document => "document",
            project_knowledge::SummaryLevel::Batch => "batch",
            project_knowledge::SummaryLevel::Global => "global",
        }
        .to_owned(),
        state: match node.state {
            project_knowledge::SummaryState::Pending => "pending",
            project_knowledge::SummaryState::Ready => "ready",
            project_knowledge::SummaryState::Failed => "failed",
            project_knowledge::SummaryState::Stale => "stale",
        }
        .to_owned(),
        failure: node.failure.as_ref().map(|failure| {
            match failure {
                project_knowledge::SummaryFailure::ProviderUnavailable => "provider_unavailable",
                project_knowledge::SummaryFailure::ExecutionFailed => "execution_failed",
                project_knowledge::SummaryFailure::InvalidOutput => "invalid_output",
                project_knowledge::SummaryFailure::EmptyCorpus => "empty_corpus",
            }
            .to_owned()
        }),
        content: node.content.as_ref().map(|content| SummaryContentView {
            summary: content.summary.clone(),
            topics: content
                .topics
                .iter()
                .map(|item| SummaryItemView {
                    text: item.text.clone(),
                    evidence: item.evidence.clone(),
                })
                .collect(),
            decisions: content
                .decisions
                .iter()
                .map(|item| SummaryItemView {
                    text: item.text.clone(),
                    evidence: item.evidence.clone(),
                })
                .collect(),
            action_items: content
                .action_items
                .iter()
                .map(|item| SummaryItemView {
                    text: item.text.clone(),
                    evidence: item.evidence.clone(),
                })
                .collect(),
            questions: content
                .questions
                .iter()
                .map(|item| SummaryItemView {
                    text: item.text.clone(),
                    evidence: item.evidence.clone(),
                })
                .collect(),
        }),
        source_count: node.source_ids.len(),
        parent_summary_id: node.parent_summary_id.clone(),
    }
}

fn material_source(material: &Material) -> MaterialSource {
    MaterialSource {
        material_id: material.id.clone(),
        source_name: material.original_file_name.clone(),
        relative_path: material.relative_path.as_str().to_owned(),
        media_type: material
            .content_type
            .as_ref()
            .map(|content_type| content_type.as_str().to_owned()),
    }
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

fn accepted_import_progress_view(
    operation: project_knowledge::AcceptedImportOperation,
) -> AcceptedImportProgressView {
    AcceptedImportProgressView {
        operation_id: operation.operation_id,
        state: accepted_import_state_str(operation.state).to_owned(),
        agent_state: accepted_import_agent_state_str(operation.agent_state).to_owned(),
        total: operation.total,
        copied: operation.copied,
        lexical_completed: operation.lexical_completed,
        embedding_completed: operation.embedding_completed,
        failed: operation.failed,
        embeddings_created: operation.embeddings_created,
        embeddings_reused: operation.embeddings_reused,
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
        turn_metrics: m.turn_metrics.clone().map(TurnMetricsView::from),
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

/// Renders one entry for every source selected by the user. Dated names sort
/// chronologically; ties and entries without a safe filename date retain the
/// original composer order. This avoids inventing chronology from content.
fn render_selected_summary_surface(
    sources: &[SelectedSummarySource],
    contents: &std::collections::BTreeMap<String, Option<project_knowledge::SummaryContent>>,
) -> String {
    let mut ordered: Vec<&SelectedSummarySource> = sources.iter().collect();
    ordered.sort_by_key(|source| {
        let date = safe_filename_date(&source.source_label);
        (date.is_none(), date, source.selected_order)
    });
    ordered
        .into_iter()
        .map(|source| {
            let heading = match safe_filename_date(&source.source_label) {
                Some((year, month, day)) => {
                    format!("{year:04}-{month:02}-{day:02} — {}", source.source_label)
                }
                None => format!("Sin fecha confiable — {}", source.source_label),
            };
            let text = source
                .document_id
                .as_ref()
                .and_then(|document_id| contents.get(document_id))
                .and_then(|content| content.as_ref())
                .map(|content| content.summary.trim())
                .filter(|summary| !summary.is_empty())
                .unwrap_or("No se pudo procesar este archivo.");
            format!("{heading}\n{text}")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Accept only an unambiguous ISO-like filename date (`YYYY-MM-DD` or
/// `YYYY_MM_DD`) bounded by non-alphanumeric characters. Numeric timestamp
/// noise such as `202401011230` is deliberately not a date hint.
fn safe_filename_date(name: &str) -> Option<(i32, u32, u32)> {
    let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(name);
    let bytes = stem.as_bytes();
    for start in 0..bytes.len().saturating_sub(9) {
        let end = start + 10;
        if end > bytes.len()
            || (start > 0 && bytes[start - 1].is_ascii_alphanumeric())
            || (end < bytes.len() && bytes[end].is_ascii_alphanumeric())
            || (bytes[start + 4] != b'-' && bytes[start + 4] != b'_')
            || (bytes[start + 7] != b'-' && bytes[start + 7] != b'_')
            || !bytes[start..start + 4].iter().all(u8::is_ascii_digit)
            || !bytes[start + 5..start + 7].iter().all(u8::is_ascii_digit)
            || !bytes[start + 8..end].iter().all(u8::is_ascii_digit)
        {
            continue;
        }
        let year = std::str::from_utf8(&bytes[start..start + 4])
            .ok()?
            .parse()
            .ok()?;
        let month: u32 = std::str::from_utf8(&bytes[start + 5..start + 7])
            .ok()?
            .parse()
            .ok()?;
        let day: u32 = std::str::from_utf8(&bytes[start + 8..end])
            .ok()?
            .parse()
            .ok()?;
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
            2 => 28,
            _ => continue,
        };
        if day > 0 && day <= days {
            return Some((year, month, day));
        }
    }
    None
}

fn evidence_kind_label(signals: &HybridMatchSignals) -> Option<String> {
    match (signals.lexical_match, signals.semantic_match) {
        (true, true) => Some("both".to_owned()),
        (true, false) => Some("lexical".to_owned()),
        (false, true) => Some("semantic".to_owned()),
        (false, false) => None,
    }
}

fn exhaustive_local_answer(
    report: &project_knowledge::ExhaustiveSearchReport,
    terms: &[String],
) -> Option<String> {
    if terms.is_empty() || report.lexical_hits > 0 {
        return None;
    }
    match report.coverage {
        ExhaustiveCoverage::Incomplete => Some(
            "No encontré evidencia suficiente en la búsqueda realizada, pero no pude verificar exhaustivamente todo el corpus."
                .to_owned(),
        ),
        ExhaustiveCoverage::Complete => {
            let inspected = report.materials_inspected;
            let topic = if terms.is_empty() {
                "ese tema".to_owned()
            } else {
                terms.join(" ni ")
            };
            Some(format!(
                "No encontré menciones de {topic} en los {inspected} materiales inspeccionados.\nInspeccioné los {inspected} materiales disponibles del corpus."
            ))
        }
        ExhaustiveCoverage::NotRequested => None,
    }
}

fn append_source_traceability(text: &str, knowledge: Option<&AgentKnowledgeContext>) -> String {
    let Some(knowledge) = knowledge else {
        return text.to_owned();
    };
    let names: Vec<String> = if !knowledge.citation_source_names.is_empty() {
        knowledge
            .citation_source_names
            .iter()
            .map(|name| project_core::safe_file_name(name))
            .filter(|name| !name.contains('/') && !name.contains('\\'))
            .collect()
    } else {
        let mut seen = HashSet::new();
        knowledge
            .entries
            .iter()
            .filter_map(|entry| {
                let name = project_core::safe_file_name(&entry.source_name);
                if name.contains('/') || name.contains('\\') || !seen.insert(name.clone()) {
                    None
                } else {
                    Some(name)
                }
            })
            .collect()
    };
    if names.is_empty() || text.contains("Fuentes:") {
        return text.to_owned();
    }
    let mut rows: Vec<String> = names
        .into_iter()
        .map(|name| match safe_filename_date(&name) {
            Some((year, month, day)) => format!("{year:04}-{month:02}-{day:02} … {name}"),
            None => name,
        })
        .collect();
    rows.sort();
    let mut out = text.trim_end().to_owned();
    out.push_str("\n\nFuentes:\n");
    for row in rows {
        out.push_str("- ");
        out.push_str(&row);
        out.push('\n');
    }
    out
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

/// Emits the local Knowledge architecture metrics (corpus/index facts) that are
/// known regardless of semantic-provider success. This is deliberately separate
/// from provider telemetry. A K6 summary turn does not query the semantic
/// provider or build a K4 candidate/evidence set, so those per-turn fields are
/// unavailable (rendered "No disponible") rather than invented values.
fn record_summary_knowledge_metrics(
    project_id: &str,
    source_count: usize,
    corpus: &project_knowledge::KnowledgeCorpusStats,
) {
    crate::session_log::record_knowledge(
        crate::session_log::SessionKnowledgeMetrics {
            conversation_id: project_id.to_owned(),
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: corpus.naive_corpus_est_tokens,
            retrieval_candidate_count: None,
            selected_evidence_count: None,
            selected_evidence_bytes: None,
            selected_evidence_utf8_chars: None,
            evidence_est_tokens: None,
            context_reduction_pct: None,
            semantic_provider_state: "unavailable".to_owned(),
            request_preparation_ms: None,
            retrieval_mode: None,
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: None,
            lexical_hits: None,
            semantic_hits: None,
        },
        format!(
            "[knowledge] summary_corpus materials={} corpus_bytes={} corpus_chars={} corpus_est_tokens={} sources={}",
            corpus.material_count,
            corpus.corpus_bytes,
            corpus.corpus_utf8_chars,
            corpus.naive_corpus_est_tokens,
            source_count
        ),
    );
}

fn record_turn_usage(
    conversation_id: &str,
    turn_id: Option<&str>,
    model: Option<&ModelRef>,
    usage: &RemoteUsage,
    turn_duration_ms: u128,
    reason: &str,
    additional_attachment_route: bool,
) {
    let (provider, model) = model
        .map(|model| (model.provider_id.clone(), model.model_id.clone()))
        .unwrap_or_else(|| ("unavailable".to_owned(), "unavailable".to_owned()));
    let source = match usage.source {
        UsageSource::ProviderActual => "provider_actual",
        UsageSource::Estimated => "estimated",
        UsageSource::Unavailable => "unavailable",
    };
    let remote_calls = if usage.available() { Some(1) } else { None };
    crate::session_log::record_usage(crate::session_log::SessionUsage {
        conversation_id: conversation_id.to_owned(),
        turn_id: turn_id.unwrap_or("unavailable").to_owned(),
        provider,
        model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        total_tokens: usage.total_tokens,
        cost_usd: usage.cost_usd,
        turn_duration_ms: Some(turn_duration_ms),
        source: source.to_owned(),
        remote_calls,
        reason: reason.to_owned(),
        additional_attachment_route,
    });
}

fn provider_identity(model: Option<&ModelRef>) -> (Option<String>, Option<String>) {
    model
        .map(|model| {
            (
                Some(model.provider_id.clone()),
                Some(model.model_id.clone()),
            )
        })
        .unwrap_or((None, None))
}

fn completed_normal_turn_metrics(
    model: Option<&ModelRef>,
    usage: &RemoteUsage,
    duration_ms: u128,
    knowledge: Option<&TurnKnowledgeMetrics>,
) -> TurnMetrics {
    let (provider, model) = provider_identity(model);
    TurnMetrics {
        provider,
        model,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        total_tokens: usage.total_tokens,
        cost_usd: usage.cost_usd,
        turn_duration_ms: u64::try_from(duration_ms).ok(),
        source: Some(
            match usage.source {
                UsageSource::ProviderActual => "provider_actual",
                UsageSource::Estimated => "estimated",
                UsageSource::Unavailable => "unavailable",
            }
            .to_owned(),
        ),
        remote_calls: usage.available().then_some(1),
        material_count: knowledge.map(|metrics| metrics.material_count),
        corpus_bytes: knowledge.map(|metrics| metrics.corpus_bytes),
        corpus_utf8_chars: knowledge.map(|metrics| metrics.corpus_utf8_chars),
        corpus_est_tokens: knowledge.map(|metrics| metrics.corpus_est_tokens),
        retrieval_candidate_count: knowledge.and_then(|metrics| metrics.retrieval_candidate_count),
        selected_evidence_count: knowledge.and_then(|metrics| metrics.selected_evidence_count),
        selected_evidence_bytes: knowledge.and_then(|metrics| metrics.selected_evidence_bytes),
        selected_evidence_utf8_chars: knowledge
            .and_then(|metrics| metrics.selected_evidence_utf8_chars),
        evidence_est_tokens: knowledge.and_then(|metrics| metrics.evidence_est_tokens),
        context_reduction_pct: knowledge.and_then(|metrics| metrics.context_reduction_pct),
        semantic_provider_state: knowledge.map(|metrics| metrics.semantic_provider_state.clone()),
        request_preparation_ms: knowledge.and_then(|metrics| metrics.request_preparation_ms),
        retrieval_mode: knowledge.and_then(|metrics| metrics.retrieval_mode.clone()),
        eligible_materials: knowledge.and_then(|metrics| metrics.eligible_materials),
        materials_inspected: knowledge.and_then(|metrics| metrics.materials_inspected),
        chunks_inspected: knowledge.and_then(|metrics| metrics.chunks_inspected),
        exhaustive_coverage: knowledge.and_then(|metrics| metrics.exhaustive_coverage.clone()),
        lexical_hits: knowledge.and_then(|metrics| metrics.lexical_hits),
        semantic_hits: knowledge.and_then(|metrics| metrics.semantic_hits),
    }
}

fn completed_summary_turn_metrics(
    model: Option<&ModelRef>,
    report: &SummarizationReportView,
    duration_ms: u128,
    corpus: &project_knowledge::KnowledgeCorpusStats,
) -> TurnMetrics {
    let mut metrics = completed_summary_turn_metrics_without_corpus(model, report, duration_ms);
    metrics.material_count = Some(corpus.material_count);
    metrics.corpus_bytes = Some(corpus.corpus_bytes);
    metrics.corpus_utf8_chars = Some(corpus.corpus_utf8_chars);
    metrics.corpus_est_tokens = Some(corpus.naive_corpus_est_tokens);
    // K6 does not query the semantic provider, so this intentionally remains
    // None rather than inferring availability from corpus/index existence.
    metrics
}

fn completed_summary_turn_metrics_without_corpus(
    model: Option<&ModelRef>,
    report: &SummarizationReportView,
    duration_ms: u128,
) -> TurnMetrics {
    let (provider, model) = provider_identity(model);
    TurnMetrics {
        provider,
        model,
        input_tokens: report.input_tokens,
        output_tokens: report.output_tokens,
        cache_read_tokens: report.cache_read_tokens,
        cache_write_tokens: report.cache_write_tokens,
        total_tokens: None,
        cost_usd: report.cost_usd,
        turn_duration_ms: u64::try_from(duration_ms).ok(),
        source: Some(
            if report.provider_usage_actual {
                "provider_actual"
            } else {
                "unavailable"
            }
            .to_owned(),
        ),
        remote_calls: Some(report.remote_calls),
        material_count: None,
        corpus_bytes: None,
        corpus_utf8_chars: None,
        corpus_est_tokens: None,
        retrieval_candidate_count: None,
        selected_evidence_count: None,
        selected_evidence_bytes: None,
        selected_evidence_utf8_chars: None,
        evidence_est_tokens: None,
        context_reduction_pct: None,
        semantic_provider_state: None,
        request_preparation_ms: None,
        retrieval_mode: None,
        eligible_materials: None,
        materials_inspected: None,
        chunks_inspected: None,
        exhaustive_coverage: None,
        lexical_hits: None,
        semantic_hits: None,
    }
}

/// Records the AGGREGATE provider usage for a K6 summarization turn. The K6
/// per-node `SummaryAccounting` already sums every document/batch/global call,
/// so one record represents the whole logical turn. `reason` is
/// `summary_global` or `summary_selected_sources`; token fields stay `None`
/// (and render "No disponible") when the summarizer reported no actual
/// provider telemetry. Additional raw attachment forwarding is always false
/// on this path: supported indexed text stays on K6 evidence.
fn record_summary_turn_usage(
    conversation_id: &str,
    turn_id: Option<&str>,
    model: Option<&ModelRef>,
    report: &SummarizationReportView,
    turn_duration_ms: u128,
    reason: &str,
) {
    let (provider, model) = model
        .map(|model| (model.provider_id.clone(), model.model_id.clone()))
        .unwrap_or_else(|| ("unavailable".to_owned(), "unavailable".to_owned()));
    let source = if report.provider_usage_actual {
        "provider_actual"
    } else {
        "unavailable"
    };
    crate::session_log::record_usage(crate::session_log::SessionUsage {
        conversation_id: conversation_id.to_owned(),
        turn_id: turn_id.unwrap_or("unavailable").to_owned(),
        provider,
        model,
        input_tokens: report.input_tokens,
        output_tokens: report.output_tokens,
        cache_read_tokens: report.cache_read_tokens,
        cache_write_tokens: report.cache_write_tokens,
        total_tokens: None,
        cost_usd: report.cost_usd,
        turn_duration_ms: Some(turn_duration_ms),
        source: source.to_owned(),
        remote_calls: Some(report.remote_calls),
        reason: reason.to_owned(),
        additional_attachment_route: false,
    });
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)] // Production helpers continue below.
mod durable_k6_tests {
    use super::*;
    use project_agent::FakeAgentEngine;
    use project_core::{MaterialId, ProjectId};
    use project_knowledge::{
        EmbeddingGeneration, EmbeddingProvider, KnowledgeStore, MaterialIndexState, ModelManifest,
        RemoteSummarizer, SummaryContent, SummaryFailure, SummaryOutput, SummaryRequest,
        SummaryUsage,
    };
    use project_opencode::OpenCodeBackend;
    use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
    use project_tunnel::FakeTunnel;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn connector() -> FakeProviderConnector {
        FakeProviderConnector::new()
            .with_provider(ProviderDetail {
                id: "opencode".into(),
                name: "Gratis".into(),
                auth_methods: vec![],
                connections: vec![],
            })
            .with_model(ModelSummary {
                provider_id: "opencode".into(),
                model_id: "big-pickle".into(),
                name: "big-pickle".into(),
                free: true,
                recommended: true,
                deprecated: false,
            })
    }
    fn app(
        base: &std::path::Path,
    ) -> AppState<FakeAgentEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
        AppState::with_components(
            base.to_path_buf(),
            FakeAgentEngine::new(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        )
    }
    #[derive(Clone)]
    struct Multi {
        calls: Arc<Mutex<usize>>,
    }
    impl RemoteSummarizer for Multi {
        fn summarize(&self, _request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            let mut n = self.calls.lock().unwrap();
            *n += 1;
            let (input, output, cache, cost) = match *n {
                1 => (101, 11, 1001, 0.01),
                2 => (202, 22, 2002, 0.02),
                3 => (303, 33, 3003, 0.03),
                _ => (404, 44, 4004, 0.04),
            };
            let content = SummaryContent {
                summary: "respuesta K6 durable sk-project-json-sentinel VECTOR_BLOB_SENTINEL /absolute/project-json-path-sentinel".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            Ok(SummaryOutput {
                text: serde_json::to_string(&content).unwrap(),
                model_id: Some("m".into()),
                provider_id: Some("p".into()),
                usage: SummaryUsage {
                    input_tokens: Some(input),
                    output_tokens: Some(output),
                    cache_read_tokens: Some(cache),
                    cache_write_tokens: None,
                    cost_usd: Some(cost),
                    provider_actual: true,
                },
            })
        }
    }
    #[test]
    fn k6_terminal_path_persists_aggregate_across_disk_restart() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("K6").unwrap();
        for (name, body) in [
            ("a.txt", "contenido uno sk-project-json-sentinel"),
            (
                "b.txt",
                "contenido dos VECTOR_BLOB_SENTINEL /absolute/project-json-path-sentinel",
            ),
        ] {
            let source = tmp.path().join(name);
            std::fs::write(&source, body).unwrap();
            state
                .add_material_from_path(&project.id, source.to_str().unwrap())
                .unwrap();
        }
        let inputs = state
            .send_message_persist(
                &project.id,
                "resumime los archivos sk-project-json-sentinel",
                &[],
            )
            .unwrap();
        let calls = Arc::new(Mutex::new(0));
        let summarizer = Multi {
            calls: calls.clone(),
        };
        assert_eq!(
            state
                .send_summary_run_with(inputs, &summarizer)
                .unwrap()
                .status,
            "completed"
        );
        assert_eq!(*calls.lock().unwrap(), 4);
        let before = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(before.input_tokens, Some(1010));
        assert_eq!(before.output_tokens, Some(110));
        assert_eq!(before.cache_read_tokens, Some(10010));
        assert_eq!(before.remote_calls, Some(4));
        assert_eq!(before.cost_usd, Some(0.10));
        assert_eq!(before.material_count, Some(2));
        assert!(before.corpus_bytes.unwrap() > 0);
        assert!(before.corpus_utf8_chars.unwrap() > 0);
        assert!(before.corpus_est_tokens.unwrap() > 0);
        assert_eq!(before.selected_evidence_count, None);
        assert_eq!(before.retrieval_candidate_count, None);
        assert_eq!(before.selected_evidence_bytes, None);
        assert_eq!(before.selected_evidence_utf8_chars, None);
        assert_eq!(before.evidence_est_tokens, None);
        assert_eq!(before.context_reduction_pct, None);
        assert_eq!(before.semantic_provider_state, None);
        assert_eq!(before.request_preparation_ms, None);
        let view = state.open_project(&project.id).unwrap();
        assert_eq!(view.messages.iter().filter(|m| m.role == "user").count(), 1);
        assert_eq!(
            view.messages
                .iter()
                .filter(|m| m.role == "assistant")
                .count(),
            1
        );
        assert!(
            view.messages
                .iter()
                .find(|m| m.role == "user")
                .unwrap()
                .turn_metrics
                .is_some()
        );
        let disk = std::fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&disk).unwrap();
        let metrics = &json["messages"][0]["turnMetrics"];
        assert!(metrics.is_object());
        let serialized = metrics.to_string();
        for forbidden in [
            "resumime los archivos sk-project-json-sentinel",
            "respuesta K6 durable",
            "contenido uno",
            "contenido dos",
            "sk-project-json-sentinel",
            "VECTOR_BLOB_SENTINEL",
            "/absolute/project-json-path-sentinel",
        ] {
            assert!(!serialized.contains(forbidden));
        }
        let allowed = [
            "provider",
            "model",
            "inputTokens",
            "outputTokens",
            "cacheReadTokens",
            "cacheWriteTokens",
            "totalTokens",
            "costUsd",
            "turnDurationMs",
            "source",
            "remoteCalls",
            "materialCount",
            "corpusBytes",
            "corpusUtf8Chars",
            "corpusEstTokens",
            "retrievalCandidateCount",
            "selectedEvidenceCount",
            "selectedEvidenceBytes",
            "selectedEvidenceUtf8Chars",
            "evidenceEstTokens",
            "contextReductionPct",
            "semanticProviderState",
            "requestPreparationMs",
            "retrievalMode",
            "eligibleMaterials",
            "materialsInspected",
            "chunksInspected",
            "exhaustiveCoverage",
            "lexicalHits",
            "semanticHits",
        ];
        for key in metrics.as_object().unwrap().keys() {
            assert!(
                allowed.contains(&key.as_str()),
                "unexpected metrics key: {key}"
            );
        }
        state.clear_session_logs();
        drop(state);
        let fresh = app(tmp.path());
        let after = fresh.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(after.input_tokens, Some(1010));
        assert_eq!(after.output_tokens, Some(110));
        assert_eq!(after.cache_read_tokens, Some(10010));
        assert_eq!(after.remote_calls, Some(4));
        assert_eq!(after.cost_usd, Some(0.10));
        assert_eq!(after.material_count, Some(2));
        assert!(after.corpus_bytes.unwrap() > 0);
        assert!(after.corpus_utf8_chars.unwrap() > 0);
        assert!(after.corpus_est_tokens.unwrap() > 0);
        assert_eq!(after.selected_evidence_count, None);
        assert_eq!(after.retrieval_candidate_count, None);
        assert_eq!(after.selected_evidence_bytes, None);
        assert_eq!(after.selected_evidence_utf8_chars, None);
        assert_eq!(after.evidence_est_tokens, None);
        assert_eq!(after.context_reduction_pct, None);
        assert_eq!(after.semantic_provider_state, None);
        assert_eq!(after.request_preparation_ms, None);
        assert_eq!(*calls.lock().unwrap(), 4, "reopen/read performs no K6 work");
        assert_eq!(
            fresh.test_activity(),
            TestActivity::default(),
            "reopening durable K6 metrics invokes no provider/OpenCode, K6, indexing, embedding inference/persistence, retrieval, or raw attachment forwarding"
        );
    }

    #[test]
    fn selected_per_source_send_path_keeps_exact_identities_and_truthful_metrics() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let historical = tmp.path().join("1999-01-01-historical.md");
        std::fs::write(&historical, "HISTORICAL_ONLY_SENTINEL").unwrap();
        state
            .add_material_from_path(&project.id, historical.to_str().unwrap())
            .unwrap();
        let names = [
            "2024-03-02-c.md",
            "2024-01-03-a.md",
            "2024-01-03-b.md",
            "2024-02-01-d.md",
            "2024-04-05-e.md",
        ];
        let selected: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(i, name)| {
                let path = tmp.path().join(name);
                std::fs::write(&path, format!("SELECTED_{i}_SENTINEL")).unwrap();
                state
                    .add_material_from_path(&project.id, path.to_str().unwrap())
                    .unwrap()
                    .id
            })
            .collect();
        let prompt = "haceme un resumen de cada archivo ordenado por fecha";
        assert_eq!(
            crate::summarize::detect_summary_intent(prompt, selected.len()),
            crate::summarize::SummaryIntent::SelectedPerSource
        );
        let inputs = state
            .send_message_persist(&project.id, prompt, &selected)
            .unwrap();
        assert_eq!(inputs.selected_material_ids, selected);
        let calls = Arc::new(Mutex::new(0));
        let summarizer = Multi {
            calls: calls.clone(),
        };
        let run = state.send_summary_run_with(inputs, &summarizer).unwrap();
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        for name in names {
            assert_eq!(
                surface.matches(name).count(),
                1,
                "missing or duplicate {name}"
            );
        }
        assert!(!surface.contains("historical"));
        assert!(!surface.contains("HISTORICAL_ONLY_SENTINEL"));
        let positions: Vec<usize> = [
            "2024-01-03-a.md",
            "2024-01-03-b.md",
            "2024-02-01-d.md",
            "2024-03-02-c.md",
            "2024-04-05-e.md",
        ]
        .iter()
        .map(|name| surface.find(name).unwrap())
        .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(state.test_activity().k6_calls, 1);
        assert_eq!(state.test_activity().provider_calls, 0);
        assert_eq!(state.test_activity().raw_attachment_forwarding, 0);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(*calls.lock().unwrap()));
        assert_eq!(metrics.remote_calls, Some(7));
        assert_eq!(metrics.selected_evidence_count, None);
        assert_eq!(metrics.retrieval_candidate_count, None);
        assert!(metrics.material_count.unwrap() >= 6);
    }

    struct DeterministicEmbeddings {
        generation: EmbeddingGeneration,
    }
    impl EmbeddingProvider for DeterministicEmbeddings {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, _query: &str) -> project_knowledge::Result<Vec<f32>> {
            Ok(vec![0.0; 384])
        }
        fn embed_passages(
            &mut self,
            passages: &[String],
        ) -> project_knowledge::Result<Vec<Vec<f32>>> {
            Ok(passages
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    let mut vector = vec![0.0; 384];
                    vector[index % 384] = 1.0;
                    vector
                })
                .collect())
        }
    }

    fn valid_k6_json() -> String {
        serde_json::to_string(&SummaryContent {
            summary: "Resumen usable del archivo seleccionado.".to_owned(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        })
        .unwrap()
    }

    /// Production-faithful Fedora 5-file exhaustive summary: composer-staged
    /// acceptance, lexical+deterministic embedding index, then the same K6
    /// terminal (`send_summary_run_with`) against the real OpenCode 1.18.25
    /// `GET /session/{id}/message` contract (bare `{info, parts}` array,
    /// `info.finish` omitted → `tool-calls` → `stop`). Before completion
    /// detection matched that lifecycle, this path timed out 5×120s with
    /// `remote_calls=0` and "No se pudo procesar este archivo."
    #[test]
    fn five_staged_ready_markdown_files_generate_k6_nodes_through_opencode_envelope() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let historical = tmp.path().join("1999-01-01-historical.md");
        std::fs::write(&historical, "HISTORICAL_ONLY_BODY_SENTINEL").unwrap();
        state
            .add_material_from_path(&project.id, historical.to_str().unwrap())
            .unwrap();

        let names = [
            "2026-07-28-c.md",
            "2026-07-17-a.md",
            "2026-07-27-b.md",
            "2026-07-30-d.md",
            "2026-07-31-e.md",
        ];
        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let path = corpus.join(name);
                std::fs::write(
                    &path,
                    format!("SELECTED_{index}_BODY_SENTINEL decision unique to {name}\n"),
                )
                .unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let before = state.open_project(&project.id).unwrap();
        assert_eq!(
            before.materials.len(),
            1,
            "only historical material pre-send"
        );
        assert!(before.messages.is_empty());

        let prompt = "haceme un resumen de cada archivo ordenado por fecha";
        let accepted = state
            .send_staged_message_persist(&project.id, prompt, &paths, &[])
            .unwrap();
        assert_eq!(accepted.inputs.selected_material_ids.len(), 5);
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );

        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let material_ids: Vec<MaterialId> = accepted
            .material_ids
            .iter()
            .map(|id| MaterialId::parse(id).unwrap())
            .collect();
        for material_id in &material_ids {
            let status = store.material_index_status(material_id).unwrap().unwrap();
            assert_eq!(status.state, MaterialIndexState::Ready);
            assert!(
                store
                    .document_for_material(material_id.as_str())
                    .unwrap()
                    .is_some()
            );
        }
        let mut embeddings = DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        };
        let outcome = store
            .index_embeddings_for_materials(&mut embeddings, 8, &material_ids)
            .unwrap();
        assert!(
            outcome.embedded > 0,
            "deterministic embeddings must persist for the accepted batch"
        );

        let server = fake_opencode_server::FakeServer::start();
        server.set_k6_echo_from_prompt(true);
        server.set_prompt_response_text(&valid_k6_json());
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        let summarizer = crate::summarize::OpenCodeRemoteSummarizer::new(
            Arc::new(backend),
            tmp.path().join("opencode-scratch"),
        )
        .with_task_timeout(Duration::from_secs(2));

        let run = state
            .send_summary_run_with(accepted.inputs, &summarizer)
            .unwrap();
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        for name in names {
            assert_eq!(
                surface.matches(&format!("— {name}")).count(),
                1,
                "missing or duplicate heading for {name}"
            );
        }
        assert!(!surface.contains("historical"));
        assert!(!surface.contains("HISTORICAL_ONLY_BODY_SENTINEL"));
        assert!(
            !surface.contains("No se pudo procesar este archivo."),
            "READY selected Markdown must not degrade to the per-source failure copy: {surface}"
        );
        for name in names {
            assert!(
                surface.contains(&format!("{name}\nResumen de {name}")),
                "each selected source must keep its own summary: {surface}"
            );
        }
        let positions: Vec<usize> = [
            "2026-07-17-a.md",
            "2026-07-27-b.md",
            "2026-07-28-c.md",
            "2026-07-30-d.md",
            "2026-07-31-e.md",
        ]
        .iter()
        .map(|name| surface.find(name).unwrap())
        .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

        let prompts = server.prompt_texts();
        let document_prompts = prompts
            .iter()
            .filter(|prompt| prompt.contains("[E1]") && prompt.contains("BODY_SENTINEL"))
            .count();
        let synthesis_prompts = prompts
            .iter()
            .filter(|prompt| prompt.contains("[P1]"))
            .count();
        assert_eq!(
            document_prompts,
            5,
            "five selected READY sources must each generate a document node: {}",
            prompts.len()
        );
        assert_eq!(
            synthesis_prompts,
            2,
            "successful five-source plan must execute batch then global: {}",
            prompts.len()
        );
        assert_eq!(
            prompts.len(),
            document_prompts + synthesis_prompts,
            "remote prompts must be exactly the executed document+batch+global nodes"
        );
        let session_ids = server.created_session_ids();
        assert_eq!(
            session_ids.len(),
            prompts.len(),
            "each K6 remote call must own a distinct OpenCode scratch session: {session_ids:?}"
        );
        let unique_sessions: std::collections::HashSet<_> = session_ids.iter().collect();
        assert_eq!(unique_sessions.len(), session_ids.len());
        for (index, name) in names.iter().enumerate() {
            let sentinel = format!("SELECTED_{index}_BODY_SENTINEL");
            assert!(
                prompts.iter().any(|prompt| prompt.contains(&sentinel)),
                "{name} must contribute its own indexed chunks to a K6 request"
            );
        }
        assert_eq!(state.test_activity().raw_attachment_forwarding, 0);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(prompts.len()));
        assert_eq!(metrics.selected_evidence_count, None);
        assert!(metrics.material_count.unwrap() >= 6);
        assert_eq!(metrics.input_tokens, None);
        assert_eq!(metrics.output_tokens, None);

        let k6_before_reopen = state.test_activity().k6_calls;
        drop(state);
        let fresh = app(tmp.path());
        let after = fresh.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(after.remote_calls, metrics.remote_calls);
        assert_eq!(after.material_count, metrics.material_count);
        assert_eq!(
            fresh.test_activity(),
            TestActivity::default(),
            "reopening Conversation Details must not start provider/index/embed/K6 work"
        );
        assert_eq!(k6_before_reopen, 1);

        let cache_prompts = server.prompt_texts().len();
        let selected = fresh
            .open_project(&project.id)
            .unwrap()
            .messages
            .iter()
            .find(|message| message.role == "user")
            .unwrap()
            .material_ids
            .clone();
        let cache_backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config-cache"),
            0,
        );
        cache_backend.set_base_url(server.base_url());
        cache_backend.ensure_ready().unwrap();
        let cache_summarizer = crate::summarize::OpenCodeRemoteSummarizer::new(
            Arc::new(cache_backend),
            tmp.path().join("opencode-scratch"),
        )
        .with_task_timeout(Duration::from_secs(2));
        let cached = fresh
            .summarize_selected_sources(&project.id, &selected, &cache_summarizer)
            .unwrap();
        assert_eq!(cached.report.remote_calls, 0);
        assert!(cached.report.reused >= 5);
        assert_eq!(
            server.prompt_texts().len(),
            cache_prompts,
            "cache reuse must not issue another remote summary"
        );

        let logs = crate::session_log::list();
        let joined = logs
            .iter()
            .map(|entry| entry.message.clone())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!joined.contains("SELECTED_0_BODY_SENTINEL"));
        assert!(!joined.contains("HISTORICAL_ONLY_BODY_SENTINEL"));
        let project_json = std::fs::read_to_string(root.join("project.json")).unwrap();
        assert!(!project_json.contains("SELECTED_0_BODY_SENTINEL"));
        assert!(!project_json.contains("/home/"));
    }

    #[test]
    fn safe_filename_date_rejects_noise_and_invalid_calendar_values() {
        assert_eq!(
            super::safe_filename_date("2024-01-03-a.md"),
            Some((2024, 1, 3))
        );
        assert_eq!(
            super::safe_filename_date("2024_01_02-safe.md"),
            Some((2024, 1, 2))
        );
        assert_eq!(
            super::safe_filename_date("capture-202401011230-noise.md"),
            None
        );
        assert_eq!(super::safe_filename_date("2024-13-40-invalid.md"), None);
        assert_eq!(super::safe_filename_date("2024-02-30-bad.md"), None);
        assert_eq!(super::safe_filename_date("undated-first.md"), None);
    }

    /// A process can stop after the real terminal lifecycle persists its
    /// assistant result but before final accounting reaches the owning user
    /// message. The narrow test-only failpoint interrupts exactly that final
    /// durable write; the provider/assistant path itself is not simulated.
    #[test]
    fn assistant_before_metrics_crash_window_keeps_messages_and_leaves_metrics_unavailable() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let engine = FakeAgentEngine::new();
        engine.set_message("durable assistant answer".into());
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        let project = state.create_project("crash window").unwrap();
        state.fail_next_turn_metrics_persistence();
        assert!(
            state
                .send_message(&project.id, "durable user prompt", &[])
                .is_err(),
            "only the final metrics write is interrupted"
        );
        assert_eq!(
            engine
                .calls()
                .iter()
                .filter(|call| **call == project_agent::FakeCall::Send)
                .count(),
            1,
            "the original provider run occurred once"
        );

        let before = state.open_project(&project.id).unwrap();
        assert_eq!(before.messages.len(), 2);
        let user_turn_id = before.messages[0].id.clone();
        assert_eq!(before.messages[0].turn_metrics, None);
        assert_eq!(before.messages[1].role, "assistant");
        drop(state);

        let fresh_engine = FakeAgentEngine::new();
        let fresh = AppState::with_components(
            tmp.path().to_path_buf(),
            fresh_engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        let reopened = fresh.open_project(&project.id).unwrap();
        assert_eq!(reopened.messages.len(), 2, "no duplicate user or assistant");
        assert_eq!(reopened.messages[0].id, user_turn_id);
        assert_eq!(reopened.messages[0].text, "durable user prompt");
        assert_eq!(reopened.messages[1].text, "durable assistant answer");
        assert!(
            fresh.last_turn_metrics(&project.id).unwrap().is_none(),
            "unknown telemetry is unavailable, never fabricated as zero"
        );
        assert!(
            fresh_engine.calls().is_empty(),
            "reopen does not resend provider work solely to reconstruct telemetry"
        );
        assert_eq!(
            fresh.test_activity(),
            TestActivity::default(),
            "the durable read performs no provider/K6/index/retrieval/embedding persistence or raw attachment forwarding"
        );
    }
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
