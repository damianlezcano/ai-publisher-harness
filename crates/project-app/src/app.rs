//! Application facade: the Tauri-free application core that wires M1-M5.
//!
//! `AppState` composes `ProjectService` (project/material/creation CRUD), the
//! `PublicationManager` (publish/unpublish), and `AgentService` (agent tasks),
//! and exposes high-level, UI-oriented operations returning serializable DTOs
//! plus human-facing errors. The Tauri command layer is a thin adapter over
//! this facade; no domain logic lives in the frontend.

use std::collections::{HashMap, HashSet};
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
    ModelManifest, OrtEmbeddingProvider, RetrievalMode, SemanticProviderState,
    runtime_library_from_executable,
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

/// Maps a resolved sidecar location onto the [`AppConfig`] binary fields (M10
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
    /// One process-local, strictly local embedding provider. It is initialized
    /// only when the verified model and bundled runtime are available; tests
    /// inject a deterministic provider through the same seam.
    knowledge_provider: Mutex<Option<Box<dyn EmbeddingProvider + Send>>>,
    /// Shared OpenCode backend, used by the K6 remote summarizer to run
    /// bounded synthesis in a dedicated scratch session. `None` in DI test
    /// construction (`with_components`), which uses injected fake components
    /// and never runs remote summarization.
    summarizer_backend: Option<Arc<OpenCodeBackend>>,
    /// Shared OpenCode backend for the semantic intent classifier. Kept separate
    /// from `summarizer_backend` so DI tests that enable the K6 gate (with a
    /// non-classifier backend) do not accidentally enable the classifier.
    /// Production (`AppState::new`) points it at the same shared backend.
    classifier_backend: Option<Arc<OpenCodeBackend>>,
    /// Process-local handles for durable K6 operation ledgers. The map is not
    /// recovery state; SQLite remains authoritative after process loss.
    summary_runs: Mutex<HashMap<String, ActiveSummaryRun>>,
    /// Process-local set of accepted-import operation ids whose compact/generic
    /// synthesis is currently running IN THIS PROCESS. Used only to distinguish a
    /// live `synthesizing=true` phase from a stale flag left behind by a crash:
    /// reopening a project clears `synthesizing` for any operation that is not
    /// present here, so a crash can never leave a permanent "Generando
    /// resúmenes…" state. Never used to re-send a provider request.
    active_compact_synthesis: Mutex<std::collections::HashSet<String>>,
    /// Live isolated web-preview servers keyed by their single-use token. Each
    /// entry serves one immutable copy of a creation's `outputs/<id>` tree on a
    /// loopback-only, token-guarded endpoint (ADR-0010). Removed (and torn down)
    /// by `preview_close`.
    previews: Mutex<std::collections::HashMap<String, LivePreview>>,
    #[cfg(test)]
    test_activity: Mutex<TestActivity>,
    #[cfg(test)]
    fail_next_turn_metrics_persistence: Mutex<bool>,
    #[cfg(test)]
    interrupt_after_summary_completion: Mutex<bool>,
    #[cfg(test)]
    fail_next_final_summary_artifact: Mutex<bool>,
    /// Test-only seam: an injectable per-item remote summarizer used instead of
    /// the OpenCode scratch summarizer so the contextual follow-up tests can
    /// assert exact remote-call counts and exact output cardinality.
    #[cfg(test)]
    test_per_item_summarizer:
        Mutex<Option<Arc<dyn project_knowledge::RemoteSummarizer + Send + Sync>>>,
    /// Test-only seam: forces batching/word limits for the per-item path.
    #[cfg(test)]
    test_per_item_options: Mutex<Option<crate::per_item::PerItemExecutionOptions>>,
    /// Test-only seam: an injectable remote summarizer for the K6 summary route
    /// (`send_summary_run`), used instead of the OpenCode scratch summarizer so
    /// the staged same-turn-summary regression can run the full accepted-turn
    /// path without a live provider.
    #[cfg(test)]
    test_summarizer: Mutex<Option<Arc<dyn project_knowledge::RemoteSummarizer + Send + Sync>>>,
    /// Test-only seam: an injectable intent classifier used instead of the
    /// OpenCode classifier so production-seam tests can prove a turn invokes the
    /// classifier exactly once and that its result controls routing.
    /// Injectable intent classifier used instead of the OpenCode classifier.
    /// Production leaves this `None`. Tests use it to simulate multilingual
    /// semantic decisions without a live provider.
    test_classifier: Mutex<Option<Arc<dyn crate::classifier::IntentClassifier + Send + Sync>>>,
    /// Test-only observability: the most recent normalized routing decision made
    /// by `dispatch_message_run`. Lets the dispatch-equivalence tests prove the
    /// formerly divergent entry points converge on the same decision.
    #[cfg(test)]
    last_routing_decision: Mutex<Option<crate::intent::BoundRoute>>,
}

struct ActiveSummaryRun {
    operation_id: String,
    cancellation: Arc<crate::summarize::SummaryCancellation>,
}

/// Outcome of committing the final summary artifact and the terminal
/// `completed` transition as one explicit lifecycle step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryCommitOutcome {
    /// The operation owns the persisted final artifact as `completed`.
    Completed,
    /// A concurrent cancellation owned the terminal transition; the artifact
    /// is persisted but unreferenced and the operation is `cancelled`.
    Cancelled,
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

/// Test-only adapter so an injected `Arc<dyn RemoteSummarizer>` can be handed
/// to the per-item terminal which takes `&dyn RemoteSummarizer`.
#[cfg(test)]
struct TestSummarizerAdapter(Arc<dyn project_knowledge::RemoteSummarizer + Send + Sync>);

#[cfg(test)]
impl project_knowledge::RemoteSummarizer for TestSummarizerAdapter {
    fn summarize(
        &self,
        request: &project_knowledge::SummaryRequest,
    ) -> std::result::Result<project_knowledge::SummaryOutput, project_knowledge::SummaryFailure>
    {
        self.0.summarize(request)
    }

    fn summarize_controlled(
        &self,
        request: &project_knowledge::SummaryRequest,
        control: &dyn project_knowledge::SummaryExecutionControl,
    ) -> std::result::Result<project_knowledge::SummaryOutput, project_knowledge::SummaryFailure>
    {
        self.0.summarize_controlled(request, control)
    }
}

/// Adapter so an injected `Arc<dyn IntentClassifier>` can be handed to routing
/// which takes `&dyn IntentClassifier`.
struct TestClassifierAdapter(Arc<dyn crate::classifier::IntentClassifier + Send + Sync>);

impl crate::classifier::IntentClassifier for TestClassifierAdapter {
    fn classify(
        &self,
        input: &crate::classifier::ClassifierInput,
    ) -> Result<crate::intent::ClassifierDecision, crate::classifier::IntentClassificationError>
    {
        self.0.classify(input)
    }
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
        state.summarizer_backend = Some(Arc::clone(&backend));
        state.classifier_backend = Some(backend);
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
    /// A structured referent this turn produced (inventory list, thematic
    /// result) that must be persisted on the owning user message once it is
    /// durably appended. Never rendered prose; structural identities only.
    pending_referent: Option<project_core::TurnReferent>,
    /// Structural facts of a resolved creation-from-material turn. When set,
    /// this turn routes through the existing agent/artifact pipeline grounded
    /// on the creation Knowledge context (or a local clarification); it never
    /// performs top-K retrieval and its retrieval_mode stays `None`.
    creation: Option<crate::creation::CreationRunMeta>,
    /// The normalized bound route resolved once for this turn. Preparation and
    /// dispatch consume it; downstream engines never re-interpret the prompt.
    /// `None` only until the canonical resolver runs (e.g. the "without
    /// Knowledge" acceptance boundary).
    ///
    /// The contextual follow-up binding is a property of this route
    /// (`BoundRoute::followup`), never a separate `AgentRunInputs` field, so
    /// there is exactly one source of truth and it cannot diverge.
    route: Option<crate::intent::BoundRoute>,
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
    /// Truthful local-mode label when this turn never used a RAG retrieval
    /// mode (e.g. `inventory`). `None` for semantic/exhaustive/thematic turns.
    local_mode: Option<String>,
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

/// Result of the creation-from-material routing stage: a grounded
/// document-wide context ready for the existing agent/artifact pipeline, or a
/// local clarification (never top-K retrieval).
struct CreationPrepared {
    knowledge: AgentKnowledgeContext,
    metrics: TurnKnowledgeMetrics,
    meta: crate::creation::CreationRunMeta,
}

impl CreationPrepared {
    /// Local clarification: the user referenced a target that cannot be
    /// resolved. `remote_calls == 0`, `creations == 0`, no retrieval.
    fn clarify(message: String) -> Self {
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: 0,
            corpus_bytes: 0,
            corpus_utf8_chars: 0,
            corpus_est_tokens: 0,
            retrieval_candidate_count: Some(0),
            selected_evidence_count: Some(0),
            selected_evidence_bytes: Some(0),
            selected_evidence_utf8_chars: Some(0),
            evidence_est_tokens: Some(0),
            context_reduction_pct: Some(0),
            semantic_provider_state: project_knowledge::SemanticProviderState::NotRequested
                .as_str()
                .to_owned(),
            request_preparation_ms: None,
            retrieval_mode: None,
            local_mode: Some(crate::creation::CREATION_LOCAL_MODE.to_owned()),
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            lexical_hits: None,
            semantic_hits: None,
        };
        let knowledge = AgentKnowledgeContext {
            indexed_source_names: Vec::new(),
            entries: Vec::new(),
            evidence_budget_used: 0,
            evidence_budget_limit: 0,
            citation_map: Vec::new(),
            retrieval_mode: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            structural_note: None,
            local_answer: Some(message),
            authorize_negative: false,
            citation_source_names: Vec::new(),
            creation_from_material: true,
        };
        Self {
            knowledge,
            metrics: knowledge_metrics,
            meta: crate::creation::CreationRunMeta {
                target_count: 0,
                target_source_names: Vec::new(),
                context_strategy: "clarification",
                document_wide_est_tokens: 0,
                referent_type: "clarification",
                clarified: true,
            },
        }
    }
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

    /// True when this turn is a contextual per-item summary follow-up ("resumí
    /// cada uno") that must run through [`AppState::send_message_run`] instead
    /// of the whole-corpus K6 summary gate.
    ///
    /// The follow-up binding is read from the bound route, which is the single
    /// authoritative source of truth for the turn's follow-up scope.
    pub fn is_per_item_followup(&self) -> bool {
        self.route
            .as_ref()
            .and_then(|route| route.followup.as_ref())
            .is_some_and(|followup| {
                followup.action == crate::referent::FollowUpAction::PerItemSummary
            })
    }

    /// True when this turn is a resolved creation-from-material turn. It must
    /// run through the existing agent/artifact pipeline (or the local
    /// clarification path), never through the K6 summary gate or retrieval
    /// routing.
    pub fn is_creation_turn(&self) -> bool {
        self.creation.is_some()
    }

    /// OrdinaryChat never carries Knowledge processing. sqlite, embeddings,
    /// and leftover context from another path cannot reopen retrieval.
    fn clear_ordinary_chat_knowledge(&mut self) {
        self.knowledge = None;
        self.knowledge_metrics = None;
        self.pending_referent = None;
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
            classifier_backend: None,
            summary_runs: Mutex::new(HashMap::new()),
            active_compact_synthesis: Mutex::new(std::collections::HashSet::new()),
            previews: Mutex::new(std::collections::HashMap::new()),
            #[cfg(test)]
            test_activity: Mutex::new(TestActivity::default()),
            #[cfg(test)]
            fail_next_turn_metrics_persistence: Mutex::new(false),
            #[cfg(test)]
            interrupt_after_summary_completion: Mutex::new(false),
            #[cfg(test)]
            fail_next_final_summary_artifact: Mutex::new(false),
            #[cfg(test)]
            test_per_item_summarizer: Mutex::new(None),
            #[cfg(test)]
            test_per_item_options: Mutex::new(None),
            #[cfg(test)]
            test_summarizer: Mutex::new(None),
            test_classifier: Mutex::new(None),
            #[cfg(test)]
            last_routing_decision: Mutex::new(None),
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
        let creations = project
            .creations
            .iter()
            .map(|c| creation_view(c, &project.creations))
            .collect();
        let messages = project.messages.iter().map(message_view).collect();
        let publication = self.publication_status(id)?;
        let accepted_import =
            KnowledgeStore::open(self.base.join("projects").join(pid.as_str()), &pid)
                .ok()
                .and_then(|mut store| {
                    // Clear any stale "Generando resúmenes…" flag left behind by a
                    // crash before the ledger is surfaced (never a live phase).
                    self.reconcile_stale_synthesizing(&mut store);
                    store
                        .latest_accepted_import_operation()
                        .ok()
                        .flatten()
                        .map(|operation| {
                            let materials_ready =
                                active_import_materials_ready(&store, &operation).unwrap_or(0);
                            accepted_import_progress_view(&store, operation, materials_ready)
                        })
                });
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

    /// Returns durable, conversation-wide additive provider telemetry. It is
    /// derived from persisted per-turn records so reopening never relies on a
    /// process-local log. A missing provider field on any provider-using turn
    /// keeps that total unavailable rather than inventing a zero.
    pub fn accumulated_conversation_usage(
        &self,
        project_id: &str,
    ) -> AppResult<Option<crate::dtos::ConversationUsageTotalsView>> {
        let pid = parse_project_id(project_id)?;
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?;
        let turns: Vec<&TurnMetrics> = project
            .messages
            .iter()
            .filter(|message| message.role == MessageRole::User)
            .filter_map(|message| message.turn_metrics.as_ref())
            .collect();
        if turns.is_empty() {
            return Ok(None);
        }
        let latest_provider = turns
            .iter()
            .rev()
            .find_map(|metrics| metrics.provider.clone());
        let latest_model = turns.iter().rev().find_map(|metrics| metrics.model.clone());
        let sum_all_u64 = |field: fn(&TurnMetrics) -> Option<u64>| {
            turns.iter().try_fold(0_u64, |total, metrics| {
                field(metrics).and_then(|value| total.checked_add(value))
            })
        };
        let sum_provider_u64 = |field: fn(&TurnMetrics) -> Option<u64>| {
            turns
                .iter()
                .try_fold(0_u64, |total, metrics| match metrics.remote_calls {
                    Some(0) => Some(total),
                    Some(_) => field(metrics).and_then(|value| total.checked_add(value)),
                    None => None,
                })
        };
        let sum_provider_f64 = |field: fn(&TurnMetrics) -> Option<f64>| {
            turns
                .iter()
                .try_fold(0.0_f64, |total, metrics| match metrics.remote_calls {
                    Some(0) => Some(total),
                    Some(_) => field(metrics).map(|value| total + value),
                    None => None,
                })
        };
        let remote_calls = turns.iter().try_fold(0_usize, |total, metrics| {
            metrics
                .remote_calls
                .and_then(|value| total.checked_add(value))
        });
        let provider_actual = turns
            .iter()
            .any(|metrics| metrics.source.as_deref() == Some("provider_actual"));
        Ok(Some(crate::dtos::ConversationUsageTotalsView {
            provider: latest_provider,
            model: latest_model,
            input_tokens: sum_provider_u64(|metrics| metrics.input_tokens),
            output_tokens: sum_provider_u64(|metrics| metrics.output_tokens),
            cache_read_tokens: sum_provider_u64(|metrics| metrics.cache_read_tokens),
            cache_write_tokens: sum_provider_u64(|metrics| metrics.cache_write_tokens),
            total_tokens: sum_provider_u64(|metrics| metrics.total_tokens),
            cost_usd: sum_provider_f64(|metrics| metrics.cost_usd),
            turn_duration_ms: sum_all_u64(|metrics| metrics.turn_duration_ms),
            source: Some(
                if provider_actual {
                    "provider_actual"
                } else {
                    "unavailable"
                }
                .to_owned(),
            ),
            remote_calls,
        }))
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
    fn interrupt_after_summary_completion(&self) {
        *self
            .interrupt_after_summary_completion
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = true;
    }

    #[cfg(test)]
    fn fail_next_final_summary_artifact(&self) {
        *self
            .fail_next_final_summary_artifact
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = true;
    }

    fn take_interrupt_after_summary_completion(&self) -> bool {
        #[cfg(test)]
        {
            let mut flag = self
                .interrupt_after_summary_completion
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *flag {
                *flag = false;
                return true;
            }
        }
        false
    }

    fn take_fail_next_final_summary_artifact(&self) -> bool {
        #[cfg(test)]
        {
            let mut flag = self
                .fail_next_final_summary_artifact
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if *flag {
                *flag = false;
                return true;
            }
        }
        false
    }

    #[cfg(test)]
    fn test_activity(&self) -> TestActivity {
        self.test_activity
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn last_routing_decision(&self) -> Option<crate::intent::BoundRoute> {
        self.last_routing_decision
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn cancel_target_role(&self, project_id: &str) -> Option<&'static str> {
        self.agent.cancel_target_role(project_id)
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
                store.index_embeddings(provider, EMBEDDING_BATCH_SIZE)
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
        let operation = operation_id;
        let (embedded, reused, failure_state, persist_failure) = match self
            .with_local_embedding_provider(|provider| {
                #[cfg(test)]
                {
                    self.test_activity
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .embedding_persistence += 1;
                }
                // Bounded incremental progress: the store advances the durable
                // accepted-import ledger at ~1/s while this closure logs the
                // same sanitized counters. Never per-embedding IPC/SQLite churn.
                let ledger = operation.map(|id| project_knowledge::AcceptedImportEmbeddingLedger {
                    operation_id: id,
                });
                let mut report_progress =
                    |progress: project_knowledge::EmbeddingIndexProgress| {
                        let operation_id = operation.unwrap_or("none");
                        let kind = if progress.completed == 0 && progress.total > 0 {
                            "embedding_started"
                        } else if progress.total > 0 && progress.completed == progress.total {
                            "embedding_completed"
                        } else {
                            "embedding_progress"
                        };
                        let embeddings_per_sec = if progress.elapsed_ms >= 1_000 {
                            progress.completed as f64 / (progress.elapsed_ms as f64 / 1_000.0)
                        } else {
                            0.0
                        };
                        crate::session_log::record(
                            "INFO",
                            format!(
                                "[knowledge][perf] {kind} operation_id={operation_id} completed={} total={} created={} reused={} elapsed_ms={} embeddings_per_sec={embeddings_per_sec:.1}",
                                progress.completed,
                                progress.total,
                                progress.created,
                                progress.reused,
                                progress.elapsed_ms,
                            ),
                        );
                        if kind == "embedding_started" {
                            crate::session_log::record(
                                "INFO",
                                format!(
                                    "[knowledge][embedding] operation_id={operation_id} runtime=onnx intra_threads={} batch_size={}",
                                    project_knowledge::bounded_intra_threads(),
                                    EMBEDDING_BATCH_SIZE
                                ),
                            );
                        }
                    };
                store.index_embeddings_for_materials_reporting(
                    provider,
                    EMBEDDING_BATCH_SIZE,
                    &material_ids,
                    ledger,
                    Some(&mut report_progress),
                )
            }) {
            (Some(Ok(outcome)), _) => (Some(outcome.embedded), Some(outcome.reused), None, None),
            (Some(Err(error)), _) => {
                let state = embedding_index_failure_state(&error);
                let detail = error.embedding_persist_class("embedding_index");
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
            // Sanitized stage/class/kind + SQLite primary code only: never SQL
            // text, a path, a vector, or a document body.
            let sqlite_code = failure
                .sqlite_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "none".to_owned());
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge][embedding] stage={} failure_class={} sqlite_code={} kind={}",
                    failure.stage,
                    failure.class.as_str(),
                    sqlite_code,
                    failure.kind
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
            // Embedding counters stay in chunk/vector units end to end. The
            // embedding pass above already advanced `embedding_completed`,
            // `embeddings_created`, and `embeddings_reused` in chunk units
            // (see `EmbeddingIndexOutcome`); writing a Material-level counter
            // here (e.g. `indexed`) would make the durable embedding progress
            // jump backwards (31778/31778 -> 50/31778) and mix units. Preserve
            // the monotonic chunk-level values; on a failed pass, never regress
            // below the last durable embedding progress.
            let previous = store.accepted_import_operation(operation_id).ok().flatten();
            let keep_or = |live: Option<usize>, durable: usize| {
                live.map_or(durable, |value| value.max(durable))
            };
            let embedding_completed = keep_or(
                embedded,
                previous
                    .as_ref()
                    .map_or(0, |operation| operation.embedding_completed),
            );
            let embeddings_created = keep_or(
                embedded,
                previous
                    .as_ref()
                    .map_or(0, |operation| operation.embeddings_created),
            );
            let embeddings_reused = keep_or(
                reused,
                previous
                    .as_ref()
                    .map_or(0, |operation| operation.embeddings_reused),
            );
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
                embeddings_created,
                embeddings_reused,
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
        let all = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(&pid)
            .map_err(AppError::from_core)?
            .creations;
        Ok(creation_view(&creation, &all))
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
        route: &crate::intent::BoundRoute,
    ) -> AppResult<(
        Option<AgentKnowledgeContext>,
        Option<TurnKnowledgeMetrics>,
        Option<project_core::TurnReferent>,
    )> {
        // OrdinaryChat is the per-turn Knowledge negation. A persisted index,
        // embeddings, or prior RAG turn must not reopen retrieval here.
        if !route.uses_knowledge() {
            return Ok((None, None, None));
        }
        let root = self.base.join("projects").join(project_id.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return Ok((None, None, None));
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

        // A resolved contextual follow-up takes precedence over fresh
        // classification: it supplies the SCOPE (a prior MaterialSet) or the
        // exact ACTION (a prior ThemeSet) without re-running discovery. The
        // binding comes from the route; no prompt wording is re-interpreted.
        if let Some(followup) = &route.followup {
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge] contextual_followup=true referent_type={} referent_count={} origin_turn_id={} base_intent={} turn_kind={} resolution={}",
                    followup.referent_type(),
                    followup.referent_count(),
                    followup.origin_turn_id,
                    followup.action.base_intent(),
                    followup.action.turn_kind(),
                    followup.resolution.as_str(),
                ),
            );
            match (&followup.referent, followup.action) {
                (
                    project_core::TurnReferent::MaterialSet(set),
                    crate::referent::FollowUpAction::ScopedExhaustive,
                ) => {
                    return self.prepare_exhaustive_knowledge_context(
                        project_id,
                        user_query,
                        &store,
                        &corpus,
                        started,
                        Some(&set.material_ids),
                    );
                }
                (project_core::TurnReferent::ThemeSet(set), _) => {
                    return self.prepare_thematic_knowledge_context(
                        project_id,
                        user_query,
                        &store,
                        &corpus,
                        started,
                        Some(&set.theme_keys),
                    );
                }
                _ => {}
            }
        }

        // The normalized intent (already resolved once) selects the engine.
        match route.decision.intent {
            crate::intent::Intent::CorpusExhaustive => {
                return self.prepare_exhaustive_knowledge_context(
                    project_id, user_query, &store, &corpus, started, None,
                );
            }
            // Corpus-wide thematic synthesis. A corpus with fewer than two READY
            // documents cannot exhibit recurring themes across distinct meetings,
            // so it falls back to the compact K3/K4 semantic path (which reports
            // the truthful `normal` mode instead of claiming corpus-wide coverage).
            crate::intent::Intent::CorpusThematic if corpus.ready >= 2 => {
                return self.prepare_thematic_knowledge_context(
                    project_id, user_query, &store, &corpus, started, None,
                );
            }
            // Knowledge inventory is a LOCAL METADATA COMMAND, not a RAG mode: it
            // reads persisted KnowledgeStore metadata and short-circuits through
            // the local answer path. No semantic retrieval, lexical evidence, query
            // embedding, context budget, top-K, or provider synthesis happens.
            crate::intent::Intent::KnowledgeInventory => {
                return self.prepare_inventory_knowledge_context(
                    project_id, user_query, &store, &corpus, started,
                );
            }
            _ => {}
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
            local_mode: None,
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
                return Ok((None, Some(knowledge_metrics), None));
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
                    creation_from_material: false,
                }),
                Some(knowledge_metrics),
                None,
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
        let citation_source_names = citation_names_from_selected_evidence(&entries, false);
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
                citation_source_names,
                creation_from_material: false,
            }),
            Some(knowledge_metrics),
            None,
        ))
    }

    fn prepare_exhaustive_knowledge_context(
        &self,
        project_id: &ProjectId,
        user_query: &str,
        store: &KnowledgeStore,
        corpus: &project_knowledge::KnowledgeCorpusStats,
        started: std::time::Instant,
        material_scope: Option<&[String]>,
    ) -> AppResult<(
        Option<AgentKnowledgeContext>,
        Option<TurnKnowledgeMetrics>,
        Option<project_core::TurnReferent>,
    )> {
        let terms = crate::extract_presence_terms(user_query);
        let (search, semantic_state) = self.with_local_embedding_provider(|provider| {
            if let Some(scope) = material_scope {
                store.exhaustive_presence_search_scoped(user_query, &terms, Some(provider), scope)
            } else {
                store.exhaustive_presence_search(user_query, &terms, Some(provider))
            }
        });
        let report = match search {
            Some(Ok(report)) => report,
            Some(Err(_)) | None => {
                if let Some(scope) = material_scope {
                    store
                        .exhaustive_presence_search_scoped(user_query, &terms, None, scope)
                        .map_err(|_| {
                            AppError::new(
                                ErrorCode::Internal,
                                "No pudimos buscar el material de apoyo.",
                            )
                        })?
                } else {
                    store
                        .exhaustive_presence_search(user_query, &terms, None)
                        .map_err(|_| {
                            AppError::new(
                                ErrorCode::Internal,
                                "No pudimos buscar el material de apoyo.",
                            )
                        })?
                }
            }
        };
        // Fuentes and the remote evidence package may only contain selected
        // supporting chunks. Semantic expansion stays in inspection metrics
        // (`semantic_hits`) and must not become citations or fake evidence.
        let evidence_candidates = lexical_exhaustive_candidates(&report.candidates);
        let package = store
            .assemble_context(
                user_query,
                &evidence_candidates,
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
            local_mode: None,
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
                "[knowledge] retrieval mode=exhaustive coverage={} eligible_materials={} materials_inspected={} chunks_inspected={} phrase_count={} lexical_hits={} semantic_hits={} candidates={} evidence={} evidence_est_tokens={} naive_corpus_est_tokens={} context_reduction_pct_vs_naive_corpus={} semantic_state={}",
                report.coverage.as_str(),
                report.eligible_materials,
                report.materials_inspected,
                report.chunks_inspected,
                terms.len(),
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
        let citation_source_names = citation_names_from_selected_evidence(&entries, true);
        // A positive exhaustive result is a concrete, locally proven material
        // set. Persist opaque IDs from the scanner's lexical provenance, never
        // names parsed from the provider's prose. This replaces the old
        // accidental dependency on an OpenCode transcript for "de esas"
        // follow-ups.
        let pending_referent =
            material_set_from_grounded_candidates(&report.candidates, "exhaustive_result");
        // A negative global conclusion is authorized only when every eligible
        // READY material was inspected (complete coverage), no lexical hit was
        // found, and a concrete presence needle was actually extracted. Semantic
        // expansion (`semantic_hits`) never authorizes a negative and never
        // becomes positive evidence; it only surfaces conceptually related
        // candidates whose absence of the exact phrase was correctly verified.
        let negative_authorized = report.coverage == ExhaustiveCoverage::Complete
            && report.lexical_hits == 0
            && !terms.is_empty();
        let structural_note = Some(format!(
            "exhaustive_coverage={} eligible_materials={} materials_inspected={} chunks_inspected={} phrase_count={} lexical_hits={} semantic_hits={} selected_evidence_sources={} negative_authorized={}",
            report.coverage.as_str(),
            report.eligible_materials,
            report.materials_inspected,
            report.chunks_inspected,
            terms.len(),
            report.lexical_hits,
            report.semantic_hits,
            citation_source_names.len(),
            negative_authorized
        ));
        let local_answer = exhaustive_local_answer(&report, &terms);
        if indexed_source_names.is_empty() && entries.is_empty() && local_answer.is_none() {
            return Ok((None, Some(knowledge_metrics), None));
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
                authorize_negative: negative_authorized,
                citation_source_names,
                creation_from_material: false,
            }),
            Some(knowledge_metrics),
            pending_referent,
        ))
    }

    /// Corpus-wide thematic synthesis (distinct from normal K3/K4 and from the
    /// exhaustive presence scanner). Local-only aggregation: the store scans
    /// every READY chunk, reuses persisted per-document K6 summaries only to
    /// broaden candidate themes, ranks and caps a bounded recurring-theme set,
    /// and selects theme-local evidence from distinct supporting documents. The
    /// final answer is one bounded remote synthesis over that evidence. No
    /// embeddings are queried, no summaries are generated, and no per-document
    /// remote call happens.
    fn prepare_thematic_knowledge_context(
        &self,
        project_id: &ProjectId,
        user_query: &str,
        store: &KnowledgeStore,
        corpus: &project_knowledge::KnowledgeCorpusStats,
        started: std::time::Instant,
        theme_keys: Option<&[String]>,
    ) -> AppResult<(
        Option<AgentKnowledgeContext>,
        Option<TurnKnowledgeMetrics>,
        Option<project_core::TurnReferent>,
    )> {
        let report = match theme_keys {
            // A contextual follow-up to a previous CorpusThematic turn MUST
            // reuse the exact persisted theme keys; discovery/ranking is never
            // re-run and can never silently replace them.
            Some(keys) => store.thematic_evidence_for_themes(keys).map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos agregar los temas del material.",
                )
            })?,
            None => store.thematic_synthesis_evidence().map_err(|_| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos agregar los temas del material.",
                )
            })?,
        };
        let package = store
            .assemble_context(user_query, &report.candidates, thematic_assembly_options())
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
        // Provenance is exactly the evidence that actually entered the final
        // context: distinct sources represented in the bounded package.
        let mut citation_source_names = Vec::new();
        let mut seen = HashSet::new();
        let mut contributing_documents = 0usize;
        let mut contributing_document_ids = HashSet::new();
        for entry in &package.entries {
            if contributing_document_ids.insert(entry.document_id.clone()) {
                contributing_documents += 1;
            }
            let name = project_core::safe_file_name(&entry.source_name);
            if !name.contains('/') && !name.contains('\\') && seen.insert(name.clone()) {
                citation_source_names.push(name);
            }
        }
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: naive_corpus_est_tokens,
            retrieval_candidate_count: Some(report.thematic_candidates),
            selected_evidence_count: Some(package.entries.len()),
            selected_evidence_bytes: Some(evidence_bytes),
            selected_evidence_utf8_chars: Some(evidence_utf8_chars),
            evidence_est_tokens: Some(evidence_est_tokens),
            context_reduction_pct: Some(context_reduction_pct_vs_naive_corpus),
            semantic_provider_state: project_knowledge::SemanticProviderState::NotRequested
                .as_str()
                .to_owned(),
            request_preparation_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            retrieval_mode: Some(RetrievalMode::Thematic.as_str().to_owned()),
            local_mode: None,
            eligible_materials: Some(report.eligible_materials),
            materials_inspected: Some(contributing_documents),
            chunks_inspected: Some(report.chunks_inspected),
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
                "[knowledge] retrieval mode=thematic eligible_materials={} contributing_sources={} chunks_inspected={} thematic_candidates={} summaries_reused={} evidence_entries={} evidence_est_tokens={} naive_corpus_est_tokens={} context_reduction_pct_vs_naive_corpus={} semantic_state=not_requested",
                report.eligible_materials,
                contributing_documents,
                report.chunks_inspected,
                report.thematic_candidates,
                report.summaries_reused,
                package.entries.len(),
                evidence_est_tokens,
                naive_corpus_est_tokens,
                context_reduction_pct_vs_naive_corpus,
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
                evidence_kind: Some("thematic".to_owned()),
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
            "thematic_candidates={} eligible_materials={} contributing_sources={} chunks_inspected={} summaries_reused={}{}",
            report.thematic_candidates,
            report.eligible_materials,
            contributing_documents,
            report.chunks_inspected,
            report.summaries_reused,
            if theme_keys.is_some() {
                format!(" selected_themes={}", report.selected_themes.join(" | "))
            } else {
                String::new()
            },
        ));
        // Capture the exact resulting theme set as a durable ThemeSet referent
        // so a later "para cada tema..." follow-up reuses these exact keys.
        let pending_referent = if report.selected_themes.is_empty() {
            None
        } else {
            Some(project_core::TurnReferent::ThemeSet(
                project_core::ThemeSetReferent {
                    theme_keys: report.selected_themes.clone(),
                    display_labels: report.selected_themes.clone(),
                    origin_turn_id: String::new(),
                    source_names: report.contributing_source_names.clone(),
                },
            ))
        };
        if indexed_source_names.is_empty() && entries.is_empty() {
            return Ok((None, Some(knowledge_metrics), pending_referent));
        }
        Ok((
            Some(AgentKnowledgeContext {
                indexed_source_names,
                entries,
                evidence_budget_used: package.totals.estimated_budget_used,
                evidence_budget_limit: package.totals.estimated_budget_limit,
                citation_map,
                retrieval_mode: Some(RetrievalMode::Thematic.as_str().to_owned()),
                exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
                structural_note,
                local_answer: None,
                authorize_negative: false,
                citation_source_names,
                creation_from_material: false,
            }),
            Some(knowledge_metrics),
            pending_referent,
        ))
    }

    /// Pure Knowledge-inventory command. This is a LOCAL METADATA path and is
    /// deliberately not a RAG retrieval mode: no semantic retrieval, no lexical
    /// evidence, no query embeddings, no context budget, no top-K, no provider
    /// synthesis, and no OpenCode agent execution. It queries the persisted
    /// KnowledgeStore metadata and short-circuits through the existing
    /// local-answer path, so the send flow never touches the provider
    /// (`remote_calls == 0`, provider tokens == 0, semantic candidates == 0,
    /// query embeddings == 0). The default status semantics are READY; explicit
    /// "incluí los que fallaron" style phrasings may widen the filter.
    fn prepare_inventory_knowledge_context(
        &self,
        project_id: &ProjectId,
        user_query: &str,
        store: &KnowledgeStore,
        corpus: &project_knowledge::KnowledgeCorpusStats,
        started: std::time::Instant,
    ) -> AppResult<(
        Option<AgentKnowledgeContext>,
        Option<TurnKnowledgeMetrics>,
        Option<project_core::TurnReferent>,
    )> {
        // Intent::KnowledgeInventory is already authoritative at this point (the
        // engine was selected from the normalized route, not from prompt
        // wording). Execution therefore must NOT fail merely because the legacy
        // ES/EN keyword parser cannot recognize the wording (an unsupported
        // language). A turn with no more specific parseable action defaults to a
        // generic list action instead of an Internal error.
        let request = crate::inventory_request_or_list(user_query);
        let (local_answer, inventory_action, materials_ready, listed_records) =
            self.build_inventory_local_answer(store, &request, corpus);
        let indexed_source_names: Vec<String> = store
            .ready_source_names()
            .unwrap_or_default()
            .into_iter()
            .map(|name| project_core::safe_file_name(&name))
            .collect();
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: corpus.naive_corpus_est_tokens,
            retrieval_candidate_count: Some(0),
            selected_evidence_count: Some(0),
            selected_evidence_bytes: Some(0),
            selected_evidence_utf8_chars: Some(0),
            evidence_est_tokens: Some(0),
            context_reduction_pct: Some(0),
            semantic_provider_state: project_knowledge::SemanticProviderState::NotRequested
                .as_str()
                .to_owned(),
            request_preparation_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            retrieval_mode: None,
            local_mode: Some("inventory".to_owned()),
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            lexical_hits: None,
            semantic_hits: None,
        };
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge] local_mode=inventory inventory_action={inventory_action} materials_ready={materials_ready} remote_calls=0"
            ),
        );
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
                retrieval_mode: None,
                eligible_materials: knowledge_metrics.eligible_materials,
                materials_inspected: knowledge_metrics.materials_inspected,
                chunks_inspected: knowledge_metrics.chunks_inspected,
                exhaustive_coverage: knowledge_metrics.exhaustive_coverage.clone(),
                lexical_hits: knowledge_metrics.lexical_hits,
                semantic_hits: knowledge_metrics.semantic_hits,
            },
            format!(
                "[knowledge] local_mode=inventory inventory_action={inventory_action} materials_ready={materials_ready} remote_calls=0 semantic_candidates=0 query_embeddings=0"
            ),
        );
        // Capture the returned material set as a durable MaterialSet referent
        // so a later "resumí cada uno" / "de esos archivos..." follow-up
        // resolves the exact list. A pure inventory answer still uses zero
        // provider calls; the referent is structural metadata only.
        let pending_referent = if inventory_action == "list" && !listed_records.is_empty() {
            Some(project_core::TurnReferent::MaterialSet(
                project_core::MaterialSetReferent {
                    material_ids: listed_records
                        .iter()
                        .map(|record| record.material_id.clone())
                        .collect(),
                    source_names: listed_records
                        .iter()
                        .map(|record| record.source_name.clone())
                        .collect(),
                    origin_turn_id: String::new(),
                    produced_by: "inventory".to_owned(),
                },
            ))
        } else {
            None
        };
        Ok((
            Some(AgentKnowledgeContext {
                indexed_source_names,
                entries: Vec::new(),
                evidence_budget_used: 0,
                evidence_budget_limit: 0,
                citation_map: Vec::new(),
                retrieval_mode: None,
                exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
                structural_note: None,
                local_answer: Some(local_answer),
                authorize_negative: false,
                citation_source_names: Vec::new(),
                creation_from_material: false,
            }),
            Some(knowledge_metrics),
            pending_referent,
        ))
    }

    /// Builds the deterministic local inventory answer from the persisted
    /// KnowledgeStore. Returns (answer text, action label, READY count,
    /// listed READY records in rendered order for referent capture).
    fn build_inventory_local_answer(
        &self,
        store: &KnowledgeStore,
        request: &crate::retrieval_intent::InventoryRequest,
        corpus: &project_knowledge::KnowledgeCorpusStats,
    ) -> (
        String,
        &'static str,
        usize,
        Vec<project_knowledge::KnowledgeMaterialRecord>,
    ) {
        use crate::retrieval_intent::InventoryAction;
        use project_knowledge::{InventoryMembership, InventorySort, MaterialIndexState};
        match request.action {
            InventoryAction::List => {
                let sort = match request.sort {
                    crate::retrieval_intent::InventorySort::ChronologicalAsc => {
                        InventorySort::ChronologicalAsc
                    }
                    crate::retrieval_intent::InventorySort::ChronologicalDesc => {
                        InventorySort::ChronologicalDesc
                    }
                    crate::retrieval_intent::InventorySort::Alpha => InventorySort::Alpha,
                    crate::retrieval_intent::InventorySort::Unsorted => InventorySort::Alpha,
                };
                let records = store
                    .inventory_snapshot(Some(MaterialIndexState::Ready), sort)
                    .unwrap_or_default();
                let count = records.len();
                if records.is_empty() {
                    (
                        "Todavía no tenés materiales registrados en Knowledge.".to_owned(),
                        "list",
                        corpus.ready,
                        Vec::new(),
                    )
                } else {
                    let mut lines: Vec<String> = records
                        .iter()
                        .map(|record| record.source_name.clone())
                        .collect();
                    lines.dedup();
                    let mut answer = format!(
                        "Tenés {} materiales registrados y listos en Knowledge:\n",
                        lines.len()
                    );
                    for line in lines {
                        answer.push_str("- ");
                        answer.push_str(&line);
                        answer.push('\n');
                    }
                    (answer, "list", count, records)
                }
            }
            InventoryAction::Count => {
                let count = store
                    .inventory_count(Some(MaterialIndexState::Ready))
                    .unwrap_or_default();
                let answer = if count == 0 {
                    "Todavía no tenés materiales registrados en Knowledge.".to_owned()
                } else {
                    format!("Tenés {count} materiales registrados y listos en Knowledge.")
                };
                (answer, "count", count, Vec::new())
            }
            InventoryAction::Membership => {
                let needle = request.target.as_deref().unwrap_or_default();
                let outcome = store
                    .find_material_by_source_name(needle, Some(MaterialIndexState::Ready))
                    .unwrap_or(InventoryMembership::NotFound);
                let (answer, action_label) = match outcome {
                    InventoryMembership::Exact { material } => (
                        format!(
                            "Sí, tenés el material \"{}\" cargado en Knowledge.",
                            material.source_name
                        ),
                        "membership",
                    ),
                    InventoryMembership::NotFound => (
                        format!(
                            "No, no tenés ningún material con el nombre \"{needle}\" en Knowledge."
                        ),
                        "membership",
                    ),
                    InventoryMembership::Ambiguous { count } => (
                        format!(
                            "Hay {count} materiales con ese nombre en Knowledge; decime cuál querés consultar."
                        ),
                        "membership",
                    ),
                };
                (answer, action_label, corpus.ready, Vec::new())
            }
        }
    }

    /// Detects a creation-from-material turn, resolves its target, and prepares
    /// the creation-specific document-wide Knowledge context.
    ///
    /// Routing precedence (after contextual follow-up resolution, before the
    /// K6 summary gate and before retrieval routing):
    /// 1. a creation verb AND an artifact noun must both be present;
    /// 2. the target is resolved deterministically (current-turn READY
    ///    attachment, explicit filename, compatible prior `MaterialSet`);
    /// 3. a resolved target builds the bounded document-wide creation context;
    ///    an unresolved referenced target becomes a local clarification.
    ///
    /// Returns `None` when the turn is not a creation-from-material request
    /// (ordinary routing continues unchanged), or when the request is a generic
    /// creation with no referenced target (the existing agent pipeline keeps
    /// handling it).
    fn prepare_creation_turn(
        &self,
        project_id: &ProjectId,
        request: &crate::creation::CreationRequest,
        current_material_ids: &[String],
    ) -> AppResult<Option<CreationPrepared>> {
        let root = self.base.join("projects").join(project_id.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            // No persisted Knowledge index: a referenced target cannot resolve.
            // A generic creation with no target reference stays on the normal
            // agent pipeline.
            return match &request.target_cue {
                crate::creation::CreationTargetCue::ExplicitName(name) => Ok(Some(
                    CreationPrepared::clarify(format!(
                        "No encuentro un material llamado \"{name}\" en Knowledge."
                    )),
                )),
                crate::creation::CreationTargetCue::BareDemonstrative => Ok(Some(
                    CreationPrepared::clarify(
                        "¿Sobre qué material querés que cree eso? Adjuntá el archivo o decime su nombre."
                            .to_owned(),
                    ),
                )),
                _ => Ok(None),
            };
        }
        let store = KnowledgeStore::open(&root, project_id).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el material de apoyo.",
            )
        })?;
        let corpus = store
            .corpus_stats()
            .unwrap_or_else(|_| project_knowledge::KnowledgeCorpusStats::default());
        let started = std::time::Instant::now();
        let prior = self.prior_referents(project_id);
        let resolution = crate::creation::resolve_creation_targets(
            &store,
            request,
            current_material_ids,
            &prior,
        );
        match resolution {
            crate::creation::CreationResolution::Grounded {
                material_ids,
                source_names,
                referent_type,
            } => {
                let (knowledge, metrics, meta) = self.prepare_creation_knowledge_context(
                    &store,
                    &corpus,
                    &material_ids,
                    &source_names,
                    referent_type,
                    started,
                )?;
                Ok(Some(CreationPrepared {
                    knowledge,
                    metrics,
                    meta,
                }))
            }
            crate::creation::CreationResolution::Clarification { message } => {
                Ok(Some(CreationPrepared::clarify(message)))
            }
            crate::creation::CreationResolution::NotApplicable => Ok(None),
        }
    }

    /// Builds the document-wide creation Knowledge context for the resolved
    /// target materials. No re-embedding, no on-demand K6 synthesis, no
    /// re-ingestion: each target uses its persisted `Ready` document-level K6
    /// summary when present, otherwise a deterministic bounded document-wide
    /// representative coverage from its persisted chunks. Multiple targets are
    /// composed under a controlled total budget (`N × compact_representation`,
    /// never `N × document_size`). `retrieval_mode` stays `None`.
    #[allow(clippy::too_many_arguments)]
    fn prepare_creation_knowledge_context(
        &self,
        store: &KnowledgeStore,
        corpus: &project_knowledge::KnowledgeCorpusStats,
        material_ids: &[String],
        source_names: &[String],
        referent_type: &'static str,
        started: std::time::Instant,
    ) -> AppResult<(
        AgentKnowledgeContext,
        TurnKnowledgeMetrics,
        crate::creation::CreationRunMeta,
    )> {
        let multi_target = material_ids.len() > 1;
        let per_target_budget = if multi_target {
            crate::creation::CREATION_PER_TARGET_COMPACT_CHARS
        } else {
            crate::creation::CREATION_SINGLE_TARGET_MAX_CHARS
        };
        let mut entries: Vec<AgentKnowledgeEntry> = Vec::new();
        let mut citation_source_names: Vec<String> = Vec::new();
        let mut seen_names = HashSet::new();
        let mut used_representative = 0usize;
        for (index, material_id) in material_ids.iter().enumerate() {
            let source_name = source_names
                .get(index)
                .cloned()
                .unwrap_or_else(|| material_id.clone());
            let safe_name = project_core::safe_file_name(&source_name);
            let document_id = store.document_for_material(material_id).ok().flatten();
            let summary = document_id
                .as_ref()
                .and_then(|document_id| {
                    store
                        .ready_document_summary_text(document_id)
                        .ok()
                        .flatten()
                })
                .filter(|text| !text.trim().is_empty());
            let (text, from_summary) = if let Some(summary) = summary {
                (
                    crate::creation::bounded_text(&summary, per_target_budget),
                    true,
                )
            } else if let Some(document_id) = document_id.as_ref() {
                let chunks = store
                    .document_chunks(document_id, crate::creation::CREATION_CHUNK_FETCH_LIMIT)
                    .unwrap_or_default();
                let representative =
                    crate::creation::creation_document_representative(&chunks, per_target_budget);
                if representative.trim().is_empty() {
                    continue;
                }
                (representative, false)
            } else {
                continue;
            };
            if !from_summary {
                used_representative += 1;
            }
            if seen_names.insert(safe_name.clone()) {
                citation_source_names.push(safe_name.clone());
            }
            entries.push(AgentKnowledgeEntry {
                label: format!("E{}", entries.len() + 1),
                source_label: format!("S{}", entries.len() + 1),
                source_name: safe_name,
                chunk_label: format!("C{}", entries.len() + 1),
                line_start: None,
                line_end: None,
                heading_path: Vec::new(),
                text,
                source_id: Some(material_id.clone()),
                evidence_kind: Some("creation_material".to_owned()),
            });
        }
        // A multi-target request that resolves to zero document-wide
        // representatives cannot be grounded; clarify instead of fabricating.
        if entries.is_empty() {
            let prepared = CreationPrepared::clarify(
                "El material indicado no tiene contenido listo en Knowledge todavía. Volvé a intentarlo cuando esté procesado."
                    .to_owned(),
            );
            return Ok((prepared.knowledge, prepared.metrics, prepared.meta));
        }
        let context_strategy = if used_representative == 0 {
            "document_summary"
        } else {
            "representative"
        };
        let document_wide_est_tokens = entries
            .iter()
            .map(|entry| entry.text.len().max(3).div_ceil(3) + 24)
            .sum::<usize>();
        let total_text_chars: usize = entries.iter().map(|entry| entry.text.chars().count()).sum();
        let total_text_bytes: usize = entries.iter().map(|entry| entry.text.len()).sum();
        let structural_note = Some(format!(
            "creation_from_material=true target_count={} source_names={} context_strategy={context_strategy} document_wide_est_tokens={document_wide_est_tokens} | Basá la creación en el material indicado en el contexto de Knowledge. El material ya está disponible y listo en Knowledge; no respondas que el directorio está vacío.",
            material_ids.len(),
            citation_source_names.join(", "),
        ));
        let citation_map: Vec<AgentEvidenceProvenance> = entries
            .iter()
            .map(|entry| AgentEvidenceProvenance {
                label: entry.label.clone(),
                source_label: entry.source_label.clone(),
                chunk_label: entry.chunk_label.clone(),
            })
            .collect();
        let knowledge_metrics = TurnKnowledgeMetrics {
            material_count: corpus.material_count,
            corpus_bytes: corpus.corpus_bytes,
            corpus_utf8_chars: corpus.corpus_utf8_chars,
            corpus_est_tokens: corpus.naive_corpus_est_tokens,
            retrieval_candidate_count: Some(0),
            selected_evidence_count: Some(entries.len()),
            selected_evidence_bytes: Some(total_text_bytes),
            selected_evidence_utf8_chars: Some(total_text_chars),
            evidence_est_tokens: Some(document_wide_est_tokens),
            context_reduction_pct: None,
            semantic_provider_state: project_knowledge::SemanticProviderState::NotRequested
                .as_str()
                .to_owned(),
            request_preparation_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            retrieval_mode: None,
            local_mode: Some(crate::creation::CREATION_LOCAL_MODE.to_owned()),
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            lexical_hits: None,
            semantic_hits: None,
        };
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge] creation_from_material=true turn_kind=creation_from_material reason=creation_from_material target_count={} referent_type={referent_type} context_strategy={context_strategy} document_wide_est_tokens={document_wide_est_tokens} no_reembedding=true",
                material_ids.len(),
            ),
        );
        let meta = crate::creation::CreationRunMeta {
            target_count: material_ids.len(),
            target_source_names: citation_source_names.clone(),
            context_strategy,
            document_wide_est_tokens,
            referent_type,
            clarified: false,
        };
        let knowledge = AgentKnowledgeContext {
            indexed_source_names: citation_source_names.clone(),
            entries,
            evidence_budget_used: document_wide_est_tokens,
            evidence_budget_limit: if multi_target {
                crate::creation::CREATION_MULTI_TARGET_TOTAL_CHARS
            } else {
                crate::creation::CREATION_SINGLE_TARGET_MAX_CHARS
            },
            citation_map,
            retrieval_mode: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            structural_note,
            local_answer: None,
            authorize_negative: false,
            citation_source_names,
            creation_from_material: true,
        };
        Ok((knowledge, knowledge_metrics, meta))
    }

    /// Reads the persisted turn referents of a project (newest-first), used by
    /// both the contextual-follow-up resolver and creation-target resolution.
    fn prior_referents(&self, project_id: &ProjectId) -> Vec<crate::referent::PriorReferent> {
        let Some(project) = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
            .ok()
        else {
            return Vec::new();
        };
        project
            .messages
            .iter()
            .rev()
            .filter_map(|message| {
                message
                    .turn_referent
                    .as_ref()
                    .map(|referent| crate::referent::PriorReferent {
                        turn_id: message.id.as_str().to_owned(),
                        referent: referent.clone(),
                    })
            })
            .collect()
    }

    /// Builds the structural routing context for the normalized intent seam.
    /// Only counts, opaque identities, and small flags — never document bodies,
    /// chunk contents, or provider state.
    fn knowledge_routing_context(
        &self,
        project_id: &ProjectId,
        selected_material_ids: &[String],
        prior: &[crate::referent::PriorReferent],
    ) -> crate::intent::KnowledgeRoutingContext {
        let root = self.base.join("projects").join(project_id.as_str());
        let has_persisted_knowledge = root.join("knowledge/knowledge.sqlite").is_file();
        let mut persisted_material_count = 0;
        let mut persisted_ready_count = 0;
        let mut current_turn_ready_count = 0;
        if has_persisted_knowledge && let Ok(store) = KnowledgeStore::open(&root, project_id) {
            if let Ok(stats) = store.corpus_stats() {
                persisted_material_count = stats.material_count;
                persisted_ready_count = stats.ready;
            }
            let mut seen = std::collections::HashSet::new();
            current_turn_ready_count = selected_material_ids
                .iter()
                .filter(|id| {
                    if !seen.insert((*id).clone()) {
                        return false;
                    }
                    let Ok(material_id) = MaterialId::parse(*id) else {
                        return false;
                    };
                    store
                        .material_index_status(&material_id)
                        .ok()
                        .flatten()
                        .is_some_and(|status| {
                            status.state == project_knowledge::MaterialIndexState::Ready
                        })
                })
                .count();
        }
        let prior_referent_kind = prior.first().map(|item| match &item.referent {
            project_core::TurnReferent::MaterialSet(_) => {
                crate::intent::PriorReferentKind::MaterialSet
            }
            project_core::TurnReferent::ThemeSet(_) => crate::intent::PriorReferentKind::ThemeSet,
        });
        crate::intent::KnowledgeRoutingContext {
            current_turn_material_ids: selected_material_ids.to_vec(),
            current_turn_attachment_count: selected_material_ids.len(),
            current_turn_ready_count,
            persisted_material_count,
            persisted_ready_count,
            has_persisted_knowledge,
            remote_summarizer_available: self.summarizer_backend.is_some(),
            prior_referent_kind,
        }
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
            *provider =
                loaded.map(|provider| Box::new(provider) as Box<dyn EmbeddingProvider + Send>);
            if provider.is_none() {
                return (None, state);
            }
        }
        (
            provider.as_deref_mut().map(|provider| operation(provider)),
            project_knowledge::SemanticProviderState::Available,
        )
    }

    /// Test-only: inject a deterministic embedding provider through the same
    /// `with_local_embedding_provider` seam production uses, so app-level
    /// indexing tests can exercise the real batch/progress/persistence path
    /// without the bundled ONNX model.
    #[cfg(test)]
    pub fn set_local_embedding_provider(&self, provider: impl EmbeddingProvider + Send + 'static) {
        self.knowledge_provider
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(Box::new(provider));
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
            Ok(provider) => {
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge][embedding] runtime=onnx intra_threads={} batch_size={}",
                        provider.intra_threads(),
                        EMBEDDING_BATCH_SIZE
                    ),
                );
                (Some(provider), State::Available)
            }
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
        self.dispatch_message_run(inputs)
    }

    /// Canonical dispatch seam: routes an already-persisted turn through the
    /// normalized [`crate::intent::BoundRoute`] to its execution engine.
    ///
    /// This is the single source of truth that replaces the three formerly
    /// divergent routing blocks (`send_message`, `run_accepted_staged_turn_inner`,
    /// and the Tauri `agent_send`). It consumes the already-resolved normalized
    /// [`crate::intent::BoundRoute`] (resolving one when the caller did not) and
    /// maps the decision to the existing engines:
    ///
    /// - `WholeCorpusSummary` / `PerSourceSummary` -> K6 (`send_summary_run`);
    /// - `PerItemBatchAggregate` over a resolved follow-up -> `send_message_run`
    ///   (compact per-item summary over the prior referent set);
    /// - `PerItemBatchAggregate` over a fresh turn -> bounded per-item aggregate
    ///   over the deterministically resolved selected set;
    /// - `BatchSummary` over the current-turn set -> per-item aggregate
    ///   (`complete_current_turn_selected_batch_summary`);
    /// - everything else (ordinary chat, retrieval, inventory, exhaustive,
    ///   thematic, creation, scoped follow-ups) -> `send_message_run`.
    pub fn dispatch_message_run(&self, mut inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        // Invariant: before dispatch_route executes, AgentRunInputs MUST be
        // fully bound/applied for the route it carries. A caller that supplied
        // no route has its route resolved AND applied here (follow-up scope,
        // creation context, Knowledge preparation) before dispatch, so a route
        // can never be dispatched while only partially bound.
        if inputs.route.is_none() {
            let route = self.resolve_route(
                &inputs.project_id,
                &inputs.prompt,
                &inputs.selected_material_ids,
                inputs.model.as_ref(),
            );
            self.apply_route(&mut inputs, &route)?;
            inputs.route = Some(route);
        } else if inputs
            .route
            .as_ref()
            .is_some_and(|route| route.decision.intent == crate::intent::Intent::OrdinaryChat)
        {
            inputs.clear_ordinary_chat_knowledge();
        }
        #[cfg(test)]
        {
            *self
                .last_routing_decision
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = inputs.route.clone();
        }
        self.dispatch_route(inputs)
    }

    /// Maps an already-resolved [`crate::intent::BoundRoute`] to its execution
    /// engine. This is the deterministic binding step; no prompt wording is
    /// re-interpreted here. The route is read from `inputs.route`, so a turn can
    /// never be dispatched with a route it does not already carry.
    fn dispatch_route(&self, inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        let route = inputs
            .route
            .clone()
            .expect("route must be bound before dispatch");
        match route.decision.intent {
            crate::intent::Intent::WholeCorpusSummary => {
                self.send_summary_run(inputs, crate::intent::SummaryExecutionKind::WholeCorpus)
            }
            crate::intent::Intent::PerSourceSummary => self.dispatch_per_source_summary(inputs),
            // A per-item follow-up is a compact per-source summary over a
            // *prior* referent set; it runs through `send_message_run`, whose
            // follow-up branch reaches the per-item executor.
            crate::intent::Intent::PerItemBatchAggregate if route.followup.is_some() => {
                self.send_message_run(inputs)
            }
            // A fresh compact per-source summary resolves its exact material
            // scope deterministically and runs the bounded per-item aggregate.
            crate::intent::Intent::PerItemBatchAggregate => {
                self.dispatch_per_item_batch_aggregate(inputs)
            }
            // A per-item follow-up is a BatchSummary over a *prior* referent
            // set; it runs through `send_message_run`, whose follow-up branch
            // reaches the per-item executor.
            crate::intent::Intent::BatchSummary if route.followup.is_some() => {
                self.send_message_run(inputs)
            }
            crate::intent::Intent::BatchSummary => {
                self.complete_current_turn_selected_batch_summary(inputs)
            }
            _ => self.send_message_run(inputs),
        }
    }

    /// Deterministic material binding for a per-source summary turn.
    ///
    /// The semantic classifier only resolved `Intent::PerSourceSummary` (WHAT).
    /// Rust resolves WHICH materials here, with a fixed precedence:
    /// current-turn attachments, then the newest compatible prior MaterialSet
    /// referent, then the explicit active accepted-import set. With no bindable
    /// set this produces a truthful local no-selection answer instead of
    /// silently completing with `documents=0`.
    fn dispatch_per_source_summary(&self, mut inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        let scope =
            self.resolve_per_source_scope(&inputs.project_id, &inputs.selected_material_ids);
        match scope {
            crate::intent::PerSourceScope::CurrentTurn { material_ids } => {
                inputs.selected_material_ids = material_ids;
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge] summary_scope_source=current_turn selected_materials={} conversation_id={} turn_id={}",
                        inputs.selected_material_ids.len(),
                        inputs.project_id,
                        inputs.turn_id().unwrap_or("none"),
                    ),
                );
                self.send_summary_run(
                    inputs,
                    crate::intent::SummaryExecutionKind::SelectedPerSource,
                )
            }
            crate::intent::PerSourceScope::PriorMaterialSet {
                material_ids,
                origin_turn_id,
            } => {
                inputs.selected_material_ids = material_ids;
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge] summary_scope_source=prior_material_set selected_materials={} origin_turn_id={} conversation_id={} turn_id={}",
                        inputs.selected_material_ids.len(),
                        origin_turn_id,
                        inputs.project_id,
                        inputs.turn_id().unwrap_or("none"),
                    ),
                );
                self.send_summary_run(
                    inputs,
                    crate::intent::SummaryExecutionKind::SelectedPerSource,
                )
            }
            crate::intent::PerSourceScope::ConversationActiveMaterialSet { material_ids } => {
                inputs.selected_material_ids = material_ids;
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge] summary_scope_source=conversation_active_material_set selected_materials={} conversation_id={} turn_id={}",
                        inputs.selected_material_ids.len(),
                        inputs.project_id,
                        inputs.turn_id().unwrap_or("none"),
                    ),
                );
                self.send_summary_run(
                    inputs,
                    crate::intent::SummaryExecutionKind::SelectedPerSource,
                )
            }
            crate::intent::PerSourceScope::NoSelection => {
                self.complete_per_source_no_selection(inputs, "selected_per_source")
            }
        }
    }

    /// Deterministic compact per-source summary: `Intent::PerItemBatchAggregate`.
    ///
    /// This is the bounded compact per-file path. It resolves the exact selected
    /// material set with the same precedence as the deep per-source summary
    /// (current-turn attachments, then the newest compatible prior MaterialSet,
    /// then the conversation-active set), and then runs the bounded per-item
    /// aggregate executor. It never enters K6 and never makes one remote call
    /// per document.
    fn dispatch_per_item_batch_aggregate(
        &self,
        mut inputs: AgentRunInputs,
    ) -> AppResult<AgentRunView> {
        let scope =
            self.resolve_per_source_scope(&inputs.project_id, &inputs.selected_material_ids);
        let (material_ids, scope_source, origin_turn_id) = match scope {
            crate::intent::PerSourceScope::CurrentTurn { material_ids } => {
                (material_ids, "current_turn", None)
            }
            crate::intent::PerSourceScope::PriorMaterialSet {
                material_ids,
                origin_turn_id,
            } => (material_ids, "prior_material_set", Some(origin_turn_id)),
            crate::intent::PerSourceScope::ConversationActiveMaterialSet { material_ids } => {
                (material_ids, "conversation_active_material_set", None)
            }
            crate::intent::PerSourceScope::NoSelection => {
                return self.complete_per_source_no_selection(inputs, "per_item_batch_aggregate");
            }
        };
        inputs.selected_material_ids = material_ids.clone();
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge] summary_mode=per_item_batch_aggregate summary_scope_source={scope_source} selected_materials={} origin_turn_id={} conversation_id={} turn_id={}",
                material_ids.len(),
                origin_turn_id.as_deref().unwrap_or("none"),
                inputs.project_id,
                inputs.turn_id().unwrap_or("none"),
            ),
        );
        let source_names = self.resolve_material_source_names(&inputs.project_id, &material_ids);
        let origin_turn_id =
            origin_turn_id.unwrap_or_else(|| inputs.turn_id().unwrap_or("current_turn").to_owned());
        let followup = crate::referent::ContextualFollowUp {
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids,
                source_names,
                origin_turn_id: origin_turn_id.clone(),
                produced_by: "per_item_batch_aggregate".to_owned(),
            }),
            action: crate::referent::FollowUpAction::PerItemSummary,
            origin_turn_id,
            resolution: crate::referent::ResolutionReason::ActionCompatibleRecent,
        };
        self.complete_per_item_summary_turn(inputs, followup)
    }

    /// Resolves a stable display name per material id from the persisted project
    /// materials, falling back to the material id. Never exposes a path.
    fn resolve_material_source_names(
        &self,
        project_id: &ProjectId,
        material_ids: &[String],
    ) -> Vec<String> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
            .ok();
        material_ids
            .iter()
            .map(|id| {
                project
                    .as_ref()
                    .and_then(|project| {
                        project
                            .materials
                            .iter()
                            .find(|material| material.id.as_str() == id)
                    })
                    .map(|material| project_core::safe_file_name(&material.original_file_name))
                    .unwrap_or_else(|| id.clone())
            })
            .collect()
    }

    /// Resolves the deterministic material scope for a per-source summary from
    /// structural facts only. Current-turn attachments and prior MaterialSet
    /// referents are resolved without store access; the durable
    /// conversation-active material set is resolved last and never falls back
    /// to arbitrary project Knowledge.
    fn resolve_per_source_scope(
        &self,
        project_id: &ProjectId,
        current_material_ids: &[String],
    ) -> crate::intent::PerSourceScope {
        let prior = self.prior_referents(project_id);
        let scope = crate::intent::resolve_per_source_scope(current_material_ids, &prior);
        if scope != crate::intent::PerSourceScope::NoSelection {
            return scope;
        }
        let root = self.base.join("projects").join(project_id.as_str());
        let Ok(store) = KnowledgeStore::open(&root, project_id) else {
            return crate::intent::PerSourceScope::NoSelection;
        };
        let Ok(material_ids) = store.conversation_active_material_ids() else {
            return crate::intent::PerSourceScope::NoSelection;
        };
        let material_ids: Vec<String> = material_ids
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        if material_ids.is_empty() {
            crate::intent::PerSourceScope::NoSelection
        } else {
            crate::intent::PerSourceScope::ConversationActiveMaterialSet { material_ids }
        }
    }

    /// Best-effort durable record of the conversation-active material set: the
    /// exact set the user last explicitly attached/accepted. Only a non-empty
    /// set is ever written, so a failed/empty import (or unrelated ordinary
    /// chat) can never silently erase a valid active set.
    fn set_conversation_active_material_set(
        &self,
        project_id: &ProjectId,
        material_ids: &[String],
    ) {
        if material_ids.is_empty() {
            return;
        }
        let root = self.base.join("projects").join(project_id.as_str());
        let Ok(mut store) = KnowledgeStore::open(&root, project_id) else {
            return;
        };
        let ids: Vec<MaterialId> = material_ids
            .iter()
            .filter_map(|id| MaterialId::parse(id).ok())
            .collect();
        if ids.len() != material_ids.len() {
            return;
        }
        if store.set_conversation_active_material_set(&ids).is_err() {
            crate::session_log::record(
                "WARN",
                format!(
                    "[knowledge] conversation_active_material_set_write_failed conversation_id={project_id}"
                ),
            );
        }
    }

    /// Truthful local no-selection outcome for a per-source summary request that
    /// has no bindable material set. It never reaches K6, never creates a summary
    /// operation, and never emits a silent `completed` with `documents=0`.
    fn complete_per_source_no_selection(
        &self,
        inputs: AgentRunInputs,
        summary_mode: &str,
    ) -> AppResult<AgentRunView> {
        let project_id = inputs.project_id.clone();
        let durable_turn_id = inputs.turn_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let started = std::time::Instant::now();
        let text = PER_SOURCE_NO_SELECTION_ANSWER.to_owned();
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge] summary_scope_source=none selected_materials=0 summary_mode={summary_mode} status=no_selection conversation_id={} turn_id={}",
                project_id,
                turn_id.as_deref().unwrap_or("none"),
            ),
        );
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(
                &project_id,
                &text,
                MessageStatus::Ok,
                &[],
                durable_turn_id.clone(),
            )
            .map_err(AppError::from_core)?;
        self.persist_completed_turn_metrics(
            &project_id,
            durable_turn_id.as_ref(),
            per_source_no_selection_turn_metrics(
                inputs.model.as_ref(),
                started.elapsed().as_millis(),
            ),
        )?;
        Ok(AgentRunView {
            status: "completed".to_owned(),
            turn_id,
            registered_creation_ids: Vec::new(),
            message: Some(text),
        })
    }

    /// Whole-project summary turn (K6): the user message is already persisted by
    /// [`Self::send_message_persist`]; this runs the bounded hierarchical
    /// summarization (never sending the corpus) and appends the user-facing
    /// global summary as the assistant message. It never creates a normal agent
    /// run or a scratch chat turn.
    pub fn send_summary_run(
        &self,
        inputs: AgentRunInputs,
        kind: crate::intent::SummaryExecutionKind,
    ) -> AppResult<AgentRunView> {
        use crate::summarize::OpenCodeRemoteSummarizer;
        #[cfg(test)]
        if let Some(summarizer) = self
            .test_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return self.send_summary_run_with(inputs, &*summarizer, kind);
        }
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let control = Arc::new(crate::summarize::SummaryCancellation::default());
        let operation_id = summary_operation_id(inputs.turn_id.as_ref().map(|id| id.as_str()));
        let project_key = inputs.project_id.as_str().to_owned();
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                inputs.project_id.as_str().to_owned(),
                ActiveSummaryRun {
                    operation_id,
                    cancellation: Arc::clone(&control),
                },
            );
        let mut summarizer =
            OpenCodeRemoteSummarizer::new(Arc::clone(backend), self.base.join("opencode-scratch"))
                .with_cancellation(control);
        if let Some(model) = &inputs.model {
            summarizer = summarizer.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        let result = self.send_summary_run_with(inputs, &summarizer, kind);
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&project_key);
        result
    }

    /// Shared K6 terminal lifecycle. Production and deterministic tests use
    /// this same assistant/metrics persistence path. `kind` is the normalized
    /// summary route already resolved before entering K6; this terminal never
    /// re-derives WholeCorpus vs SelectedPerSource from the prompt wording.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn send_summary_run_with(
        &self,
        inputs: AgentRunInputs,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
        kind: crate::intent::SummaryExecutionKind,
    ) -> AppResult<AgentRunView> {
        // Defense-in-depth: a per-source summary with an empty selected set must
        // never silently reach K6 and complete with documents=0. The production
        // dispatch resolves NoSelection before this terminal, but direct
        // internal callers must obey the same invariant.
        if kind == crate::intent::SummaryExecutionKind::SelectedPerSource
            && inputs.selected_material_ids.is_empty()
        {
            return self.complete_per_source_no_selection(inputs, "selected_per_source");
        }
        #[cfg(test)]
        {
            self.test_activity
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .k6_calls += 1;
        }
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        {
            let mut runs = self.summary_runs.lock().unwrap_or_else(|e| e.into_inner());
            runs.entry(project_id.as_str().to_owned())
                .or_insert_with(|| ActiveSummaryRun {
                    operation_id: summary_operation_id(turn_id.as_deref()),
                    cancellation: Arc::new(crate::summarize::SummaryCancellation::default()),
                });
        }
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
        let selected_per_source = kind == crate::intent::SummaryExecutionKind::SelectedPerSource;
        let operation_id = {
            let mut store = KnowledgeStore::open(
                self.base.join("projects").join(project_id.as_str()),
                &project_id,
            )
            .map_err(|_| AppError::internal("No pudimos preparar el resumen."))?;
            let id = self
                .summary_runs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(project_id.as_str())
                .map(|run| run.operation_id.clone())
                .unwrap_or_else(|| summary_operation_id(turn_id.as_deref()));
            let scope = if selected_per_source {
                "selected_sources"
            } else {
                "project"
            };
            let model_identity = inputs_model
                .as_ref()
                .map(|m| format!("{}/{}", m.provider_id, m.model_id));
            // Material ids are stable user-facing slots, but re-ingestion can
            // repoint `material_sources.material_id` at a new content-addressed
            // document. Compatibility therefore includes the current immutable
            // document identity, not merely the mutable material id.
            let compatibility_sources = if selected_per_source {
                inputs
                    .selected_material_ids
                    .iter()
                    .map(|material_id| {
                        let document = store
                            .document_for_material(material_id)
                            .ok()
                            .flatten()
                            .unwrap_or_else(|| "unavailable".to_owned());
                        format!("{material_id}:{document}")
                    })
                    .collect::<Vec<_>>()
            } else {
                store
                    .summary_document_levels()
                    .map_err(|_| AppError::internal("No pudimos preparar el resumen."))?
                    .into_iter()
                    .map(|(document_id, _)| document_id)
                    .collect::<Vec<_>>()
            };
            let fingerprint = summary_operation_fingerprint(
                scope,
                &compatibility_sources,
                model_identity.as_deref(),
            );
            let operation = store
                .create_summary_operation(
                    &id,
                    turn_id.as_deref(),
                    scope,
                    &inputs.selected_material_ids,
                    &fingerprint,
                    model_identity.as_deref(),
                )
                .map_err(|_| AppError::internal("No pudimos preparar el resumen."))?;
            if operation.compatibility_fingerprint != fingerprint {
                // Never reinterpret an existing operation after its source
                // identity, model, scope, or contract compatibility changed.
                mark_summary_operation_incompatible(&mut store, &operation)?;
                return Ok(AgentRunView {
                    status: "failed".to_owned(),
                    turn_id,
                    registered_creation_ids: Vec::new(),
                    message: Some(
                        "El resumen anterior ya no es compatible con los materiales actuales."
                            .to_owned(),
                    ),
                });
            }
            let operation = store
                .reconcile_summary_operation_after_restart(&operation.operation_id)
                .map_err(|_| AppError::internal("No pudimos preparar el resumen."))?;
            // Positive allow-list: only a pending or running operation may
            // execute K6. Every other state is terminal or requires an explicit
            // action; none of them falls through into synthesis.
            match operation.status {
                project_knowledge::SummaryOperationStatus::Cancelled => {
                    return Ok(AgentRunView {
                        status: "cancelled".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some("El resumen se canceló.".to_owned()),
                    });
                }
                project_knowledge::SummaryOperationStatus::RetryRequired => {
                    return Ok(AgentRunView { status: "failed".to_owned(), turn_id, registered_creation_ids: Vec::new(), message: Some("El resultado anterior quedó pendiente; reintentá el resumen de forma explícita.".to_owned()) });
                }
                project_knowledge::SummaryOperationStatus::Failed => {
                    return Ok(AgentRunView {
                        status: "failed".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some(
                            "Ese resumen falló; reintentá de forma explícita.".to_owned(),
                        ),
                    });
                }
                project_knowledge::SummaryOperationStatus::Stale => {
                    return Ok(AgentRunView {
                        status: "failed".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some(
                            "Ese resumen ya no es compatible con sus materiales.".to_owned(),
                        ),
                    });
                }
                project_knowledge::SummaryOperationStatus::Completed => {
                    // Artifact-only: reuse the durable final surface; never
                    // re-run provider work for an already-completed operation.
                    let surface = self.completed_summary_surface(&mut store, &operation)?;
                    drop(store);
                    self.persist_assistant_from_summary_if_missing(
                        &project_id,
                        turn_id.as_deref(),
                        &surface,
                    )?;
                    return Ok(AgentRunView {
                        status: "completed".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some(surface),
                    });
                }
                project_knowledge::SummaryOperationStatus::Pending
                | project_knowledge::SummaryOperationStatus::Running => {}
            }
            id
        };
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][summary-operation] operation_id={operation_id} turn_id={} status=running scope={} resume=false",
                turn_id.as_deref().unwrap_or("none"),
                if selected_per_source {
                    "selected_sources"
                } else {
                    "project"
                }
            ),
        );
        if selected_per_source {
            // The authoritative scope source (`summary_scope_source=...`) is
            // already recorded by the dispatch layer; this terminal must not
            // re-assert a scope that may actually be prior/active/none.
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge] summary_mode=selected_per_source materials={} retrieval_mode=none query_embeddings=0 raw_forwarding=false conversation_id={} turn_id={}",
                    inputs.selected_material_ids.len(),
                    project_id,
                    turn_id.as_deref().unwrap_or("none"),
                ),
            );
        }
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
                if self.summary_cancelled(project_id.as_str())
                    || error.message == "El resumen se canceló."
                {
                    self.finish_summary_operation(
                        &project_id,
                        &operation_id,
                        project_knowledge::SummaryOperationStatus::Cancelled,
                        None,
                        &SummarizationReportView {
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
                        },
                        None,
                    )?;
                    return Ok(AgentRunView {
                        status: "cancelled".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some("El resumen se canceló.".to_owned()),
                    });
                }
                self.finish_summary_operation(
                    &project_id,
                    &operation_id,
                    project_knowledge::SummaryOperationStatus::Failed,
                    Some("summary_failure"),
                    &SummarizationReportView {
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
                    },
                    None,
                )?;
                let text = error.message.clone();
                self.projects
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .append_assistant_message(
                        &project_id,
                        &text,
                        MessageStatus::Failed,
                        &[],
                        durable_turn_id.clone(),
                    )
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
            match self.commit_final_summary_artifact(
                &project_id,
                &operation_id,
                "",
                inputs_model.as_ref(),
                &answer.report,
            ) {
                Ok(SummaryCommitOutcome::Completed) => {}
                Ok(SummaryCommitOutcome::Cancelled) => {
                    return Ok(AgentRunView {
                        status: "cancelled".to_owned(),
                        turn_id,
                        registered_creation_ids: Vec::new(),
                        message: Some("El resumen se canceló.".to_owned()),
                    });
                }
                Err(error) => return Err(error),
            }
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
        // Commit the exact locally-rendered surface before marking the
        // operation complete. Recovery consumes this typed artifact directly.
        match self.commit_final_summary_artifact(
            &project_id,
            &operation_id,
            &surface,
            inputs_model.as_ref(),
            &answer.report,
        ) {
            Ok(SummaryCommitOutcome::Completed) => {}
            Ok(SummaryCommitOutcome::Cancelled) => {
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge][summary-operation] operation_id={operation_id} status=cancelled cancelled=true"
                    ),
                );
                return Ok(AgentRunView {
                    status: "cancelled".to_owned(),
                    turn_id,
                    registered_creation_ids: Vec::new(),
                    message: Some("El resumen se canceló.".to_owned()),
                });
            }
            Err(error) => return Err(error),
        }
        if self.take_interrupt_after_summary_completion() {
            return Ok(AgentRunView {
                status: "completed".to_owned(),
                turn_id,
                registered_creation_ids: Vec::new(),
                message: None,
            });
        }
        self.persist_assistant_from_summary_if_missing(&project_id, turn_id.as_deref(), &surface)?;
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
        // Persist any referent this turn produced (inventory list / thematic
        // result) on the owning user message so a later follow-up resolves it
        // even after restart. The origin turn id is the durable message id.
        if let Some(referent) = inputs.pending_referent.take() {
            let turn_id = inputs.turn_id.take().expect("turn id");
            let referent = with_referent_origin(referent, turn_id.as_str());
            self.projects
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_turn_referent(&inputs.project_id, &turn_id, referent)
                .map_err(AppError::from_core)?;
            inputs.turn_id = Some(turn_id);
        }
        // A non-empty explicit attachment set becomes the conversation-active
        // set for later no-attachment compatible operations.
        self.set_conversation_active_material_set(&inputs.project_id, attachment_ids);
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
        // A duplicate-in-batch source is already represented by its first
        // occurrence: `accept_staged_materials` returns the same Material id for
        // both. The user message must reference each Material exactly once, so
        // de-duplicate the accepted identities (preserving first-occurrence
        // order) before persisting the turn. Otherwise `append_user_message`
        // rejects the whole turn with a duplicate-material error and leaves the
        // durable operation orphaned at accepted/turn_id=NULL/prepared=0.
        let mut attachment_ids = Vec::new();
        let mut seen_material_ids = std::collections::HashSet::new();
        for item in &report.items {
            if let Some(id) = &item.material_id
                && seen_material_ids.insert(id.clone())
            {
                attachment_ids.push(id.clone());
            }
        }
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
        crate::session_log::record(
            "INFO",
            format!(
                "turn accepted (staged) conversation_id={} message_id={} materials={} chars={}",
                pid,
                inputs.turn_id.as_ref().expect("turn id"),
                material_ids.len(),
                prompt.chars().count()
            ),
        );
        // Every selected path remains accounted for by the operation. A
        // duplicate can legitimately bind the already durable Material; it is
        // not silently discarded merely because no new copy was needed. The
        // first durable `copied > 0` write and the turn link happen together,
        // so a crash between acceptance and this point leaves `prepared == 0`
        // and can never fabricate a recoverable operation without its turn.
        if let Ok(mut store) = KnowledgeStore::open(&root, &pid) {
            // `total` counts every selected file except within-batch
            // duplicates (the same content hash selected twice). Those collapse
            // to one Material, so they must not be double-counted in the
            // progress denominators; genuine failures stay in `total` and are
            // reported separately by the `failed` counter.
            let duplicate_in_batch = report
                .items
                .iter()
                .filter(|item| item.status == "duplicate_in_batch")
                .count();
            let distinct_total =
                (staged_paths.len() + staged_images.len()).saturating_sub(duplicate_in_batch);
            let _ = store.set_accepted_import_operation_total(&operation_id, distinct_total);
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
        // The explicitly accepted material set becomes the conversation-active
        // set for later no-attachment compatible operations. Only non-empty
        // sets are recorded, so a zero-material turn never erases the prior set.
        self.set_conversation_active_material_set(&pid, &attachment_ids);
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
        // Resolve the semantic intent and its deterministic bindings exactly
        // once (after indexing, so readiness facts are final), then prepare the
        // Knowledge/creation context from that normalized route.
        let route = self.resolve_route(
            &accepted.inputs.project_id,
            &accepted.inputs.prompt,
            &accepted.inputs.selected_material_ids,
            accepted.inputs.model.as_ref(),
        );
        if let Err(error) = self.apply_route(&mut accepted.inputs, &route) {
            self.finish_accepted_import_operation(
                &accepted.inputs.project_id,
                &accepted.operation_id,
                false,
            );
            return Err(error);
        }
        // A derived Knowledge referent (inventory list / thematic result) is
        // persisted on the owning user message so a later follow-up resolves it
        // even after restart.
        if let Some(referent) = accepted.inputs.pending_referent.clone()
            && let Some(turn_id) = accepted.inputs.turn_id.as_ref()
        {
            let referent = with_referent_origin(referent, turn_id.as_str());
            self.projects
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .set_turn_referent(&accepted.inputs.project_id, turn_id, referent)
                .map_err(AppError::from_core)?;
            accepted.inputs.pending_referent = None;
        }
        accepted.inputs.route = Some(route);
        let project_id = accepted.inputs.project_id.clone();
        // The durable ledger now says that the remote terminal outcome is
        // unknown. A process loss after this point must never auto-resend a
        // possibly completed provider request.
        self.update_accepted_import_agent_state(
            &project_id,
            &accepted.operation_id,
            project_knowledge::AcceptedImportAgentState::StartedOutcomeUnknown,
        );
        // Truthful synthesis phase: a compact/generic per-item summary runs the
        // bounded aggregate executor after embeddings complete. Mark the phase
        // so the UI shows "Generando resúmenes..." instead of a stale
        // "99% · N de N archivos listos". The K6 deep route is never marked.
        let synthesis_phase = if let Some(route) = accepted.inputs.route.as_ref()
            && matches!(
                route.decision.intent,
                crate::intent::Intent::BatchSummary | crate::intent::Intent::PerItemBatchAggregate
            ) {
            self.set_accepted_import_synthesizing(&project_id, &accepted.operation_id, true);
            // Track the live in-process owner so reopen-recovery can distinguish
            // a running phase from a stale flag left behind by a crash.
            self.active_compact_synthesis
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(accepted.operation_id.clone());
            true
        } else {
            false
        };
        // All routing converges on the canonical normalized dispatch seam. The
        // route was resolved and bound above; `dispatch_message_run` consumes it
        // and maps it to the same engines the previous inline dispatch reached.
        let run = self.dispatch_message_run(accepted.inputs);
        // The synthesis phase (and its live-owner marker) is released on every
        // exit path so a stale flag can never survive an ordinary finish.
        if synthesis_phase {
            self.active_compact_synthesis
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&accepted.operation_id);
        }
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
                // A pre-turn interruption has no recoverable prompt: terminate
                // the operation cleanly so it never re-fires a futile resume on
                // a later launch. It is a terminal abandonment, never a
                // retryable local interruption.
                self.update_accepted_import_agent_state(
                    &pid,
                    operation_id,
                    project_knowledge::AcceptedImportAgentState::FailedTerminal,
                );
                return Err(AppError::new(
                    ErrorCode::RecoveryNoTurn,
                    "El envío se interrumpió antes de confirmarse; volvé a enviarlo.",
                ));
            }
        };
        // Once a K6 operation exists it is the authority for the summary
        // remote boundary. Completed recovery is artifact-only and must
        // recompute the current compatibility fingerprint; it never returns a
        // stale final surface and never starts provider work.
        if let Ok(Some(summary_operation)) = store.summary_operation_for_turn(turn_id) {
            let mut summary_store = KnowledgeStore::open(&root, &pid)
                .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
            let summary_operation = summary_store
                .reconcile_summary_operation_after_restart(&summary_operation.operation_id)
                .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
            let selected_model = self.selected_model_ref()?;
            let current_model = selected_model.as_ref().map(model_identity_string);
            if self.summary_operation_inputs_changed(
                &summary_store,
                &summary_operation,
                current_model.as_deref(),
            )? {
                // Completed stays immutable. Pending/Running/Failed/RetryRequired
                // become Stale so ordinary resume cannot execute them later.
                mark_summary_operation_incompatible(&mut summary_store, &summary_operation)?;
                return Err(AppError::new(
                    ErrorCode::SummaryIncompatible,
                    "Ese resumen ya no es compatible con sus materiales.",
                ));
            }
            match summary_operation.status {
                project_knowledge::SummaryOperationStatus::Completed => {
                    let surface =
                        self.completed_summary_surface(&mut summary_store, &summary_operation)?;
                    drop(summary_store);
                    self.persist_assistant_from_summary_if_missing(&pid, Some(turn_id), &surface)?;
                    self.update_accepted_import_agent_state(
                        &pid,
                        operation_id,
                        project_knowledge::AcceptedImportAgentState::Completed,
                    );
                    self.finish_accepted_import_operation(&pid, operation_id, true);
                    return Ok(AgentRunView {
                        status: "completed".to_owned(),
                        turn_id: Some(turn_id.to_owned()),
                        registered_creation_ids: Vec::new(),
                        message: Some(surface),
                    });
                }
                project_knowledge::SummaryOperationStatus::RetryRequired
                | project_knowledge::SummaryOperationStatus::Failed => {
                    return Err(AppError::invalid(
                        "El resultado anterior quedó pendiente; reintentá el resumen de forma explícita.",
                    ));
                }
                project_knowledge::SummaryOperationStatus::Cancelled
                | project_knowledge::SummaryOperationStatus::Stale => {
                    return Err(AppError::invalid(
                        "Ese resumen ya no se puede continuar automáticamente.",
                    ));
                }
                project_knowledge::SummaryOperationStatus::Pending
                | project_knowledge::SummaryOperationStatus::Running => {
                    if operation.agent_state
                        != project_knowledge::AcceptedImportAgentState::NotStarted
                    {
                        drop(summary_store);
                        let answer = self.resume_summary_operation_with_backend(
                            project_id,
                            &summary_operation.operation_id,
                            selected_model.as_ref(),
                        )?;
                        let surface = answer.summarize_surface_text().unwrap_or_else(|| {
                            "No encontramos materiales listos para resumir.".to_owned()
                        });
                        self.projects
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .append_assistant_message(
                                &pid,
                                &surface,
                                MessageStatus::Ok,
                                &[],
                                MessageId::parse(turn_id).ok(),
                            )
                            .map_err(AppError::from_core)?;
                        self.update_accepted_import_agent_state(
                            &pid,
                            operation_id,
                            project_knowledge::AcceptedImportAgentState::Completed,
                        );
                        self.finish_accepted_import_operation(&pid, operation_id, true);
                        crate::session_log::record(
                            "INFO",
                            format!(
                                "[knowledge][summary-operation] operation_id={} status=completed resume=true",
                                summary_operation.operation_id
                            ),
                        );
                        return Ok(AgentRunView {
                            status: "completed".to_owned(),
                            turn_id: Some(turn_id.to_owned()),
                            registered_creation_ids: Vec::new(),
                            message: Some(surface),
                        });
                    }
                }
            }
        }
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
            let _ = store.set_accepted_import_synthesizing(operation_id, false);
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

    /// Marks the truthful synthesis phase on the durable accepted-import ledger.
    fn set_accepted_import_synthesizing(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        synthesizing: bool,
    ) {
        let root = self.base.join("projects").join(project_id.as_str());
        if let Ok(mut store) = KnowledgeStore::open(&root, project_id) {
            let _ = store.set_accepted_import_synthesizing(operation_id, synthesizing);
        }
    }

    /// Reconciles a stale `synthesizing` flag on reopen.
    ///
    /// A compact/generic synthesis phase sets `synthesizing=true` on the durable
    /// ledger and clears it on ordinary completion. If the process crashes after
    /// setting the flag but before clearing it, the flag remains `true` with no
    /// live in-process owner, and the frontend would show an endless
    /// "Generando resúmenes…" state. This clears the flag for any operation that
    /// is NOT currently being synthesized in THIS process (tracked in
    /// `active_compact_synthesis`), so the reopen is truthful: the operation
    /// stays in its interrupted/unknown state and the user can re-run the
    /// compact request explicitly. It never re-sends a provider request.
    fn reconcile_stale_synthesizing(&self, store: &mut KnowledgeStore) {
        let Some(operation) = store.latest_accepted_import_operation().ok().flatten() else {
            return;
        };
        if !operation.synthesizing {
            return;
        }
        let live = self
            .active_compact_synthesis
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(&operation.operation_id);
        if live {
            return;
        }
        let _ = store.set_accepted_import_synthesizing(&operation.operation_id, false);
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][synthesis] stale_synthesizing_recovered operation_id={} agent_state={}",
                operation.operation_id,
                accepted_import_agent_state_str(operation.agent_state),
            ),
        );
    }

    /// Runs the agent using the prepared inputs and appends an assistant
    /// message reflecting the outcome (`ok`, `failed`, or `cancelled`).
    pub fn send_message_run(&self, mut inputs: AgentRunInputs) -> AppResult<AgentRunView> {
        if inputs
            .route
            .as_ref()
            .is_some_and(|route| route.decision.intent == crate::intent::Intent::OrdinaryChat)
        {
            inputs.clear_ordinary_chat_knowledge();
        }
        // A contextual per-item follow-up resolves before the local-answer
        // short-circuit and before any provider chat call: it executes its own
        // bounded aggregate synthesis over the exact previous MaterialSet.
        //
        // The follow-up binding is read from the bound route (`route.followup`),
        // the single authoritative source of truth for follow-up scope.
        let per_item_followup = inputs
            .route
            .as_ref()
            .and_then(|route| route.followup.clone())
            .filter(|followup| followup.action == crate::referent::FollowUpAction::PerItemSummary);
        if let Some(followup) = per_item_followup {
            return self.complete_per_item_summary_turn(inputs, followup);
        }
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
        let followup_for_metrics = inputs
            .route
            .as_ref()
            .and_then(|route| route.followup.clone());
        let creation_for_metrics = inputs.creation.clone();
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
                crate::session_log::record_prompt_context(
                    crate::session_log::PromptContextMetrics {
                        conversation_id: project_id.as_str().to_owned(),
                        turn_id: turn_id.as_deref().unwrap_or("unavailable").to_owned(),
                        user_prompt_est_tokens: result.prompt_telemetry.user_prompt_est_tokens,
                        conversation_history_est_tokens: result
                            .prompt_telemetry
                            .conversation_history_est_tokens,
                        knowledge_context_est_tokens: result
                            .prompt_telemetry
                            .knowledge_context_est_tokens,
                        system_context_est_tokens: result
                            .prompt_telemetry
                            .system_context_est_tokens,
                        rag_attachment_count: result.prompt_telemetry.rag_attachment_count,
                        raw_attachment_count: result.prompt_telemetry.raw_attachment_count,
                        serialized_request_est_tokens: result
                            .prompt_telemetry
                            .serialized_request_est_tokens,
                        fresh_session: result.prompt_telemetry.fresh_session,
                        session_role: result.prompt_telemetry.session_role.to_owned(),
                        session_reused: result.prompt_telemetry.session_reused,
                        session_rotated: result.prompt_telemetry.session_rotated,
                        rotation_reason: result
                            .prompt_telemetry
                            .rotation_reason
                            .unwrap_or("")
                            .to_owned(),
                        cache_invalidated: result.prompt_telemetry.cache_invalidated,
                        conversation_context_messages: result
                            .prompt_telemetry
                            .conversation_context_messages,
                        conversation_context_chars: result
                            .prompt_telemetry
                            .conversation_context_chars,
                    },
                );
                let usage_reason = if creation_for_metrics.is_some() {
                    crate::creation::CREATION_REASON
                } else {
                    "normal_chat"
                };
                record_turn_usage(
                    project_id.as_str(),
                    turn_id.as_deref(),
                    model_ref.as_ref(),
                    &result.task.usage,
                    started.elapsed().as_millis(),
                    usage_reason,
                    additional_attachment_route,
                );
                if let Some(creation) = &creation_for_metrics {
                    crate::session_log::record(
                        "INFO",
                        format!(
                            "[knowledge] creation_from_material=true turn_kind={} reason={} target_count={} referent_type={} context_strategy={} document_wide_est_tokens={} creation_calls=1 creations={} no_reembedding=true",
                            crate::creation::CREATION_LOCAL_MODE,
                            crate::creation::CREATION_REASON,
                            creation.target_count,
                            creation.referent_type,
                            creation.context_strategy,
                            creation.document_wide_est_tokens,
                            result.registered.len(),
                        ),
                    );
                }
                let creation_ids: Vec<CreationId> = result
                    .registered
                    .iter()
                    .map(|id| parse_creation_id(id))
                    .collect::<AppResult<Vec<_>>>()?;
                let mut text = assistant_reply_text(
                    result.task.message.as_deref(),
                    !result.registered.is_empty(),
                );
                let source_names = grounded_source_names(knowledge_for_citations.as_ref());
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
                    .append_assistant_message(
                        &project_id,
                        &text,
                        status,
                        &creation_ids,
                        durable_turn_id.clone(),
                    )
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
                            followup_for_metrics.as_ref(),
                            creation_for_metrics.as_ref(),
                            source_names,
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
                    .append_assistant_message(
                        &project_id,
                        &text,
                        MessageStatus::Cancelled,
                        &[],
                        durable_turn_id.clone(),
                    )
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
                    .append_assistant_message(
                        &project_id,
                        &err.message,
                        MessageStatus::Failed,
                        &[],
                        durable_turn_id.clone(),
                    )
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
        // A pure Knowledge-inventory turn is a local metadata command, never a
        // normal chat provider call. The usage reason must be truthful.
        let is_inventory = knowledge_metrics
            .as_ref()
            .and_then(|metrics| metrics.local_mode.as_deref())
            == Some("inventory");
        // A creation-from-material clarification is also local: the referenced
        // target could not be resolved, so no retrieval and no provider call.
        let creation = inputs.creation.clone();
        let is_creation = creation.is_some();
        let usage_reason = if is_inventory {
            "inventory"
        } else if is_creation {
            crate::creation::CREATION_REASON
        } else {
            "normal_chat"
        };
        let text = local;
        let source_names = grounded_source_names(inputs.knowledge.as_ref());
        let started = std::time::Instant::now();
        let usage = RemoteUsage::default();
        record_turn_usage(
            project_id.as_str(),
            turn_id.as_deref(),
            model_ref.as_ref(),
            &usage,
            started.elapsed().as_millis(),
            usage_reason,
            false,
        );
        if let Some(creation) = &creation {
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge] creation_from_material=true turn_kind={} reason={} target_count={} referent_type={} context_strategy={} document_wide_est_tokens={} creation_calls=0 creations=0 no_reembedding=true clarified=true",
                    crate::creation::CREATION_LOCAL_MODE,
                    crate::creation::CREATION_REASON,
                    creation.target_count,
                    creation.referent_type,
                    creation.context_strategy,
                    creation.document_wide_est_tokens,
                ),
            );
        }
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(
                &project_id,
                &text,
                MessageStatus::Ok,
                &[],
                durable_turn_id.clone(),
            )
            .map_err(AppError::from_core)?;
        let mut metrics = completed_normal_turn_metrics(
            model_ref.as_ref(),
            &usage,
            started.elapsed().as_millis(),
            knowledge_metrics.as_ref(),
            None,
            creation.as_ref(),
            source_names,
        );
        metrics.remote_calls = Some(0);
        metrics.source = Some("unavailable".to_owned());
        metrics.local_mode = knowledge_metrics.and_then(|metrics| metrics.local_mode.clone());
        self.persist_completed_turn_metrics(&project_id, durable_turn_id.as_ref(), metrics)?;
        Ok(AgentRunView {
            status: "completed".to_owned(),
            turn_id,
            registered_creation_ids: Vec::new(),
            message: Some(text),
        })
    }

    /// Terminal path for a contextual per-item summary follow-up ("resumí cada
    /// uno" over a previous MaterialSet). Builds the production summarizer and
    /// delegates to [`Self::complete_per_item_summary_turn_with`].
    fn complete_per_item_summary_turn(
        &self,
        inputs: AgentRunInputs,
        followup: crate::referent::ContextualFollowUp,
    ) -> AppResult<AgentRunView> {
        let summarizer = self.per_item_summarizer(inputs.model.as_ref())?;
        #[cfg(test)]
        let options = self.effective_per_item_options();
        #[cfg(not(test))]
        let options = crate::per_item::PerItemExecutionOptions::default();
        self.complete_per_item_summary_turn_with(inputs, followup, summarizer.as_ref(), options)
    }

    /// Generic current-turn summaries use the same bounded aggregate executor
    /// as contextual per-item summaries, but construct their exact scope from
    /// this turn's persisted selected ids. `produced_by` is an internal routing
    /// tag only; this synthetic referent is never persisted as conversation
    /// history.
    fn complete_current_turn_selected_batch_summary(
        &self,
        inputs: AgentRunInputs,
    ) -> AppResult<AgentRunView> {
        let source_names = inputs
            .attachments
            .iter()
            .map(|attachment| project_core::safe_file_name(&attachment.display_name))
            .collect();
        let origin_turn_id = inputs.turn_id().unwrap_or("current_turn").to_owned();
        let followup = crate::referent::ContextualFollowUp {
            referent: project_core::TurnReferent::MaterialSet(project_core::MaterialSetReferent {
                material_ids: inputs.selected_material_ids.clone(),
                source_names,
                origin_turn_id: origin_turn_id.clone(),
                produced_by: "current_turn_selected_batch".to_owned(),
            }),
            action: crate::referent::FollowUpAction::PerItemSummary,
            origin_turn_id,
            resolution: crate::referent::ResolutionReason::ActionCompatibleRecent,
        };
        self.complete_per_item_summary_turn(inputs, followup)
    }

    /// Shared per-item terminal lifecycle. Production and deterministic tests
    /// use this same assistant/metrics persistence path.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn complete_per_item_summary_turn_with(
        &self,
        inputs: AgentRunInputs,
        followup: crate::referent::ContextualFollowUp,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
        options: crate::per_item::PerItemExecutionOptions,
    ) -> AppResult<AgentRunView> {
        use crate::per_item::{
            PER_ITEM_MISSING, PER_ITEM_SYNTHESIS_FAILED, PerItemAccounting, PerItemMaterial,
            PerItemOutcome, bound_representative, bound_source_name, build_per_item_request,
            chunk_representative, pack_per_item_batches, parse_per_item_output,
            per_item_word_limit, render_per_item_outcomes,
        };
        let project_id = inputs.project_id.clone();
        let turn_id = inputs.turn_id.as_ref().map(ToString::to_string);
        let durable_turn_id = inputs.turn_id.clone();
        let model_ref = inputs.model.clone();
        let started = std::time::Instant::now();
        let root = self.base.join("projects").join(project_id.as_str());
        let store = KnowledgeStore::open(&root, &project_id).map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let corpus = store.corpus_stats().unwrap_or_default();
        let project_core::TurnReferent::MaterialSet(set) = &followup.referent else {
            return Err(AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            ));
        };
        let referent_count = set.material_ids.len();
        let per_item_batch_aggregate = set.produced_by == "per_item_batch_aggregate";
        let current_turn_selected_batch = set.produced_by == "current_turn_selected_batch";
        // Truthful route label: the current-turn generic aggregate, the new
        // compact per-source aggregate, or a contextual per-item follow-up.
        let summary_mode = if current_turn_selected_batch {
            "selected_batch_aggregate"
        } else if per_item_batch_aggregate {
            "per_item_batch_aggregate"
        } else {
            "per_item_summary"
        };

        // Re-resolve the referent defensively. Deduplicated material identities
        // keep their original order; a material that no longer resolves to
        // READY content keeps an explicit failure slot (cardinality is never
        // silently reduced).
        let mut seen = std::collections::HashSet::new();
        let mut materials: Vec<PerItemMaterial> = Vec::new();
        let mut representations_reused = 0usize;
        let mut representations_generated_locally = 0usize;
        for (material_id, source_name) in set.material_ids.iter().zip(&set.source_names) {
            if !seen.insert(material_id.clone()) {
                continue;
            }
            let document_id = store.document_for_material(material_id).ok().flatten();
            // Prefer a persisted `Ready` document-level summary (explicit reuse,
            // telemetry-visible, never synthesized on demand). Otherwise build a
            // deterministic bounded chunk representative locally.
            let reused_summary = document_id
                .as_ref()
                .and_then(|document_id| {
                    store
                        .ready_document_summary_text(document_id)
                        .ok()
                        .flatten()
                })
                .filter(|text| !text.trim().is_empty());
            let representative = if let Some(summary) = reused_summary {
                representations_reused += 1;
                bound_representative(&summary)
            } else {
                let chunk_representative = document_id
                    .and_then(|document_id| {
                        store
                            .document_chunks(&document_id, 256)
                            .ok()
                            .map(|chunks| chunk_representative(&chunks))
                    })
                    .unwrap_or_default();
                if !chunk_representative.trim().is_empty() {
                    representations_generated_locally += 1;
                }
                bound_representative(&chunk_representative)
            };
            materials.push(PerItemMaterial {
                key: format!("M{:03}", materials.len() + 1),
                material_id: material_id.clone(),
                source_name: bound_source_name(source_name),
                representative,
                order: materials.len(),
            });
        }

        let max_words = per_item_word_limit(&inputs.prompt, options);
        // Outcomes keep the exact referent order: every material gets a slot at
        // its own position, so a missing material never reorders the others.
        let mut slots: Vec<Option<PerItemOutcome>> = (0..materials.len()).map(|_| None).collect();
        let mut accounting = PerItemAccounting::default();

        // Materials without content get an explicit missing slot (in place) and
        // never cross the remote boundary.
        let mut remote: Vec<PerItemMaterial> = Vec::new();
        for material in materials {
            if material.representative.trim().is_empty() {
                slots[material.order] = Some(PerItemOutcome {
                    material_id: material.material_id,
                    source_name: material.source_name,
                    summary: PER_ITEM_MISSING.to_owned(),
                    fallback: true,
                });
            } else {
                remote.push(material);
            }
        }

        // Bounded aggregate batches: remote-call count is a function of the
        // provider-input budget (items per call AND hard input units), never of
        // the material count and never one call per document.
        let items_requested = remote.len();
        let mut items_generated = 0usize;
        let mut items_failed = 0usize;
        let mut batches = 0usize;
        let mut max_batch_estimated_units = 0usize;
        let packed = pack_per_item_batches(&remote, options);
        for batch in &packed {
            batches += 1;
            let request = build_per_item_request(batch, max_words);
            accounting.add_units(request.estimated_units());
            max_batch_estimated_units = max_batch_estimated_units.max(request.estimated_units());
            let batch_keys: Vec<String> = batch.iter().map(|item| item.key.clone()).collect();
            let parsed = match summarizer.summarize(&request) {
                Ok(output) => {
                    accounting.add_request(&output.usage);
                    parse_per_item_output(&output.text, &batch_keys, max_words)
                }
                Err(_) => {
                    accounting.remote_calls += 1;
                    std::collections::BTreeMap::new()
                }
            };
            for item in batch {
                let (summary, fallback) = match parsed.get(&item.key) {
                    Some(summary) => {
                        items_generated += 1;
                        (summary.clone(), false)
                    }
                    None => {
                        items_failed += 1;
                        (PER_ITEM_SYNTHESIS_FAILED.to_owned(), true)
                    }
                };
                slots[item.order] = Some(PerItemOutcome {
                    material_id: item.material_id.clone(),
                    source_name: item.source_name.clone(),
                    summary,
                    fallback,
                });
            }
        }
        // Exact-cardinality accounting invariant: every requested item resolved
        // to either a generated summary or an explicit synthesis-failure slot.
        debug_assert_eq!(items_generated + items_failed, items_requested);

        let outcomes: Vec<PerItemOutcome> = slots
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::Internal,
                    "No pudimos preparar el resumen del material.",
                )
            })?;

        // Blocking cardinality invariant: every referent material has exactly
        // one slot, even when it is an explicit fallback.
        debug_assert_eq!(outcomes.len(), referent_count);
        if outcomes.len() != referent_count {
            return Err(AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            ));
        }

        let text = render_per_item_outcomes(&outcomes);
        // `items_completed` is the legacy "successfully synthesized" counter,
        // kept for compatibility and equal to `items_generated`; `items_failed`
        // is the explicit partial-success signal (requested - generated).
        let items_completed = items_generated;
        let request_budget_limit = options.max_serialized_bytes_per_call;
        crate::session_log::record(
            "INFO",
            if current_turn_selected_batch {
                format!(
                    "[knowledge] summary_scope=current_turn_materials summary_mode={summary_mode} selected_materials={} per_item_batches={} remote_summary_calls={} items_requested={} items_completed={} items_generated={} items_failed={} request_budget_limit={} max_batch_estimated_units={} representations_reused={} representations_generated_locally={} retrieval_mode=none query_embeddings=0 raw_forwarding=false estimated_input_units={}",
                    referent_count,
                    batches,
                    accounting.remote_calls,
                    items_requested,
                    items_completed,
                    items_generated,
                    items_failed,
                    request_budget_limit,
                    max_batch_estimated_units,
                    representations_reused,
                    representations_generated_locally,
                    accounting.estimated_input_units,
                )
            } else if per_item_batch_aggregate {
                format!(
                    "[knowledge] summary_mode={summary_mode} selected_materials={} per_item_batches={} remote_summary_calls={} items_requested={} items_completed={} items_generated={} items_failed={} request_budget_limit={} max_batch_estimated_units={} representations_reused={} representations_generated_locally={} retrieval_mode=none query_embeddings=0 raw_forwarding=false estimated_input_units={}",
                    referent_count,
                    batches,
                    accounting.remote_calls,
                    items_requested,
                    items_completed,
                    items_generated,
                    items_failed,
                    request_budget_limit,
                    max_batch_estimated_units,
                    representations_reused,
                    representations_generated_locally,
                    accounting.estimated_input_units,
                )
            } else {
                format!(
                    "[knowledge] contextual_followup=true referent_type={} referent_count={} origin_turn_id={} base_intent={} turn_kind={summary_mode} resolution={} remote_calls={} per_item_batches={} items_requested={} items_completed={} items_generated={} items_failed={} request_budget_limit={} max_batch_estimated_units={} representations_reused={} representations_generated_locally={} estimated_input_units={}",
                    followup.referent_type(),
                    referent_count,
                    followup.origin_turn_id,
                    followup.action.base_intent(),
                    followup.resolution.as_str(),
                    accounting.remote_calls,
                    batches,
                    items_requested,
                    items_completed,
                    items_generated,
                    items_failed,
                    request_budget_limit,
                    max_batch_estimated_units,
                    representations_reused,
                    representations_generated_locally,
                    accounting.estimated_input_units,
                )
            },
        );
        crate::session_log::record_usage(crate::session_log::SessionUsage {
            conversation_id: project_id.as_str().to_owned(),
            turn_id: turn_id.clone().unwrap_or_else(|| "unavailable".to_owned()),
            provider: model_ref
                .as_ref()
                .map(|model| model.provider_id.clone())
                .unwrap_or_else(|| "unavailable".to_owned()),
            model: model_ref
                .as_ref()
                .map(|model| model.model_id.clone())
                .unwrap_or_else(|| "unavailable".to_owned()),
            input_tokens: accounting.input_tokens,
            output_tokens: accounting.output_tokens,
            cache_read_tokens: accounting.cache_read_tokens,
            cache_write_tokens: accounting.cache_write_tokens,
            total_tokens: None,
            cost_usd: accounting.cost_usd,
            turn_duration_ms: Some(started.elapsed().as_millis()),
            source: if accounting.provider_actual {
                "provider_actual".to_owned()
            } else {
                "unavailable".to_owned()
            },
            remote_calls: Some(accounting.remote_calls),
            reason: summary_mode.to_owned(),
            additional_attachment_route: false,
        });
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(
                &project_id,
                &text,
                MessageStatus::Ok,
                &[],
                durable_turn_id.clone(),
            )
            .map_err(AppError::from_core)?;
        let (provider, model) = provider_identity(model_ref.as_ref());
        let metrics = TurnMetrics {
            provider,
            model,
            input_tokens: accounting.input_tokens,
            output_tokens: accounting.output_tokens,
            cache_read_tokens: accounting.cache_read_tokens,
            cache_write_tokens: accounting.cache_write_tokens,
            total_tokens: None,
            cost_usd: accounting.cost_usd,
            turn_duration_ms: u64::try_from(started.elapsed().as_millis()).ok(),
            source: Some(
                if accounting.provider_actual {
                    "provider_actual"
                } else {
                    "unavailable"
                }
                .to_owned(),
            ),
            remote_calls: Some(accounting.remote_calls),
            material_count: Some(corpus.material_count),
            corpus_bytes: Some(corpus.corpus_bytes),
            corpus_utf8_chars: Some(corpus.corpus_utf8_chars),
            corpus_est_tokens: Some(corpus.naive_corpus_est_tokens),
            retrieval_candidate_count: None,
            selected_evidence_count: None,
            selected_evidence_bytes: None,
            selected_evidence_utf8_chars: None,
            evidence_est_tokens: None,
            context_reduction_pct: None,
            semantic_provider_state: Some(
                project_knowledge::SemanticProviderState::NotRequested
                    .as_str()
                    .to_owned(),
            ),
            request_preparation_ms: None,
            retrieval_mode: None,
            eligible_materials: None,
            materials_inspected: None,
            chunks_inspected: None,
            exhaustive_coverage: Some(ExhaustiveCoverage::NotRequested.as_str().to_owned()),
            lexical_hits: None,
            semantic_hits: None,
            local_mode: Some(summary_mode.to_owned()),
            contextual_followup: Some(!current_turn_selected_batch && !per_item_batch_aggregate),
            referent_type: Some(followup.referent_type().to_owned()),
            referent_count: Some(referent_count),
            origin_turn_id: (!current_turn_selected_batch && !per_item_batch_aggregate)
                .then(|| followup.origin_turn_id.clone()),
            base_intent: Some(followup.action.base_intent().to_owned()),
            turn_kind: Some(summary_mode.to_owned()),
            source_names: Vec::new(),
        };
        self.persist_completed_turn_metrics(&project_id, durable_turn_id.as_ref(), metrics)?;
        crate::session_log::record(
            "INFO",
            format!(
                "turn terminal conversation_id={} turn_id={} status=completed kind={summary_mode} duration_ms={}",
                project_id,
                turn_id.as_deref().unwrap_or("none"),
                started.elapsed().as_millis()
            ),
        );
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
                    if failure == project_knowledge::SummaryFailure::Cancelled {
                        // Cancellation is operation state, never a failed
                        // summary node. Stop before scheduling descendants.
                        return Err(AppError::new(
                            ErrorCode::AiTaskFailed,
                            "El resumen se canceló.",
                        ));
                    }
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

    /// Explicit, caller-approved continuation of a durable selected-source K6
    /// operation. It never invokes ingestion or embeddings: it plans only from
    /// already-ready Knowledge documents and reuses committed summary nodes.
    pub fn resume_summary_operation_with(
        &self,
        project_id: &str,
        operation_id: &str,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<ProjectSummaryAnswerView> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
        let operation = store
            .reconcile_summary_operation_after_restart(operation_id)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
        let current_model = self
            .selected_model_ref()?
            .as_ref()
            .map(model_identity_string);
        if self.summary_operation_inputs_changed(&store, &operation, current_model.as_deref())? {
            mark_summary_operation_incompatible(&mut store, &operation)?;
            return Err(AppError::new(
                ErrorCode::SummaryIncompatible,
                "Ese resumen ya no es compatible con sus materiales.",
            ));
        }
        match operation.status {
            project_knowledge::SummaryOperationStatus::Completed => {
                drop(store);
                return self.resume_completed_summary_operation(project_id, &operation);
            }
            project_knowledge::SummaryOperationStatus::Cancelled => {
                return Err(AppError::invalid(
                    "Ese resumen fue cancelado y no se reanuda automáticamente.",
                ));
            }
            project_knowledge::SummaryOperationStatus::RetryRequired => {
                return Err(AppError::invalid(
                    "El resultado remoto anterior no se conoce; reintentá ese nodo explícitamente.",
                ));
            }
            project_knowledge::SummaryOperationStatus::Failed => {
                return Err(AppError::invalid(
                    "Ese resumen falló; reintentá de forma explícita.",
                ));
            }
            project_knowledge::SummaryOperationStatus::Stale => {
                return Err(AppError::invalid(
                    "Ese resumen ya no es compatible con sus materiales.",
                ));
            }
            project_knowledge::SummaryOperationStatus::Pending
            | project_knowledge::SummaryOperationStatus::Running => {}
        }
        drop(store);
        let mut runs = self.summary_runs.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = runs.get(project_id) {
            if active.operation_id != operation_id {
                return Err(AppError::invalid(
                    "Ya hay otro resumen en ejecución para esta conversación.",
                ));
            }
        } else {
            runs.insert(
                project_id.to_owned(),
                ActiveSummaryRun {
                    operation_id: operation_id.to_owned(),
                    cancellation: Arc::new(crate::summarize::SummaryCancellation::default()),
                },
            );
        }
        drop(runs);
        crate::session_log::record(
            "INFO",
            format!(
                "[knowledge][summary-operation] operation_id={operation_id} status=running resume=true"
            ),
        );
        let result = self.resume_summary_scope(project_id, &operation, summarizer);
        let mut runs = self.summary_runs.lock().unwrap_or_else(|e| e.into_inner());
        if runs
            .get(project_id)
            .is_some_and(|active| active.operation_id == operation_id)
        {
            runs.remove(project_id);
        }
        if let Ok(answer) = &result {
            match self.commit_completed_summary_artifact(&pid, operation_id, answer) {
                Ok(()) => {}
                Err(error) => return Err(error),
            }
        }
        result
    }

    fn resume_summary_operation_with_backend(
        &self,
        project_id: &str,
        operation_id: &str,
        model: Option<&ModelRef>,
    ) -> AppResult<ProjectSummaryAnswerView> {
        #[cfg(test)]
        if let Some(summarizer) = self
            .test_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return self.resume_summary_operation_with(project_id, operation_id, &*summarizer);
        }
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let control = Arc::new(crate::summarize::SummaryCancellation::default());
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                project_id.to_owned(),
                ActiveSummaryRun {
                    operation_id: operation_id.to_owned(),
                    cancellation: Arc::clone(&control),
                },
            );
        let mut summarizer = crate::summarize::OpenCodeRemoteSummarizer::new(
            Arc::clone(backend),
            self.base.join("opencode-scratch"),
        )
        .with_cancellation(control);
        if let Some(model) = model {
            summarizer = summarizer.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        let result = self.resume_summary_operation_with(project_id, operation_id, &summarizer);
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project_id);
        result
    }

    /// Explicit retry is deliberately separate from resume: an unknown remote
    /// outcome is never resent by restart recovery. The UI/API must make this
    /// an intentional user action.
    pub fn retry_summary_operation_with(
        &self,
        project_id: &str,
        operation_id: &str,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<ProjectSummaryAnswerView> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
        let operation = store
            .summary_operation(operation_id)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese resumen."))?;
        if matches!(
            operation.status,
            project_knowledge::SummaryOperationStatus::Completed
                | project_knowledge::SummaryOperationStatus::Cancelled
                | project_knowledge::SummaryOperationStatus::Stale
        ) {
            return Err(AppError::invalid(
                "Ese resumen no admite un reintento explícito.",
            ));
        }
        let current_model = self
            .selected_model_ref()?
            .as_ref()
            .map(model_identity_string);
        if self.summary_operation_inputs_changed(&store, &operation, current_model.as_deref())? {
            mark_summary_operation_incompatible(&mut store, &operation)?;
            return Err(AppError::new(
                ErrorCode::SummaryIncompatible,
                "Ese resumen ya no es compatible con sus materiales.",
            ));
        }
        store
            .claim_summary_operation_retry(operation_id)
            .map_err(|_| AppError::invalid("Ese resumen no admite un reintento explícito."))?;
        drop(store);
        self.resume_summary_operation_with(project_id, operation_id, summarizer)
    }

    /// Production explicit-retry seam. Distinct from `resume_accepted_import_operation`:
    /// unknown remote outcomes and failed summaries are never continued by ordinary resume.
    pub fn retry_accepted_import_operation(
        &self,
        project_id: &str,
        operation_id: &str,
    ) -> AppResult<AgentRunView> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        let store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::internal("No pudimos recuperar los materiales."))?;
        let operation = store
            .accepted_import_operation(operation_id)
            .map_err(|_| AppError::internal("No pudimos recuperar los materiales."))?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró esa operación."))?;
        let turn_id = operation.turn_id.as_deref().ok_or_else(|| {
            AppError::new(
                ErrorCode::RecoveryNoTurn,
                "El envío se interrumpió antes de confirmarse; volvé a enviarlo.",
            )
        })?;
        let summary_operation = store
            .summary_operation_for_turn(turn_id)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?
            .ok_or_else(|| AppError::new(ErrorCode::NotFound, "No se encontró ese resumen."))?;
        let selected_model = self.selected_model_ref()?;
        let answer = self.retry_summary_operation_with_backend(
            project_id,
            &summary_operation.operation_id,
            selected_model.as_ref(),
        )?;
        let surface = answer
            .summarize_surface_text()
            .unwrap_or_else(|| "No encontramos materiales listos para resumir.".to_owned());
        self.persist_assistant_from_summary_if_missing(&pid, Some(turn_id), &surface)?;
        self.update_accepted_import_agent_state(
            &pid,
            operation_id,
            project_knowledge::AcceptedImportAgentState::Completed,
        );
        self.finish_accepted_import_operation(&pid, operation_id, true);
        Ok(AgentRunView {
            status: "completed".to_owned(),
            turn_id: Some(turn_id.to_owned()),
            registered_creation_ids: Vec::new(),
            message: Some(surface),
        })
    }

    fn retry_summary_operation_with_backend(
        &self,
        project_id: &str,
        operation_id: &str,
        model: Option<&ModelRef>,
    ) -> AppResult<ProjectSummaryAnswerView> {
        #[cfg(test)]
        if let Some(summarizer) = self
            .test_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return self.retry_summary_operation_with(project_id, operation_id, &*summarizer);
        }
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let control = Arc::new(crate::summarize::SummaryCancellation::default());
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                project_id.to_owned(),
                ActiveSummaryRun {
                    operation_id: operation_id.to_owned(),
                    cancellation: Arc::clone(&control),
                },
            );
        let mut summarizer = crate::summarize::OpenCodeRemoteSummarizer::new(
            Arc::clone(backend),
            self.base.join("opencode-scratch"),
        )
        .with_cancellation(control);
        if let Some(model) = model {
            summarizer = summarizer.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        let result = self.retry_summary_operation_with(project_id, operation_id, &summarizer);
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project_id);
        result
    }

    fn summary_operation_inputs_changed(
        &self,
        store: &KnowledgeStore,
        operation: &project_knowledge::SummaryOperation,
        current_model: Option<&str>,
    ) -> AppResult<bool> {
        // Fingerprint and model checks use the SAME current identity. Stored
        // `None` is never equivalent to an arbitrary selected model.
        let current_fingerprint =
            current_summary_operation_fingerprint(store, operation, current_model)?;
        Ok(current_fingerprint != operation.compatibility_fingerprint
            || !summary_models_compatible(operation.model_identity.as_deref(), current_model))
    }

    fn resume_completed_summary_operation(
        &self,
        project_id: &str,
        operation: &project_knowledge::SummaryOperation,
    ) -> AppResult<ProjectSummaryAnswerView> {
        let pid = parse_project_id(project_id)?;
        let root = self.base.join("projects").join(pid.as_str());
        let mut store = KnowledgeStore::open(&root, &pid)
            .map_err(|_| AppError::internal("No pudimos recuperar el resumen."))?;
        let surface = self.completed_summary_surface(&mut store, operation)?;
        let naive_corpus_est_tokens = store
            .corpus_stats()
            .map(|stats| stats.naive_corpus_est_tokens)
            .unwrap_or(0);
        Ok(ProjectSummaryAnswerView {
            report: SummarizationReportView {
                remote_calls: 0,
                estimated_input_units: 0,
                cache_hits: 0,
                reused: 0,
                regenerated: 0,
                source_count: operation.selected_ids.len(),
                hierarchy_depth: 0,
                input_tokens: None,
                output_tokens: None,
                cache_read_tokens: None,
                cache_write_tokens: None,
                cost_usd: None,
                provider_usage_actual: false,
            },
            global_summary: Some(surface),
            naive_corpus_est_tokens,
        })
    }

    fn completed_summary_surface(
        &self,
        store: &mut KnowledgeStore,
        operation: &project_knowledge::SummaryOperation,
    ) -> AppResult<String> {
        let Some(final_summary_id) = operation.final_summary_id.as_deref() else {
            let _ = store.mark_summary_operation_artifact_corrupt(
                &operation.operation_id,
                "durable_artifact_missing",
            );
            return Err(AppError::new(
                ErrorCode::SummaryArtifactCorrupt,
                "El resumen completado no tiene un resultado recuperable.",
            ));
        };
        let readable = store.get_summary(final_summary_id).ok().flatten();
        match readable.and_then(|node| node.content.map(|content| content.summary)) {
            Some(surface) => Ok(surface),
            None => {
                let _ = store.mark_summary_operation_artifact_corrupt(
                    &operation.operation_id,
                    "durable_artifact_missing",
                );
                Err(AppError::new(
                    ErrorCode::SummaryArtifactCorrupt,
                    "El resultado final del resumen no existe.",
                ))
            }
        }
    }

    fn commit_completed_summary_artifact(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        answer: &ProjectSummaryAnswerView,
    ) -> AppResult<()> {
        let surface = answer.summarize_surface_text().unwrap_or_default();
        let model = self.selected_model_ref().ok().flatten();
        match self.commit_final_summary_artifact(
            project_id,
            operation_id,
            &surface,
            model.as_ref(),
            &answer.report,
        ) {
            Ok(SummaryCommitOutcome::Completed) => Ok(()),
            Ok(SummaryCommitOutcome::Cancelled) => Err(AppError::invalid("El resumen se canceló.")),
            Err(error) => Err(error),
        }
    }

    /// Resume according to the durable operation contract, never the current
    /// composer wording or an empty selected-id set.
    fn resume_summary_scope(
        &self,
        project_id: &str,
        operation: &project_knowledge::SummaryOperation,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
    ) -> AppResult<ProjectSummaryAnswerView> {
        match operation.scope.as_str() {
            "selected_sources" => {
                self.summarize_selected_sources(project_id, &operation.selected_ids, summarizer)
            }
            "project" => self.summarize_project_with(project_id, summarizer),
            _ => Err(AppError::invalid(
                "El alcance persistido del resumen no es compatible.",
            )),
        }
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
        // Resolve the semantic intent and its deterministic bindings exactly
        // once, then prepare the Knowledge context from that normalized route.
        let model = inputs.model.clone();
        let route = self.resolve_route(
            &inputs.project_id,
            prompt,
            &inputs.selected_material_ids,
            model.as_ref(),
        );
        self.apply_route(&mut inputs, &route)?;
        inputs.route = Some(route);
        Ok(inputs)
    }

    /// Canonical resolution step: builds the structural routing context and
    /// resolves the normalized [`crate::intent::BoundRoute`] exactly once. The
    /// deterministic follow-up/creation pre-gates run inside `resolve_intent`;
    /// semantic classification is delegated to the injected classifier (the
    /// OpenCode semantic classifier in production, with deterministic fallback).
    fn resolve_route(
        &self,
        project_id: &ProjectId,
        prompt: &str,
        selected_material_ids: &[String],
        model: Option<&ModelRef>,
    ) -> crate::intent::BoundRoute {
        let prior = self.prior_referents(project_id);
        let context = self.knowledge_routing_context(project_id, selected_material_ids, &prior);
        let route = match self.semantic_classifier(model) {
            Some(classifier) => {
                crate::intent::resolve_intent_with(prompt, &context, &prior, classifier.as_ref())
            }
            None => crate::intent::resolve_intent(prompt, &context, &prior),
        };
        crate::intent::routing_telemetry(project_id.as_str(), &route, &context);
        route
    }

    /// Builds the semantic classifier for this turn: the test-injected fake when
    /// present, otherwise the OpenCode classifier when a backend is available,
    /// otherwise `None` (deterministic routing). The classifier is created per
    /// turn and is never stored, so it remains stateless.
    fn semantic_classifier(
        &self,
        model: Option<&ModelRef>,
    ) -> Option<Box<dyn crate::classifier::IntentClassifier>> {
        if let Some(classifier) = self
            .test_classifier
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Some(Box::new(TestClassifierAdapter(classifier)));
        }
        let backend = self.classifier_backend.as_ref()?;
        let mut opencode = crate::classifier::opencode::OpenCodeIntentClassifier::new(
            Arc::clone(backend),
            self.base.join("opencode-scratch"),
        );
        if let Some(model) = model {
            opencode = opencode.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        Some(Box::new(crate::classifier::SemanticIntentClassifier::new(
            opencode,
        )))
    }

    /// Deterministic binding + preparation step: consumes the normalized route
    /// and prepares the Knowledge/creation context. No prompt wording is
    /// re-interpreted here; the route's intent and follow-up binding are
    /// authoritative. The follow-up binding stays on the route itself (never
    /// copied into a separate `AgentRunInputs` field).
    fn apply_route(
        &self,
        inputs: &mut AgentRunInputs,
        route: &crate::intent::BoundRoute,
    ) -> AppResult<()> {
        match route.decision.intent {
            // Per-item and K6 summary turns build their own contexts inside
            // their own executors; no Knowledge retrieval is prepared here.
            crate::intent::Intent::BatchSummary
            | crate::intent::Intent::PerItemBatchAggregate
            | crate::intent::Intent::WholeCorpusSummary
            | crate::intent::Intent::PerSourceSummary => {}
            // A creation turn resolves its exact material target here (never in
            // the classifier); the document-wide creation context is prepared
            // deterministically.
            crate::intent::Intent::Creation => {
                if let Some(request) = &route.creation_request
                    && let Some(prepared) = self.prepare_creation_turn(
                        &inputs.project_id,
                        request,
                        &inputs.selected_material_ids,
                    )?
                {
                    inputs.creation = Some(prepared.meta);
                    inputs.knowledge = Some(prepared.knowledge);
                    inputs.knowledge_metrics = Some(prepared.metrics);
                }
            }
            // OrdinaryChat is a hard no-Knowledge turn: strip any leaked
            // context and skip the retrieval seam even when an index exists.
            crate::intent::Intent::OrdinaryChat => {
                inputs.clear_ordinary_chat_knowledge();
                crate::session_log::record(
                    "INFO",
                    format!(
                        "[knowledge] intent=ordinary_chat knowledge_used=false retrieval_mode=none query_embeddings=0 conversation_id={}",
                        inputs.project_id
                    ),
                );
            }
            crate::intent::Intent::NormalSemantic
            | crate::intent::Intent::CorpusExhaustive
            | crate::intent::Intent::CorpusThematic
            | crate::intent::Intent::KnowledgeInventory => {
                let (knowledge, knowledge_metrics, pending_referent) =
                    self.prepare_knowledge_context(&inputs.project_id, &inputs.prompt, route)?;
                inputs.knowledge = knowledge;
                inputs.knowledge_metrics = knowledge_metrics;
                inputs.pending_referent = pending_referent;
            }
        }
        Ok(())
    }

    /// Test-only: inject a fake per-item remote summarizer.
    #[cfg(test)]
    pub fn set_per_item_summarizer(
        &self,
        summarizer: impl project_knowledge::RemoteSummarizer + Send + Sync + 'static,
    ) {
        self.test_per_item_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Arc::new(summarizer));
    }

    /// Test-only: inject a fake remote summarizer for the K6 summary route.
    #[cfg(test)]
    pub fn set_summarizer(
        &self,
        summarizer: impl project_knowledge::RemoteSummarizer + Send + Sync + 'static,
    ) {
        self.test_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Arc::new(summarizer));
    }

    /// Inject a fake intent classifier used instead of the OpenCode classifier.
    /// Production never calls this; integration tests use it to simulate the
    /// semantic multilingual classifier without a live provider.
    pub fn set_test_classifier(
        &self,
        classifier: impl crate::classifier::IntentClassifier + Send + Sync + 'static,
    ) {
        self.test_classifier
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(Arc::new(classifier));
    }

    /// Test-only: force per-item batching/word-limit options.
    #[cfg(test)]
    pub fn set_per_item_options(&self, options: crate::per_item::PerItemExecutionOptions) {
        self.test_per_item_options
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(options);
    }

    /// Builds the per-item remote summarizer: the injected test fake when
    /// present, otherwise the production OpenCode scratch summarizer pinned to
    /// the conversation model.
    fn per_item_summarizer(
        &self,
        model: Option<&ModelRef>,
    ) -> AppResult<Box<dyn project_knowledge::RemoteSummarizer>> {
        #[cfg(test)]
        if let Some(summarizer) = self
            .test_per_item_summarizer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(Box::new(TestSummarizerAdapter(summarizer)));
        }
        let backend = self.summarizer_backend.as_ref().ok_or_else(|| {
            AppError::new(
                ErrorCode::Internal,
                "No pudimos preparar el resumen del material.",
            )
        })?;
        let mut summarizer = crate::summarize::OpenCodeRemoteSummarizer::new(
            Arc::clone(backend),
            self.base.join("opencode-scratch"),
        );
        if let Some(model) = model {
            summarizer = summarizer.with_model(model.provider_id.clone(), model.model_id.clone());
        }
        Ok(Box::new(summarizer))
    }

    /// Test-only: the effective per-item execution options.
    #[cfg(test)]
    fn effective_per_item_options(&self) -> crate::per_item::PerItemExecutionOptions {
        self.test_per_item_options
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .unwrap_or_default()
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
            pending_referent: None,
            creation: None,
            route: None,
        })
    }

    /// Runs the agent without holding the projects mutex. The long agent task
    /// is serialized per project by `AgentService`.
    fn run_agent_with_inputs(
        &self,
        mut inputs: AgentRunInputs,
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

        if inputs
            .route
            .as_ref()
            .is_some_and(|route| route.decision.intent == crate::intent::Intent::OrdinaryChat)
        {
            inputs.clear_ordinary_chat_knowledge();
        }
        let ephemeral = project_agent::knowledge_uses_ephemeral_session(inputs.knowledge.as_ref());
        let conversation_context = if ephemeral {
            self.bounded_visible_conversation(
                &inputs.project_id,
                inputs.turn_id.as_ref().map(|id| id.as_str()),
            )
        } else {
            None
        };
        let current_intent = inputs
            .route
            .as_ref()
            .map(|route| route.decision.intent.as_str())
            .unwrap_or("unbound");
        let previous_turn_kind = self.previous_turn_kind(
            &inputs.project_id,
            inputs.turn_id.as_ref().map(|id| id.as_str()),
        );
        let knowledge_context_present = inputs.knowledge.as_ref().is_some_and(|context| {
            !context.entries.is_empty()
                || context.structural_note.is_some()
                || context.local_answer.is_some()
        });
        let conversation_context_messages = conversation_context
            .as_deref()
            .map(|text| {
                text.lines()
                    .filter(|line| line.starts_with("Usuario:") || line.starts_with("Asistente:"))
                    .count()
            })
            .unwrap_or(0);
        let conversation_context_chars = conversation_context
            .as_deref()
            .map(|text| text.chars().count())
            .unwrap_or(0);
        crate::session_log::record(
            "INFO",
            format!(
                "[lifecycle] conversation_id={} turn_id={} previous_turn_kind={} current_intent={} session_role={} conversation_context_messages={} conversation_context_chars={} knowledge_context_present={} active_material_scope={}",
                inputs.project_id,
                inputs
                    .turn_id
                    .as_ref()
                    .map(|id| id.as_str())
                    .unwrap_or("none"),
                previous_turn_kind,
                current_intent,
                if ephemeral {
                    "ephemeral_knowledge"
                } else {
                    "conversational"
                },
                conversation_context_messages,
                conversation_context_chars,
                knowledge_context_present,
                self.active_material_scope_count(&inputs.project_id),
            ),
        );
        self.agent.run(AgentRequest {
            project_id: inputs.project_id.as_str().to_owned(),
            prompt: AgentPrompt {
                text: inputs.prompt,
                model: inputs.model,
                knowledge: inputs.knowledge,
                conversation_context,
            },
            attachments: inputs.attachments,
        })
    }

    fn bounded_visible_conversation(
        &self,
        project_id: &ProjectId,
        current_user_message_id: Option<&str>,
    ) -> Option<String> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
            .ok()?;
        let messages: Vec<crate::conversation_context::VisibleChatMessage<'_>> = project
            .messages
            .iter()
            .map(|message| crate::conversation_context::VisibleChatMessage {
                id: message.id.as_str(),
                is_user: message.role == MessageRole::User,
                ok: message.status == MessageStatus::Ok,
                text: message.text.as_str(),
            })
            .collect();
        crate::conversation_context::bounded_conversation_context(
            &messages,
            current_user_message_id,
        )
    }

    fn previous_turn_kind(
        &self,
        project_id: &ProjectId,
        current_user_message_id: Option<&str>,
    ) -> &'static str {
        let Ok(project) = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
        else {
            return "none";
        };
        let previous = project.messages.iter().rev().find(|message| {
            message.role == MessageRole::User
                && message.status == MessageStatus::Ok
                && current_user_message_id.is_none_or(|id| message.id.as_str() != id)
        });
        let Some(metrics) = previous.and_then(|message| message.turn_metrics.as_ref()) else {
            return if previous.is_some() {
                "ordinary_chat"
            } else {
                "none"
            };
        };
        if metrics.contextual_followup == Some(true) {
            return "knowledge_followup";
        }
        if let Some(mode) = metrics.local_mode.as_deref() {
            return match mode {
                "inventory" => "inventory",
                "" => "ordinary_chat",
                _ => "local_knowledge",
            };
        }
        match metrics.retrieval_mode.as_deref() {
            Some("normal") => "normal_semantic",
            Some("exhaustive") => "corpus_exhaustive",
            Some("thematic") => "corpus_thematic",
            Some(_) => "knowledge",
            None => "ordinary_chat",
        }
    }

    fn active_material_scope_count(&self, project_id: &ProjectId) -> usize {
        let root = self.base.join("projects").join(project_id.as_str());
        if !root.join("knowledge/knowledge.sqlite").is_file() {
            return 0;
        }
        KnowledgeStore::open(&root, project_id)
            .ok()
            .and_then(|store| store.conversation_active_material_ids().ok())
            .map(|ids| ids.len())
            .unwrap_or(0)
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
        if let Some(run) = self
            .summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(project_id)
        {
            run.cancellation.cancel();
            let pid = parse_project_id(project_id)?;
            let root = self.base.join("projects").join(pid.as_str());
            if let Ok(mut store) = KnowledgeStore::open(&root, &pid) {
                let _ = store.summary_operation_set_active_session(
                    &run.operation_id,
                    run.cancellation.active_session().as_deref(),
                );
                if let Ok(Some(operation)) = store.summary_operation(&run.operation_id) {
                    let _ = store.finish_summary_operation(
                        &run.operation_id,
                        project_knowledge::SummaryOperationStatus::Cancelled,
                        None,
                        None,
                        operation.nodes_generated,
                        operation.nodes_reused,
                        operation.retries,
                        operation.remote_calls,
                    );
                }
            }
            crate::session_log::record(
                "INFO",
                format!(
                    "[knowledge][summary-operation] operation_id={} status=cancelled cancelled=true",
                    run.operation_id
                ),
            );
            return Ok(());
        }
        let role = self.agent.cancel_target_role(project_id).unwrap_or("none");
        crate::session_log::record(
            "INFO",
            format!("[lifecycle] cancel_target_role={role} conversation_id={project_id}"),
        );
        self.agent.cancel(project_id).map_err(AppError::from_agent)
    }

    fn active_summary_operation_id(&self, project_id: &str) -> Option<String> {
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(project_id)
            .map(|run| run.operation_id.clone())
    }

    fn summary_cancelled(&self, project_id: &str) -> bool {
        self.summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(project_id)
            .is_some_and(|run| run.cancellation.is_cancelled())
    }

    fn finish_summary_operation(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        status: project_knowledge::SummaryOperationStatus,
        failure: Option<&str>,
        report: &SummarizationReportView,
        final_summary_id: Option<&str>,
    ) -> AppResult<()> {
        let root = self.base.join("projects").join(project_id.as_str());
        let mut store = KnowledgeStore::open(&root, project_id)
            .map_err(|_| AppError::internal("No pudimos actualizar el resumen."))?;
        // Completion must not erase the accepted explicit-retry count.
        let retries = store
            .summary_operation(operation_id)
            .ok()
            .flatten()
            .map(|operation| operation.retries)
            .unwrap_or(0);
        store
            .finish_summary_operation(
                operation_id,
                status,
                failure,
                final_summary_id,
                report.regenerated,
                report.reused,
                retries,
                report.remote_calls,
            )
            .map_err(|_| AppError::internal("No pudimos actualizar el resumen."))
    }

    fn persist_assistant_from_summary_if_missing(
        &self,
        project_id: &ProjectId,
        turn_id: Option<&str>,
        surface: &str,
    ) -> AppResult<()> {
        let project = self
            .projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .open_project(project_id)
            .map_err(AppError::from_core)?;
        let user_index = turn_id.and_then(|turn_id| {
            project.messages.iter().position(|message| {
                message.id.as_str() == turn_id && message.role == MessageRole::User
            })
        });
        let already_present =
            match user_index {
                Some(index) => project.messages.iter().skip(index + 1).any(|message| {
                    message.role == MessageRole::Assistant && message.text == surface
                }),
                None => project.messages.iter().any(|message| {
                    message.role == MessageRole::Assistant && message.text == surface
                }),
            };
        if already_present {
            return Ok(());
        }
        self.projects
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .append_assistant_message(
                project_id,
                surface,
                MessageStatus::Ok,
                &[],
                turn_id.and_then(|id| MessageId::parse(id).ok()),
            )
            .map_err(AppError::from_core)?;
        Ok(())
    }

    /// Stores the exact locally-rendered K6 result and completes the operation
    /// as one explicit lifecycle outcome. Never silently ignores a lost
    /// terminal transition, never deletes a persisted artifact, and never
    /// leaves the operation in a re-synthesizable state.
    fn commit_final_summary_artifact(
        &self,
        project_id: &ProjectId,
        operation_id: &str,
        surface: &str,
        model: Option<&ModelRef>,
        report: &SummarizationReportView,
    ) -> AppResult<SummaryCommitOutcome> {
        if self.take_fail_next_final_summary_artifact() {
            self.finish_summary_operation(
                project_id,
                operation_id,
                project_knowledge::SummaryOperationStatus::Failed,
                Some("final_artifact_persist_failed"),
                report,
                None,
            )?;
            return Err(AppError::internal(
                "No pudimos guardar el resultado del resumen.",
            ));
        }
        let summary_id = format!(
            "summary-operation-final-{}",
            &sha256_hex(operation_id.as_bytes())[..24]
        );
        let content = project_knowledge::SummaryContent {
            summary: surface.to_owned(),
            topics: Vec::new(),
            decisions: Vec::new(),
            action_items: Vec::new(),
            questions: Vec::new(),
        };
        let node = project_knowledge::SummaryNode {
            summary_id: summary_id.clone(),
            level: project_knowledge::SummaryLevel::Global,
            state: project_knowledge::SummaryState::Ready,
            failure: None,
            content: Some(content.clone()),
            source_ids: Vec::new(),
            source_chunk_ids: Vec::new(),
            parent_summary_id: None,
            input_fingerprint: format!("final-surface:{operation_id}"),
            output_fingerprint: project_knowledge::fingerprint_output(&content),
            generation_id: "operation-final-v1".to_owned(),
            model_id: model.map(|value| value.model_id.clone()),
            provider_id: model.map(|value| value.provider_id.clone()),
            contract_version: project_knowledge::SUMMARY_CONTRACT_VERSION.to_owned(),
            created_at: unix_seconds_now(),
            updated_at: unix_seconds_now(),
        };
        let root = self.base.join("projects").join(project_id.as_str());
        let mut store = KnowledgeStore::open(&root, project_id)
            .map_err(|_| AppError::internal("No pudimos guardar el resultado del resumen."))?;
        let retries = store
            .summary_operation(operation_id)
            .ok()
            .flatten()
            .map(|operation| operation.retries)
            .unwrap_or(0);
        match store.commit_completed_summary_artifact(
            operation_id,
            &node,
            report.regenerated,
            report.reused,
            retries,
            report.remote_calls,
        ) {
            Ok(project_knowledge::SummaryCompletionOutcome::Completed)
            | Ok(project_knowledge::SummaryCompletionOutcome::AlreadyCompleted) => {
                Ok(SummaryCommitOutcome::Completed)
            }
            Ok(project_knowledge::SummaryCompletionOutcome::LostToTerminal(
                project_knowledge::SummaryOperationStatus::Cancelled,
            )) => Ok(SummaryCommitOutcome::Cancelled),
            Ok(project_knowledge::SummaryCompletionOutcome::LostToTerminal(_)) => {
                // Another legal terminal transition (failed/retry_required/
                // stale) already owns the row. The artifact is preserved but
                // unreferenced; fail closed without overwriting that owner.
                Err(AppError::internal(
                    "No pudimos confirmar el resultado del resumen.",
                ))
            }
            Err(_) => {
                // The artifact write itself failed; mark Failed so a later
                // resume cannot fabricate a completed answer, then surface the
                // persistence failure. Re-synthesis here is safe: nothing was
                // persisted.
                self.finish_summary_operation(
                    project_id,
                    operation_id,
                    project_knowledge::SummaryOperationStatus::Failed,
                    Some("final_artifact_persist_failed"),
                    report,
                    None,
                )?;
                Err(AppError::internal(
                    "No pudimos guardar el resultado del resumen.",
                ))
            }
        }
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
                "publication versions={} current_version={} root_mode=history",
                publication.version_count,
                publication.current_version_id.as_deref().unwrap_or("none")
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
            root_url: publication.root_url,
            latest_url: publication.latest_url,
            current_version_id: publication.current_version_id,
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
        // Only rebuild the live URL when this turn produced a new version of an
        // already-shared web lineage. The new version becomes the current
        // publication target; a new DISTINCT activity stays private and must
        // not hijack the existing shared snapshot.
        let target = registered_ids.iter().rev().find(|id| {
            project
                .creations
                .iter()
                .any(|c| c.id.as_str() == id.as_str() && c.kind == CreationKind::Web)
        });
        let Some(target) = target else {
            return Ok(());
        };
        let lineage_shared = project.creations.iter().any(|c| {
            c.id.as_str() == target.as_str()
                && project.creations.iter().any(|other| {
                    other.lineage() == c.lineage() && other.visibility == CreationVisibility::Public
                })
        });
        if !lineage_shared {
            return Ok(());
        }
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
            root_url: None,
            latest_url: None,
            current_version_id: None,
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
                root_url: p.root_url,
                latest_url: p.latest_url,
                current_version_id: p.current_version_id,
            }),
            None => Ok(PublicationView {
                state: "local".to_owned(),
                public_url: None,
                root_url: None,
                latest_url: None,
                current_version_id: None,
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
        let target_lineage = project
            .creations
            .iter()
            .find(|c| c.id == target_id)
            .map(project_core::Creation::lineage)
            .cloned();
        for creation in &project.creations {
            if creation.id == target_id {
                if creation.visibility != CreationVisibility::Public {
                    changes.push((creation.id.clone(), CreationVisibility::Public));
                }
            } else if target_is_web
                && creation.kind == CreationKind::Web
                && creation.visibility == CreationVisibility::Public
                // Historical versions of the SAME lineage stay public: only an
                // independent public web lineage competes for the shared root.
                && target_lineage.as_ref().is_none_or(|l| creation.lineage() != l)
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
                Err(project_knowledge::SummaryFailure::Cancelled) => {
                    return Err(AppError::new(
                        ErrorCode::AiTaskFailed,
                        "El resumen se canceló.",
                    ));
                }
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
                if failure == project_knowledge::SummaryFailure::Cancelled {
                    return Err(AppError::new(
                        ErrorCode::AiTaskFailed,
                        "El resumen se canceló.",
                    ));
                }
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

        let operation_id = self.active_summary_operation_id(store.project_id());
        let execution_control = self
            .summary_runs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(store.project_id())
            .map(|run| Arc::clone(&run.cancellation));
        if self.summary_cancelled(store.project_id()) {
            if let Some(operation_id) = operation_id.as_deref() {
                let _ = store.finish_summary_operation(
                    operation_id,
                    project_knowledge::SummaryOperationStatus::Cancelled,
                    None,
                    None,
                    accounting.regenerated,
                    accounting.reused,
                    0,
                    accounting.remote_calls,
                );
            }
            return Err(project_knowledge::SummaryFailure::Cancelled);
        }
        if let Some(operation_id) = operation_id.as_deref() {
            store
                .summary_operation_begin_node(operation_id, &node.summary_id)
                .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
        }

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
                let (output, content) = self.summarize_and_validate(
                    summarizer,
                    &request,
                    &labels,
                    accounting,
                    execution_control
                        .as_deref()
                        .map(|control| control as &dyn project_knowledge::SummaryExecutionControl),
                )?;
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
                if !self.commit_synthesized_summary(store, operation_id.as_deref(), &ready)? {
                    return Err(project_knowledge::SummaryFailure::Cancelled);
                }
                accounting.regenerated += 1;
                if let Some(operation_id) = operation_id.as_deref() {
                    store
                        .summary_operation_checkpoint(
                            operation_id,
                            accounting.regenerated,
                            accounting.reused,
                            0,
                            accounting.remote_calls,
                        )
                        .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
                }
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
                let (output, content) = self.summarize_and_validate(
                    summarizer,
                    &request,
                    &labels,
                    accounting,
                    execution_control
                        .as_deref()
                        .map(|control| control as &dyn project_knowledge::SummaryExecutionControl),
                )?;
                let mut ready = node.clone();
                ready.content = Some(content.clone());
                ready.output_fingerprint = project_knowledge::fingerprint_output(&content);
                ready.model_id = output.model_id;
                ready.provider_id = output.provider_id;
                ready.generation_id = "generation-1".to_owned();
                ready.state = project_knowledge::SummaryState::Ready;
                if !self.commit_synthesized_summary(store, operation_id.as_deref(), &ready)? {
                    return Err(project_knowledge::SummaryFailure::Cancelled);
                }
                accounting.regenerated += 1;
                if let Some(operation_id) = operation_id.as_deref() {
                    store
                        .summary_operation_checkpoint(
                            operation_id,
                            accounting.regenerated,
                            accounting.reused,
                            0,
                            accounting.remote_calls,
                        )
                        .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
                }
                Ok(())
            }
        }
    }

    fn commit_synthesized_summary(
        &self,
        store: &mut KnowledgeStore,
        operation_id: Option<&str>,
        node: &project_knowledge::SummaryNode,
    ) -> std::result::Result<bool, project_knowledge::SummaryFailure> {
        if let Some(operation_id) = operation_id {
            let allowed = store
                .summary_operation_allows_commit(operation_id, &node.summary_id)
                .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
            if !allowed {
                return Ok(false);
            }
        }
        store
            .store_summary(node)
            .map_err(|_| project_knowledge::SummaryFailure::ExecutionFailed)?;
        Ok(true)
    }

    fn summarize_and_validate(
        &self,
        summarizer: &dyn project_knowledge::RemoteSummarizer,
        request: &project_knowledge::SummaryRequest,
        labels: &[String],
        accounting: &mut project_knowledge::SummaryAccounting,
        execution_control: Option<&dyn project_knowledge::SummaryExecutionControl>,
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
            let output = match execution_control {
                Some(control) => summarizer.summarize_controlled(request, control)?,
                None => summarizer.summarize(request)?,
            };
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

fn summary_operation_id(turn_id: Option<&str>) -> String {
    let stable = turn_id.unwrap_or("standalone");
    format!("summary-{}", &sha256_hex(stable.as_bytes())[..24])
}

/// Lifecycle compatibility is explicit about model identity even though the
/// current K6 node cache remains model-agnostic (`model_id=None`). This avoids
/// falsely treating an interrupted operation as resumable under a different
/// selected provider/model; model-aware node-cache migration is a later task.
fn summary_compatibility_sources(
    store: &KnowledgeStore,
    scope: &str,
    selected_ids: &[String],
) -> AppResult<Vec<String>> {
    if scope == "selected_sources" {
        Ok(selected_ids
            .iter()
            .map(|material_id| {
                let document = store
                    .document_for_material(material_id)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "unavailable".to_owned());
                format!("{material_id}:{document}")
            })
            .collect())
    } else {
        Ok(store
            .summary_document_levels()
            .map_err(|_| AppError::internal("No pudimos preparar el resumen."))?
            .into_iter()
            .map(|(document_id, _)| document_id)
            .collect())
    }
}

fn model_identity_string(model: &ModelRef) -> String {
    format!("{}/{}", model.provider_id, model.model_id)
}

/// Compatibility policy: `None` matches only `None`. A stored missing identity
/// is never treated as compatible with an arbitrary currently selected model.
fn summary_models_compatible(stored: Option<&str>, current: Option<&str>) -> bool {
    match (stored, current) {
        (Some(stored), Some(current)) => stored == current,
        (None, None) => true,
        _ => false,
    }
}

fn current_summary_operation_fingerprint(
    store: &KnowledgeStore,
    operation: &project_knowledge::SummaryOperation,
    model_identity: Option<&str>,
) -> AppResult<String> {
    let sources = summary_compatibility_sources(store, &operation.scope, &operation.selected_ids)?;
    Ok(summary_operation_fingerprint(
        &operation.scope,
        &sources,
        model_identity,
    ))
}

fn mark_summary_operation_incompatible(
    store: &mut KnowledgeStore,
    operation: &project_knowledge::SummaryOperation,
) -> AppResult<()> {
    if matches!(
        operation.status,
        project_knowledge::SummaryOperationStatus::Completed
            | project_knowledge::SummaryOperationStatus::Cancelled
            | project_knowledge::SummaryOperationStatus::Stale
    ) {
        return Ok(());
    }
    store
        .finish_summary_operation(
            &operation.operation_id,
            project_knowledge::SummaryOperationStatus::Stale,
            Some("incompatible_operation"),
            None,
            operation.nodes_generated,
            operation.nodes_reused,
            operation.retries,
            operation.remote_calls,
        )
        .map_err(|_| AppError::internal("No pudimos preparar el resumen."))
}

fn summary_operation_fingerprint(
    scope: &str,
    selected_ids: &[String],
    model: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"summary-operation-v1");
    hasher.update(project_knowledge::SUMMARY_CONTRACT_VERSION.as_bytes());
    hasher.update(scope.as_bytes());
    for id in selected_ids {
        hasher.update(b"::selected:");
        hasher.update(id.as_bytes());
    }
    if let Some(model) = model {
        hasher.update(b"::model:");
        hasher.update(model.as_bytes());
    }
    sha256_hex(&hasher.finalize())
}

/// Unix seconds now (same epoch basis the Knowledge store uses for its ledger).
fn unix_seconds_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().try_into().unwrap_or(i64::MAX))
        .unwrap_or(0)
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
        project_knowledge::SummaryFailure::Cancelled => "cancelled",
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
                project_knowledge::SummaryFailure::Cancelled => "cancelled",
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
    store: &KnowledgeStore,
    operation: project_knowledge::AcceptedImportOperation,
    materials_ready: usize,
) -> AcceptedImportProgressView {
    let now = unix_seconds_now();
    let elapsed_ms = now.saturating_sub(operation.created_at).max(0) as u64 * 1_000;
    // Smoothed running average over the embedding phase, only once a stable
    // sample exists. Never instantaneous, so it does not jump wildly.
    let throughput_embeddings_per_sec =
        if operation.embedding_started_at > 0 && operation.embedding_completed > 0 {
            let phase_elapsed = now.saturating_sub(operation.embedding_started_at);
            if phase_elapsed > 0 {
                Some(operation.embedding_completed as f64 / phase_elapsed as f64)
            } else {
                None
            }
        } else {
            None
        };
    let summary_retryable = operation
        .turn_id
        .as_deref()
        .and_then(|turn_id| store.summary_operation_for_turn(turn_id).ok().flatten())
        .is_some_and(|summary| {
            matches!(
                summary.status,
                project_knowledge::SummaryOperationStatus::RetryRequired
                    | project_knowledge::SummaryOperationStatus::Failed
            ) && summary.failure_class.as_deref() != Some("durable_artifact_missing")
        });
    AcceptedImportProgressView {
        operation_id: operation.operation_id,
        state: accepted_import_state_str(operation.state).to_owned(),
        agent_state: accepted_import_agent_state_str(operation.agent_state).to_owned(),
        summary_retryable,
        total: operation.total,
        copied: operation.copied,
        lexical_completed: operation.lexical_completed,
        embedding_completed: operation.embedding_completed,
        failed: operation.failed,
        embeddings_created: operation.embeddings_created,
        embeddings_reused: operation.embeddings_reused,
        chunks_total: operation.chunks_total,
        embeddings_total: operation.embeddings_total,
        materials_ready,
        elapsed_ms,
        throughput_embeddings_per_sec,
        synthesizing: operation.synthesizing,
    }
}

/// Computes the user-facing `materialsReady` for an accepted operation without
/// ever loading ONNX.
///
/// The active embedding generation is read from the embedded model manifest (a
/// pure JSON parse, no runtime session); the operation's persisted generation
/// id is only a fallback when the manifest cannot be resolved. This guarantees
/// that embeddings from a previous generation never count toward the current
/// operation's readiness.
///
/// When the operation has no resolved embedding context (no embedding phase
/// ever resolved chunks, i.e. no provider / nothing to embed), the explicit
/// degraded rule counts lexically-ready materials. This is consistent with the
/// current frontend `semanticReady` fallback (`failed === 0` when
/// `chunksTotal == 0`): an offline/lexical-only import is not stuck at 0/N.
/// The two units are never mixed in one call.
fn active_import_materials_ready(
    store: &KnowledgeStore,
    operation: &project_knowledge::AcceptedImportOperation,
) -> project_knowledge::Result<usize> {
    let has_embedding_context =
        operation.embedding_generation_id.is_some() || operation.chunks_total > 0;
    let generation_id = if has_embedding_context {
        ModelManifest::embedded()
            .ok()
            .map(|manifest| manifest.active().generation_id.clone())
            .or_else(|| operation.embedding_generation_id.clone())
    } else {
        None
    };
    store.accepted_import_materials_ready(&operation.operation_id, generation_id.as_deref())
}

fn creation_view(c: &Creation, all: &[Creation]) -> CreationView {
    let lineage = c.lineage();
    let mut available_version_ids: Vec<String> = all
        .iter()
        .filter(|other| other.lineage() == lineage)
        .map(|other| other.id.as_str().to_owned())
        .collect();
    available_version_ids.sort();
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
        lineage_id: lineage.as_str().to_owned(),
        version_number: c.version(),
        parent_version_id: c
            .parent_creation_id
            .as_ref()
            .map(|parent| parent.as_str().to_owned()),
        is_current: c.current(),
        available_version_ids,
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
        turn_id: m.turn_id.as_ref().map(|id| id.as_str().to_owned()),
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

/// Truthful local no-selection answer for a per-source summary request that has
/// no bindable material set. Wording is consistent with the existing
/// local-answer UX patterns (local, non-technical, user-facing).
const PER_SOURCE_NO_SELECTION_ANSWER: &str = "No tengo archivos seleccionados para resumir. Adjuntá o seleccioná los archivos que querés resumir y volvé a intentarlo.";

/// Per-file size cap for batch imports (M8 §5). Clipboard images use a stricter
/// cap ([`CLIPBOARD_IMAGE_MAX_BYTES`]).
const MAX_IMPORT_FILE_BYTES: u64 = 100 * 1024 * 1024;

/// Application-side embedding batch size. The store still applies its own
/// safety clamp and the provider re-batches internally, so this only affects
/// throughput, never vector ordering, association, or model semantics.
const EMBEDDING_BATCH_SIZE: usize = 32;

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

/// Fills the durable origin-turn id of a captured referent at persistence time
/// (the owning user message id is only known after the message is appended).
fn with_referent_origin(
    mut referent: project_core::TurnReferent,
    turn_id: &str,
) -> project_core::TurnReferent {
    match &mut referent {
        project_core::TurnReferent::MaterialSet(set) => {
            set.origin_turn_id = turn_id.to_owned();
        }
        project_core::TurnReferent::ThemeSet(set) => {
            set.origin_turn_id = turn_id.to_owned();
        }
    }
    referent
}

/// Bounded evidence assembly for corpus-wide thematic synthesis. The budget is
/// fixed regardless of corpus size, so 15, 50, or 100 documents never forward
/// the raw corpus: only a bounded set of contributing excerpts reaches the
/// single remote synthesis. The per-document caps are deliberately larger than
/// the ordinary K4 defaults (8 entries, 2 per document) so recurring themes
/// can be supported by more than a handful of sources.
fn thematic_assembly_options() -> ContextAssemblyOptions {
    ContextAssemblyOptions {
        max_evidence_budget: 8_000,
        reserve_margin: 400,
        max_entries: 20,
        max_per_document: 2,
        max_per_source: 2,
        max_entry_budget: 1_200,
        min_entry_budget: 64,
        hybrid_candidate_limit: 20,
        neighbor_radius: 0,
    }
}

fn lexical_exhaustive_candidates(
    candidates: &[project_knowledge::HybridSearchResult],
) -> Vec<project_knowledge::HybridSearchResult> {
    candidates
        .iter()
        .filter(|candidate| candidate.signals.lexical_match)
        .cloned()
        .collect()
}

fn citation_names_from_selected_evidence(
    entries: &[AgentKnowledgeEntry],
    exhaustive: bool,
) -> Vec<String> {
    let mut seen = HashSet::new();
    entries
        .iter()
        .filter(|entry| {
            if !exhaustive {
                return true;
            }
            matches!(
                entry.evidence_kind.as_deref(),
                Some("lexical") | Some("both")
            )
        })
        .filter_map(|entry| {
            let name = project_core::safe_file_name(&entry.source_name);
            if name.contains('/') || name.contains('\\') || !seen.insert(name.clone()) {
                None
            } else {
                Some(name)
            }
        })
        .collect()
}

/// Builds a durable MaterialSet only from concrete lexical matches produced by
/// local Knowledge inspection. Semantic-only candidates may be useful to rank
/// a provider prompt but do not prove that a material belongs to a concrete
/// answer result set. The scanner's deterministic candidate order is retained.
fn material_set_from_grounded_candidates(
    candidates: &[project_knowledge::HybridSearchResult],
    produced_by: &str,
) -> Option<project_core::TurnReferent> {
    let mut seen = HashSet::new();
    let mut material_ids = Vec::new();
    let mut source_names = Vec::new();
    for candidate in candidates
        .iter()
        .filter(|candidate| candidate.signals.lexical_match)
    {
        if seen.insert(candidate.source_id.clone()) {
            material_ids.push(candidate.source_id.clone());
            source_names.push(project_core::safe_file_name(&candidate.source_name));
        }
    }
    (!material_ids.is_empty()).then_some(project_core::TurnReferent::MaterialSet(
        project_core::MaterialSetReferent {
            material_ids,
            source_names,
            origin_turn_id: String::new(),
            produced_by: produced_by.to_owned(),
        },
    ))
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

fn grounded_source_names(knowledge: Option<&AgentKnowledgeContext>) -> Vec<String> {
    let Some(knowledge) = knowledge else {
        return Vec::new();
    };
    let exhaustive = knowledge.retrieval_mode.as_deref() == Some("exhaustive");
    let names: Vec<String> = if !knowledge.citation_source_names.is_empty() {
        let mut seen = HashSet::new();
        knowledge
            .citation_source_names
            .iter()
            .map(|name| project_core::safe_file_name(name))
            .filter(|name| !name.contains('/') && !name.contains('\\') && seen.insert(name.clone()))
            .collect()
    } else {
        citation_names_from_selected_evidence(&knowledge.entries, exhaustive)
    };
    if names.is_empty() {
        return Vec::new();
    }
    let mut rows: Vec<String> = names
        .into_iter()
        .map(|name| match safe_filename_date(&name) {
            Some((year, month, day)) => format!("{year:04}-{month:02}-{day:02} … {name}"),
            None => name,
        })
        .collect();
    rows.sort();
    rows
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod source_provenance_tests {
    use super::*;

    fn entry(name: &str, kind: Option<&str>) -> AgentKnowledgeEntry {
        AgentKnowledgeEntry {
            label: "E1".to_owned(),
            source_label: "S1".to_owned(),
            source_name: name.to_owned(),
            chunk_label: "C1".to_owned(),
            line_start: Some(1),
            line_end: Some(2),
            heading_path: Vec::new(),
            text: "excerpt".to_owned(),
            source_id: None,
            evidence_kind: kind.map(str::to_owned),
        }
    }

    fn context(
        mode: &str,
        entries: Vec<AgentKnowledgeEntry>,
        citation_source_names: Vec<String>,
    ) -> AgentKnowledgeContext {
        AgentKnowledgeContext {
            entries,
            evidence_budget_used: 1,
            evidence_budget_limit: 2700,
            indexed_source_names: vec!["inspected.md".to_owned()],
            citation_map: Vec::new(),
            retrieval_mode: Some(mode.to_owned()),
            exhaustive_coverage: Some("complete".to_owned()),
            structural_note: Some("materials_inspected=5".to_owned()),
            local_answer: None,
            authorize_negative: false,
            citation_source_names,
            creation_from_material: false,
        }
    }

    #[test]
    fn exhaustive_fuentes_omit_semantic_only_and_inspection_inventory() {
        let knowledge = context(
            "exhaustive",
            vec![
                entry("hit.md", Some("lexical")),
                entry("near.md", Some("semantic")),
                entry("hit.md", Some("lexical")),
            ],
            Vec::new(),
        );
        let names = grounded_source_names(Some(&knowledge));
        assert_eq!(names, vec!["hit.md".to_owned()]);
    }

    #[test]
    fn exhaustive_explicit_citation_names_win_over_unselected_entries() {
        let knowledge = context(
            "exhaustive",
            vec![entry("dropped.md", Some("lexical"))],
            vec!["kept.md".to_owned()],
        );
        let names = grounded_source_names(Some(&knowledge));
        assert_eq!(names, vec!["kept.md".to_owned()]);
    }

    #[test]
    fn normal_rag_may_cite_semantic_selected_evidence() {
        let knowledge = context(
            "normal",
            vec![entry("semantic-only.md", Some("semantic"))],
            Vec::new(),
        );
        let names = grounded_source_names(Some(&knowledge));
        assert_eq!(names, vec!["semantic-only.md".to_owned()]);
    }

    #[test]
    fn sanitized_names_never_keep_path_separators() {
        let knowledge = context(
            "normal",
            vec![entry("/home/damian/secret.md", Some("lexical"))],
            Vec::new(),
        );
        let names = grounded_source_names(Some(&knowledge));
        assert_eq!(names, vec!["home-damian-secret.md".to_owned()]);
    }
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
    followup: Option<&crate::referent::ContextualFollowUp>,
    creation: Option<&crate::creation::CreationRunMeta>,
    source_names: Vec<String>,
) -> TurnMetrics {
    let (provider, model) = provider_identity(model);
    let creation_turn = creation.is_some();
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
        retrieval_mode: if creation_turn {
            None
        } else {
            knowledge.and_then(|metrics| metrics.retrieval_mode.clone())
        },
        eligible_materials: knowledge.and_then(|metrics| metrics.eligible_materials),
        materials_inspected: knowledge.and_then(|metrics| metrics.materials_inspected),
        chunks_inspected: knowledge.and_then(|metrics| metrics.chunks_inspected),
        exhaustive_coverage: knowledge.and_then(|metrics| metrics.exhaustive_coverage.clone()),
        lexical_hits: knowledge.and_then(|metrics| metrics.lexical_hits),
        semantic_hits: knowledge.and_then(|metrics| metrics.semantic_hits),
        local_mode: if creation_turn {
            Some(crate::creation::CREATION_LOCAL_MODE.to_owned())
        } else {
            knowledge.and_then(|metrics| metrics.local_mode.clone())
        },
        contextual_followup: followup.map(|_| true),
        referent_type: followup.map(|followup| followup.referent_type().to_owned()),
        referent_count: followup.map(|followup| followup.referent_count()),
        origin_turn_id: followup.map(|followup| followup.origin_turn_id.clone()),
        base_intent: followup.map(|followup| followup.action.base_intent().to_owned()),
        turn_kind: if creation_turn {
            Some(crate::creation::CREATION_LOCAL_MODE.to_owned())
        } else {
            followup.map(|followup| followup.action.turn_kind().to_owned())
        },
        source_names,
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
        local_mode: None,
        contextual_followup: None,
        referent_type: None,
        referent_count: None,
        origin_turn_id: None,
        base_intent: None,
        turn_kind: None,
        source_names: Vec::new(),
    }
}

/// Minimal truthful turn metrics for a per-source summary request that produced
/// a local no-selection answer. `remote_calls == 0`, no provider usage, and a
/// bounded local-mode label so the turn never looks like a completed remote
/// summary.
fn per_source_no_selection_turn_metrics(
    model: Option<&ModelRef>,
    duration_ms: u128,
) -> TurnMetrics {
    let (provider, model) = provider_identity(model);
    TurnMetrics {
        provider,
        model,
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        total_tokens: None,
        cost_usd: None,
        turn_duration_ms: u64::try_from(duration_ms).ok(),
        source: Some("local".to_owned()),
        remote_calls: Some(0),
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
        local_mode: Some("per_source_no_selection".to_owned()),
        contextual_followup: None,
        referent_type: None,
        referent_count: None,
        origin_turn_id: None,
        base_intent: None,
        turn_kind: None,
        source_names: Vec::new(),
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
    use std::sync::{Arc, Barrier, Mutex, mpsc};
    use std::thread;
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
    #[derive(Clone)]
    struct LevelRecording {
        levels: Arc<Mutex<Vec<project_knowledge::SummaryLevel>>>,
    }
    impl RemoteSummarizer for LevelRecording {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            self.levels.lock().unwrap().push(request.level);
            let content = SummaryContent {
                summary: "resumed".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            Ok(SummaryOutput {
                text: serde_json::to_string(&content).unwrap(),
                model_id: None,
                provider_id: None,
                usage: SummaryUsage::default(),
            })
        }
    }

    #[derive(Clone)]
    struct Echo {
        calls: Arc<Mutex<usize>>,
        levels: Arc<Mutex<Vec<project_knowledge::SummaryLevel>>>,
    }
    impl RemoteSummarizer for Echo {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            *self.calls.lock().unwrap() += 1;
            self.levels.lock().unwrap().push(request.level);
            let summary = request.evidence_texts.join(" | ");
            let content = SummaryContent {
                summary,
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            Ok(SummaryOutput {
                text: serde_json::to_string(&content).unwrap(),
                model_id: None,
                provider_id: None,
                usage: SummaryUsage::default(),
            })
        }
    }

    #[derive(Clone)]
    struct BlockingControlled {
        started: mpsc::Sender<()>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
        sessions: Arc<Mutex<Vec<String>>>,
        expected_level: project_knowledge::SummaryLevel,
        session_id: String,
    }
    impl RemoteSummarizer for BlockingControlled {
        fn summarize(&self, _request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            unreachable!("K6 must use the controlled execution port")
        }
        fn summarize_controlled(
            &self,
            request: &SummaryRequest,
            control: &dyn project_knowledge::SummaryExecutionControl,
        ) -> Result<SummaryOutput, SummaryFailure> {
            assert_eq!(request.level, self.expected_level);
            self.sessions.lock().unwrap().push(self.session_id.clone());
            self.started.send(()).unwrap();
            while !control.is_cancelled() {
                let _ = self
                    .release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_millis(10));
            }
            self.sessions
                .lock()
                .unwrap()
                .push(format!("abort:{}", self.session_id));
            Err(SummaryFailure::Cancelled)
        }
    }

    #[derive(Clone)]
    struct LateSuccessAfterCancel {
        started: mpsc::Sender<()>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }
    impl RemoteSummarizer for LateSuccessAfterCancel {
        fn summarize(&self, _request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            unreachable!("K6 must use the controlled execution port")
        }
        fn summarize_controlled(
            &self,
            _request: &SummaryRequest,
            _control: &dyn project_knowledge::SummaryExecutionControl,
        ) -> Result<SummaryOutput, SummaryFailure> {
            self.started.send(()).unwrap();
            let _ = self.release.lock().unwrap().recv();
            let content = SummaryContent {
                summary: "late provider result".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            Ok(SummaryOutput {
                text: serde_json::to_string(&content).unwrap(),
                model_id: None,
                provider_id: None,
                usage: SummaryUsage::default(),
            })
        }
    }

    #[test]
    fn case_04_cancel_active_document_session_aborts_exact_session() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("one.md");
        std::fs::write(&path, "document body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime cada archivo por separado",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let ready_store =
            KnowledgeStore::open(tmp.path().join("projects").join(&project.id), &pid).unwrap();
        let ready_material = MaterialId::parse(&accepted.material_ids[0]).unwrap();
        assert_eq!(
            ready_store
                .material_index_status(&ready_material)
                .unwrap()
                .unwrap()
                .state,
            MaterialIndexState::Ready
        );
        drop(ready_store);
        let inputs = accepted.inputs;
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fake = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Document,
            session_id: "doc-session-A".to_owned(),
        };
        let worker_state = Arc::clone(&state);
        let worker_fake = fake.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = result_tx.send(worker_state.send_summary_run_with(
                inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            ));
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled document session never became active");
        state.cancel_agent(&project.id).unwrap();
        release_tx.send(()).unwrap();
        let run = result_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled document session did not terminate after cancellation")
            .unwrap();
        worker.join().unwrap();
        assert_eq!(run.status, "cancelled");
        assert_eq!(
            *fake.sessions.lock().unwrap(),
            vec!["doc-session-A", "abort:doc-session-A"]
        );
        let store =
            KnowledgeStore::open(tmp.path().join("projects").join(&project.id), &pid).unwrap();
        let operation = store
            .summary_operation_for_turn(run.turn_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            operation.status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
    }

    #[test]
    fn case_05_cancel_active_batch_session_aborts_exact_session() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let files = (0..11)
            .map(|i| {
                let p = tmp.path().join(format!("{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &files, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        assert_eq!(accepted.inputs.selected_material_ids.len(), 11);
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        for id in &accepted.material_ids {
            assert_eq!(
                store
                    .material_index_status(&MaterialId::parse(id).unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                MaterialIndexState::Ready
            );
        }
        let levels = store.summary_document_levels().unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &levels,
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        for mut node in plan
            .pending
            .into_iter()
            .filter(|node| node.level == project_knowledge::SummaryLevel::Document)
        {
            let content = SummaryContent {
                summary: "seeded document".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            node.state = project_knowledge::SummaryState::Ready;
            node.content = Some(content.clone());
            node.output_fingerprint = project_knowledge::fingerprint_output(&content);
            store.store_summary(&node).unwrap();
        }
        drop(store);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fake = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Batch,
            session_id: "batch-session-A".into(),
        };
        let (result_tx, result_rx) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker_fake = fake.clone();
        let inputs = accepted.inputs;
        let worker = thread::spawn(move || {
            let _ = result_tx.send(worker_state.send_summary_run_with(
                inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            ));
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled batch session never became active");
        state.cancel_agent(&project.id).unwrap();
        release_tx.send(()).unwrap();
        let run = result_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled batch session did not terminate after cancellation")
            .unwrap();
        worker.join().unwrap();
        assert_eq!(run.status, "cancelled");
        assert_eq!(
            *fake.sessions.lock().unwrap(),
            vec!["batch-session-A", "abort:batch-session-A"]
        );
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store
            .summary_operation_for_turn(run.turn_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
    }

    #[test]
    fn case_06_concurrent_cancel_is_idempotent_and_terminal() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("one.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        assert_eq!(
            KnowledgeStore::open(&root, &pid)
                .unwrap()
                .material_index_status(&MaterialId::parse(&accepted.material_ids[0]).unwrap())
                .unwrap()
                .unwrap()
                .state,
            MaterialIndexState::Ready
        );
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fake = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Document,
            session_id: "cancel-session-A".into(),
        };
        let (result_tx, result_rx) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker_fake = fake.clone();
        let worker = thread::spawn(move || {
            let _ = result_tx.send(worker_state.send_summary_run_with(
                accepted.inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::WholeCorpus,
            ));
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled session never became active");
        let barrier = Arc::new(Barrier::new(3));
        let a = Arc::clone(&state);
        let b = Arc::clone(&state);
        let project_a = project.id.clone();
        let project_b = project.id.clone();
        let ba = Arc::clone(&barrier);
        let bb = Arc::clone(&barrier);
        let c1 = thread::spawn(move || {
            ba.wait();
            a.cancel_agent(&project_a)
        });
        let c2 = thread::spawn(move || {
            bb.wait();
            b.cancel_agent(&project_b)
        });
        barrier.wait();
        c1.join().unwrap().unwrap();
        c2.join().unwrap().unwrap();
        release_tx.send(()).unwrap();
        let run = result_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("controlled session did not terminate after cancellation")
            .unwrap();
        worker.join().unwrap();
        assert_eq!(run.status, "cancelled");
        assert_eq!(
            *fake.sessions.lock().unwrap(),
            vec!["cancel-session-A", "abort:cancel-session-A"]
        );
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store
            .summary_operation_for_turn(run.turn_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
        assert!(
            store
                .summary_operation_begin_node(&op.operation_id, "late")
                .is_err()
        );
        assert!(
            store
                .summary_operation_checkpoint(&op.operation_id, 1, 0, 0, 1)
                .is_err()
        );
        assert!(
            store
                .finish_summary_operation(
                    &op.operation_id,
                    project_knowledge::SummaryOperationStatus::Completed,
                    None,
                    Some("late"),
                    1,
                    0,
                    0,
                    1
                )
                .is_err()
        );
        drop(store);
        let reopened = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(
            reopened
                .summary_operation(&op.operation_id)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
    }

    #[test]
    fn case_01_restart_resumes_missing_document_nodes_only() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..3)
            .map(|i| {
                let p = tmp.path().join(format!("case1-{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        for id in &accepted.material_ids {
            assert_eq!(
                store
                    .material_index_status(&MaterialId::parse(id).unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                MaterialIndexState::Ready
            );
        }
        let selected = accepted.inputs.selected_material_ids.clone();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id = "case-01-partial";
        store
            .create_summary_operation(
                operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let levels = store.summary_document_levels().unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &levels,
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut docs = plan
            .pending
            .into_iter()
            .filter(|node| node.level == project_knowledge::SummaryLevel::Document)
            .collect::<Vec<_>>();
        assert_eq!(docs.len(), 3);
        let mut committed = docs.remove(0);
        let content = SummaryContent {
            summary: "committed document".into(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        committed.state = project_knowledge::SummaryState::Ready;
        committed.content = Some(content.clone());
        committed.output_fingerprint = project_knowledge::fingerprint_output(&content);
        let committed_id = committed.summary_id.clone();
        store.store_summary(&committed).unwrap();
        store
            .summary_operation_begin_node(operation_id, "safe-checkpoint")
            .unwrap();
        store
            .summary_operation_checkpoint(operation_id, 1, 1, 0, 1)
            .unwrap();
        let before = store.summary_operation(operation_id).unwrap().unwrap();
        assert!(before.active_node_id.is_none());
        assert_eq!(before.selected_ids, selected);
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        let summarizer = Multi {
            calls: Arc::clone(&calls),
        };
        let answer = fresh
            .resume_summary_operation_with(&project.id, operation_id, &summarizer)
            .unwrap();
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let after = store.summary_operation(operation_id).unwrap().unwrap();
        assert_eq!(
            after.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(after.scope, "selected_sources");
        assert_eq!(after.selected_ids, selected);
        assert_eq!(after.compatibility_fingerprint, fingerprint);
        assert!(after.final_summary_id.is_some());
        assert_eq!(
            store
                .get_summary(&committed_id)
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            "committed document"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            4,
            "two missing documents plus batch and global only"
        );
        assert_eq!(answer.report.remote_calls, 4);
    }

    #[test]
    fn case_02_restart_reuses_committed_batch_and_continues_downstream() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..11)
            .map(|i| {
                let p = tmp.path().join(format!("case2-{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let selected = accepted.inputs.selected_material_ids.clone();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id = "case-02-partial";
        store
            .create_summary_operation(
                operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut committed_batch = None;
        for mut node in plan.pending {
            if node.level == project_knowledge::SummaryLevel::Document
                || (node.level == project_knowledge::SummaryLevel::Batch
                    && committed_batch.is_none())
            {
                let content = SummaryContent {
                    summary: "committed".into(),
                    topics: vec![],
                    decisions: vec![],
                    action_items: vec![],
                    questions: vec![],
                };
                node.state = project_knowledge::SummaryState::Ready;
                node.content = Some(content.clone());
                node.output_fingerprint = project_knowledge::fingerprint_output(&content);
                if node.level == project_knowledge::SummaryLevel::Batch {
                    committed_batch = Some(node.summary_id.clone());
                }
                store.store_summary(&node).unwrap();
            }
        }
        let batch_id = committed_batch.unwrap();
        store
            .summary_operation_begin_node(operation_id, "safe")
            .unwrap();
        store
            .summary_operation_checkpoint(operation_id, 12, 12, 0, 0)
            .unwrap();
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let levels = Arc::new(Mutex::new(Vec::new()));
        let answer = fresh
            .resume_summary_operation_with(
                &project.id,
                operation_id,
                &LevelRecording {
                    levels: Arc::clone(&levels),
                },
            )
            .unwrap();
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
        assert_eq!(
            *levels.lock().unwrap(),
            vec![
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Global
            ]
        );
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store.summary_operation(operation_id).unwrap().unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(op.selected_ids, selected);
        assert_eq!(op.compatibility_fingerprint, fingerprint);
        assert!(op.final_summary_id.is_some());
        assert_eq!(
            store
                .get_summary(&batch_id)
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            "committed"
        );
        assert_eq!(answer.report.remote_calls, 4);
    }

    #[test]
    fn case_03_restart_with_unknown_remote_outcome_requires_explicit_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("case3.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let (started_tx, started_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let fake = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Document,
            session_id: "unknown-session-A".into(),
        };
        let worker_state = Arc::clone(&state);
        let worker_fake = fake.clone();
        let inputs = accepted.inputs;
        let _worker = thread::spawn(move || {
            let _ = worker_state.send_summary_run_with(
                inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::WholeCorpus,
            );
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("ambiguous remote call never started");
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let turn = state.open_project(&project.id).unwrap().messages[0]
            .id
            .clone();
        let before = store.summary_operation_for_turn(&turn).unwrap().unwrap();
        assert_eq!(
            before.status,
            project_knowledge::SummaryOperationStatus::Running
        );
        let active = before.active_node_id.clone().unwrap();
        assert!(store.get_summary(&active).unwrap().is_none());
        drop(store);
        let fresh = app(tmp.path());
        let mut reconcile = KnowledgeStore::open(&root, &pid).unwrap();
        let after = reconcile
            .reconcile_summary_operation_after_restart(&before.operation_id)
            .unwrap();
        assert_eq!(
            after.status,
            project_knowledge::SummaryOperationStatus::RetryRequired
        );
        assert_eq!(
            after.failure_class.as_deref(),
            Some("remote_outcome_unknown")
        );
        assert!(after.final_summary_id.is_none());
        drop(reconcile);
        let calls_before = fake.sessions.lock().unwrap().len();
        assert_eq!(calls_before, 1);
        let resume = fresh.resume_summary_operation_with(
            &project.id,
            &before.operation_id,
            &Multi {
                calls: Arc::new(Mutex::new(0)),
            },
        );
        assert!(resume.is_err());
        assert_eq!(fake.sessions.lock().unwrap().len(), 1);
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
    }

    #[test]
    fn case_07_explicit_retry_executes_once_and_completes_retry_required_operation() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let paths = (0..2)
            .map(|index| {
                let path = tmp.path().join(format!("case7-{index}.md"));
                std::fs::write(&path, format!("body {index}")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime estos archivos", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let selected = accepted.inputs.selected_material_ids.clone();
        let turn = accepted.inputs.turn_id.as_ref().unwrap().to_string();
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let initial_plan = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut committed = initial_plan
            .pending
            .into_iter()
            .find(|node| node.level == project_knowledge::SummaryLevel::Document)
            .expect("the real plan has a first document node");
        let committed_content = SummaryContent {
            summary: "committed before unknown outcome".into(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        committed.state = project_knowledge::SummaryState::Ready;
        committed.content = Some(committed_content.clone());
        committed.output_fingerprint = project_knowledge::fingerprint_output(&committed_content);
        let committed_id = committed.summary_id.clone();
        store.store_summary(&committed).unwrap();
        drop(store);
        let (started_tx, started_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let ambiguous = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Document,
            session_id: "retry-unknown-session".into(),
        };
        let worker_state = Arc::clone(&state);
        let worker_fake = ambiguous.clone();
        let _worker = thread::spawn(move || {
            let _ = worker_state.send_summary_run_with(
                accepted.inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::WholeCorpus,
            );
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("real planner node did not start");
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let running = store.summary_operation_for_turn(&turn).unwrap().unwrap();
        let opid = running.operation_id.clone();
        let active = running.active_node_id.clone().unwrap();
        let planner = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        assert!(planner.pending.iter().any(|node| node.summary_id == active));
        assert_ne!(active, committed_id);
        assert!(store.get_summary(&active).unwrap().is_none());
        let fp = running.compatibility_fingerprint.clone();
        let op = store
            .reconcile_summary_operation_after_restart(&opid)
            .unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::RetryRequired
        );
        assert_eq!(op.failure_class.as_deref(), Some("remote_outcome_unknown"));
        drop(store);
        let fresh_state = Arc::new(app(tmp.path()));
        let ordinary_calls = Arc::new(Mutex::new(0));
        assert!(
            fresh_state
                .resume_summary_operation_with(
                    &project.id,
                    &opid,
                    &Multi {
                        calls: Arc::clone(&ordinary_calls)
                    }
                )
                .is_err()
        );
        assert_eq!(
            *ordinary_calls.lock().unwrap(),
            0,
            "ordinary resume must not resend a retry-required operation"
        );
        let calls = Arc::new(Mutex::new(0));
        let summary = Multi {
            calls: Arc::clone(&calls),
        };
        let barrier = Arc::new(Barrier::new(3));
        let a = Arc::clone(&fresh_state);
        let b = Arc::clone(&fresh_state);
        let ba = Arc::clone(&barrier);
        let bb = Arc::clone(&barrier);
        let project_a = project.id.clone();
        let project_b = project.id.clone();
        let opid_a = opid.clone();
        let opid_b = opid.clone();
        let sa = summary.clone();
        let sb = summary.clone();
        let one = thread::spawn(move || {
            ba.wait();
            a.retry_summary_operation_with(&project_a, &opid_a, &sa)
        });
        let two = thread::spawn(move || {
            bb.wait();
            b.retry_summary_operation_with(&project_b, &opid_b, &sb)
        });
        barrier.wait();
        let r1 = one.join().unwrap();
        let r2 = two.join().unwrap();
        assert!(r1.is_ok() ^ r2.is_ok());
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let done = store.summary_operation(&opid).unwrap().unwrap();
        assert_eq!(
            done.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(done.retries, 1);
        assert_eq!(done.selected_ids, selected);
        assert_eq!(done.compatibility_fingerprint, fp);
        assert!(done.final_summary_id.is_some());
        assert!(done.active_node_id.is_none());
        assert!(done.active_session_id.is_none());
        assert_eq!(
            store.get_summary(&committed_id).unwrap().unwrap().content,
            Some(committed_content),
            "retry must retain the already committed node byte-for-byte"
        );
        assert!(
            store.get_summary(&active).unwrap().is_some(),
            "the exact real planner node left ambiguous must be synthesized by retry"
        );
        assert_eq!(
            *calls.lock().unwrap(),
            3,
            "only the real ambiguous document plus its batch and global descendants run"
        );
        assert_eq!(done.remote_calls, 3);
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let post_completion_resume_calls = Arc::new(Mutex::new(0));
        assert!(
            fresh
                .resume_summary_operation_with(
                    &project.id,
                    &opid,
                    &Multi {
                        calls: Arc::clone(&post_completion_resume_calls)
                    }
                )
                .is_ok()
        );
        assert_eq!(*post_completion_resume_calls.lock().unwrap(), 0);
        let post_completion_retry_calls = Arc::new(Mutex::new(0));
        assert!(
            fresh
                .retry_summary_operation_with(
                    &project.id,
                    &opid,
                    &Multi {
                        calls: Arc::clone(&post_completion_retry_calls)
                    }
                )
                .is_err()
        );
        assert_eq!(*post_completion_retry_calls.lock().unwrap(), 0);
        let reopened = KnowledgeStore::open(&root, &pid).unwrap();
        let reopened_operation = reopened.summary_operation(&opid).unwrap().unwrap();
        assert_eq!(
            reopened_operation.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(reopened_operation.retries, 1);
        assert!(reopened_operation.final_summary_id.is_some());
        assert!(reopened_operation.active_node_id.is_none());
        assert!(reopened_operation.active_session_id.is_none());
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
    }

    #[test]
    fn case_08_completed_operation_restart_is_provider_free_and_reuses_final_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..3)
            .map(|index| {
                let path = tmp.path().join(format!("case8-{index}.md"));
                std::fs::write(&path, format!("body {index}")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let turn_id = accepted.turn_id().unwrap().to_owned();
        let calls = Arc::new(Mutex::new(0));
        let run = state
            .send_summary_run_with(
                accepted.inputs,
                &Multi {
                    calls: Arc::clone(&calls),
                },
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        let provider_calls = *calls.lock().unwrap();
        assert!(provider_calls > 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store.summary_operation_for_turn(&turn_id).unwrap().unwrap();
        assert_eq!(
            operation.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        let final_summary_id = operation.final_summary_id.clone().unwrap();
        assert!(operation.active_node_id.is_none());
        assert!(operation.active_session_id.is_none());
        let existing = store.summary_existing().unwrap();
        assert!(
            existing
                .values()
                .all(|(summary_state, _)| *summary_state == project_knowledge::SummaryState::Ready)
        );
        let artifact = store.get_summary(&final_summary_id).unwrap().unwrap();
        assert_eq!(artifact.state, project_knowledge::SummaryState::Ready);
        let expected_surface = artifact.content.as_ref().unwrap().summary.clone();
        let node_ids = store.summary_ids().unwrap();
        drop(store);
        drop(state);

        let fresh = app(tmp.path());
        let resume_calls = Arc::new(Mutex::new(0));
        let resumed = fresh
            .resume_summary_operation_with(
                &project.id,
                &operation.operation_id,
                &Multi {
                    calls: Arc::clone(&resume_calls),
                },
            )
            .unwrap();
        assert_eq!(*resume_calls.lock().unwrap(), 0);
        assert_eq!(resumed.report.remote_calls, 0);
        let retry_calls = Arc::new(Mutex::new(0));
        assert!(
            fresh
                .retry_summary_operation_with(
                    &project.id,
                    &operation.operation_id,
                    &Multi {
                        calls: Arc::clone(&retry_calls)
                    }
                )
                .is_err()
        );
        assert_eq!(*retry_calls.lock().unwrap(), 0);
        let activity = fresh.test_activity();
        assert_eq!(activity.embedding_inference, 0);
        assert_eq!(activity.indexing, 0);
        assert_eq!(activity.retrieval, 0);
        assert_eq!(activity.raw_attachment_forwarding, 0);
        assert_eq!(activity.provider_calls, 0);
        let reopened = KnowledgeStore::open(&root, &pid).unwrap();
        let after = reopened
            .summary_operation(&operation.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            after.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(
            after.final_summary_id.as_deref(),
            Some(final_summary_id.as_str())
        );
        assert_eq!(
            reopened
                .get_summary(&final_summary_id)
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            expected_surface
        );
        let mut after_ids = reopened.summary_ids().unwrap();
        let mut before_ids = node_ids;
        before_ids.sort();
        after_ids.sort();
        assert_eq!(
            after_ids, before_ids,
            "restart must not create summary nodes"
        );
        assert_eq!(after.remote_calls, provider_calls);
    }

    #[test]
    fn case_09_incompatible_operation_becomes_stale_without_invalidating_unrelated_knowledge() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        let historical = tmp.path().join("historical.md");
        std::fs::write(&historical, "unrelated historical body UNIQUE-HIST").unwrap();
        let historical_turn = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[historical.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.index_accepted_material_batch(
            &historical_turn.inputs.project_id,
            &historical_turn.material_ids,
            Some(&historical_turn.operation_id),
            false,
        );
        let historical_turn_id = historical_turn.turn_id().unwrap().to_owned();
        let historical_calls = Arc::new(Mutex::new(0));
        assert_eq!(
            state
                .send_summary_run_with(
                    historical_turn.inputs,
                    &Multi {
                        calls: Arc::clone(&historical_calls)
                    },
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
                .unwrap()
                .status,
            "completed"
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let historical_op = store
            .summary_operation_for_turn(&historical_turn_id)
            .unwrap()
            .unwrap();
        let historical_opid = historical_op.operation_id.clone();
        let historical_nodes = store
            .summary_ids()
            .unwrap()
            .into_iter()
            .map(|id| {
                let node = store.get_summary(&id).unwrap().unwrap();
                (id, node.state, node.content.map(|content| content.summary))
            })
            .collect::<Vec<_>>();
        drop(store);

        let paths = (0..2)
            .map(|index| {
                let path = tmp.path().join(format!("case9-{index}.md"));
                std::fs::write(&path, format!("selected body {index} UNIQUE-SEL-{index}")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let selected = accepted.inputs.selected_material_ids.clone();
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id = "case-09-resumable";
        store
            .create_summary_operation(
                operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let keep_document = store.document_for_material(&selected[0]).unwrap().unwrap();
        let mut docs = plan
            .pending
            .into_iter()
            .filter(|node| {
                node.level == project_knowledge::SummaryLevel::Document
                    && node.source_ids.first() == Some(&keep_document)
            })
            .collect::<Vec<_>>();
        let mut committed = docs.remove(0);
        let committed_content = SummaryContent {
            summary: "committed selected document".into(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        committed.state = project_knowledge::SummaryState::Ready;
        committed.content = Some(committed_content.clone());
        committed.output_fingerprint = project_knowledge::fingerprint_output(&committed_content);
        let committed_id = committed.summary_id.clone();
        store.store_summary(&committed).unwrap();
        let revised_material = MaterialId::parse(&selected[1]).unwrap();
        store
            .index(
                &project_knowledge::MaterialSource {
                    material_id: revised_material,
                    source_name: "case9-1.md".into(),
                    relative_path: format!("inputs/{}/case9-1.md", selected[1]),
                    media_type: None,
                },
                b"revised selected body UNIQUE-SEL-1-NEW",
            )
            .unwrap();
        drop(store);
        drop(state);

        let fresh = app(tmp.path());
        let resume_calls = Arc::new(Mutex::new(0));
        let resume = fresh.resume_summary_operation_with(
            &project.id,
            operation_id,
            &Multi {
                calls: Arc::clone(&resume_calls),
            },
        );
        assert!(resume.is_err());
        assert_eq!(*resume_calls.lock().unwrap(), 0);
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().indexing, 0);
        assert_eq!(fresh.test_activity().retrieval, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let stale = store.summary_operation(operation_id).unwrap().unwrap();
        assert_eq!(
            stale.status,
            project_knowledge::SummaryOperationStatus::Stale
        );
        assert_eq!(
            stale.failure_class.as_deref(),
            Some("incompatible_operation")
        );
        assert_eq!(
            store
                .summary_operation(&historical_opid)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        for (id, state, content) in &historical_nodes {
            let node = store.get_summary(id).unwrap().unwrap();
            assert_eq!(&node.state, state);
            assert_eq!(node.content.map(|content| content.summary), *content);
        }
        assert_eq!(
            store.get_summary(&committed_id).unwrap().unwrap().content,
            Some(committed_content)
        );
        drop(store);

        let replacement = tmp.path().join("case9-replacement.md");
        std::fs::write(&replacement, "new compatible body UNIQUE-NEW").unwrap();
        let next = fresh
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[replacement.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        fresh.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        fresh.index_accepted_material_batch(
            &next.inputs.project_id,
            &next.material_ids,
            Some(&next.operation_id),
            false,
        );
        let next_calls = Arc::new(Mutex::new(0));
        assert_eq!(
            fresh
                .send_summary_run_with(
                    next.inputs,
                    &Multi {
                        calls: Arc::clone(&next_calls)
                    },
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
                .unwrap()
                .status,
            "completed"
        );
        assert!(*next_calls.lock().unwrap() > 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(
            store
                .summary_operation(&historical_opid)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(
            store
                .summary_operation(operation_id)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Stale
        );
    }

    #[test]
    fn case_10_completed_k6_turn_recovers_from_final_summary_without_provider_or_embedding() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("case10.md");
        std::fs::write(&path, "selected source UNIQUE-CASE10").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime cada archivo por separado",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let import_operation = accepted.operation_id.clone();
        let turn_id = accepted.turn_id().unwrap().to_owned();
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        state.interrupt_after_summary_completion();
        let calls = Arc::new(Mutex::new(0));
        let interrupted = state
            .send_summary_run_with(
                accepted.inputs,
                &Multi {
                    calls: Arc::clone(&calls),
                },
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            )
            .unwrap();
        assert_eq!(interrupted.status, "completed");
        assert!(interrupted.message.is_none());
        let provider_calls = *calls.lock().unwrap();
        assert!(provider_calls > 0);
        let before = state.open_project(&project.id).unwrap();
        assert_eq!(
            before
                .messages
                .iter()
                .filter(|message| message.role == "assistant")
                .count(),
            0
        );
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store.summary_operation_for_turn(&turn_id).unwrap().unwrap();
        assert_eq!(
            operation.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        let final_summary_id = operation.final_summary_id.clone().unwrap();
        let artifact = store
            .get_summary(&final_summary_id)
            .unwrap()
            .unwrap()
            .content
            .unwrap()
            .summary;
        let node_count = store.summary_ids().unwrap().len();
        drop(store);
        drop(state);

        let fresh = app(tmp.path());
        let recovered = fresh
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(recovered.message.as_deref(), Some(artifact.as_str()));
        assert!(recovered.message.as_deref().unwrap().contains("case10.md"));
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().indexing, 0);
        assert_eq!(fresh.test_activity().retrieval, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
        assert_eq!(fresh.test_activity().provider_calls, 0);
        assert_eq!(fresh.test_activity().k6_calls, 0);
        let after = fresh.open_project(&project.id).unwrap();
        let assistants: Vec<_> = after
            .messages
            .iter()
            .filter(|message| message.role == "assistant")
            .collect();
        assert_eq!(assistants.len(), 1);
        assert_eq!(assistants[0].text, artifact);
        let again = fresh
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(again.status, "completed");
        assert_eq!(again.message.as_deref(), Some(artifact.as_str()));
        assert_eq!(
            fresh
                .open_project(&project.id)
                .unwrap()
                .messages
                .iter()
                .filter(|message| message.role == "assistant")
                .count(),
            1
        );
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(store.summary_ids().unwrap().len(), node_count);
        assert_eq!(
            store
                .summary_operation(&operation.operation_id)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        let resume_calls = Arc::new(Mutex::new(0));
        assert_eq!(
            fresh
                .resume_summary_operation_with(
                    &project.id,
                    &operation.operation_id,
                    &Multi {
                        calls: Arc::clone(&resume_calls)
                    }
                )
                .unwrap()
                .report
                .remote_calls,
            0
        );
        assert_eq!(*resume_calls.lock().unwrap(), 0);
    }

    #[test]
    fn case_12_fifty_file_k6_lifecycle_survives_restart_and_reuses_committed_work() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        let historical = tmp.path().join("historical-old.md");
        std::fs::write(&historical, "UNIQUE-HIST-EXCLUDED old material").unwrap();
        state
            .add_material_from_path(&project.id, historical.to_str().unwrap())
            .unwrap();
        let paths = (0..50)
            .map(|index| {
                let path = tmp.path().join(format!("case12-{index:02}.md"));
                std::fs::write(&path, format!("UNIQUE-F{index:02} selected body {index}")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        assert_eq!(accepted.inputs.selected_material_ids.len(), 50);
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let turn_id = accepted.turn_id().unwrap().to_owned();
        let selected = accepted.inputs.selected_material_ids.clone();
        let embeddings_after_index = state.test_activity().embedding_inference;
        assert!(embeddings_after_index > 0);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        for id in &accepted.material_ids {
            assert_eq!(
                store
                    .material_index_status(&MaterialId::parse(id).unwrap())
                    .unwrap()
                    .unwrap()
                    .state,
                MaterialIndexState::Ready
            );
        }
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id = super::summary_operation_id(Some(turn_id.as_str()));
        store
            .create_summary_operation(
                &operation_id,
                Some(&turn_id),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let chunk_counts: std::collections::BTreeMap<String, usize> = store
            .summary_document_levels()
            .unwrap()
            .into_iter()
            .collect();
        let mut seen = std::collections::HashSet::new();
        let levels = selected
            .iter()
            .filter_map(|material_id| store.document_for_material(material_id).ok().flatten())
            .filter(|document_id| seen.insert(document_id.clone()))
            .filter_map(|document_id| {
                chunk_counts
                    .get(&document_id)
                    .copied()
                    .filter(|count| *count > 0)
                    .map(|count| (document_id, count))
            })
            .collect::<Vec<_>>();
        assert_eq!(levels.len(), 50);
        let plan = project_knowledge::plan_project_summaries(
            &levels,
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let total_nodes = plan.pending.len();
        let mut committed_docs = 0usize;
        let mut committed_batch = false;
        let mut committed_ids = Vec::new();
        for mut node in plan.pending {
            let can_commit_doc =
                node.level == project_knowledge::SummaryLevel::Document && committed_docs < 20;
            let can_commit_batch = node.level == project_knowledge::SummaryLevel::Batch
                && !committed_batch
                && node.source_ids.iter().all(|source| {
                    store
                        .get_summary(source)
                        .ok()
                        .flatten()
                        .is_some_and(|existing| {
                            existing.state == project_knowledge::SummaryState::Ready
                        })
                });
            if !can_commit_doc && !can_commit_batch {
                continue;
            }
            let content = SummaryContent {
                summary: format!("committed {}", node.summary_id),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            node.state = project_knowledge::SummaryState::Ready;
            node.content = Some(content.clone());
            node.output_fingerprint = project_knowledge::fingerprint_output(&content);
            committed_ids.push(node.summary_id.clone());
            store.store_summary(&node).unwrap();
            if can_commit_doc {
                committed_docs += 1;
            }
            if can_commit_batch {
                committed_batch = true;
            }
        }
        assert_eq!(committed_docs, 20);
        assert!(committed_batch);
        let remaining = project_knowledge::plan_project_summaries(
            &levels,
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap()
        .pending
        .len();
        assert_eq!(remaining, total_nodes - committed_ids.len());
        store
            .summary_operation_begin_node(&operation_id, "safe")
            .unwrap();
        store
            .summary_operation_checkpoint(
                &operation_id,
                committed_ids.len(),
                committed_ids.len(),
                0,
                0,
            )
            .unwrap();
        drop(store);
        drop(state);

        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        let answer = fresh
            .resume_summary_operation_with(
                &project.id,
                &operation_id,
                &Echo {
                    calls: Arc::clone(&calls),
                    levels: Arc::new(Mutex::new(Vec::new())),
                },
            )
            .unwrap();
        assert_eq!(*calls.lock().unwrap(), remaining);
        assert_eq!(answer.report.remote_calls, remaining);
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().raw_attachment_forwarding, 0);
        assert_eq!(fresh.test_activity().indexing, 0);
        assert_eq!(fresh.test_activity().retrieval, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let done = store.summary_operation(&operation_id).unwrap().unwrap();
        assert_eq!(
            done.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(done.selected_ids, selected);
        assert_eq!(done.compatibility_fingerprint, fingerprint);
        assert!(done.final_summary_id.is_some());
        assert!(done.active_node_id.is_none());
        assert!(done.active_session_id.is_none());
        for id in &committed_ids {
            assert_eq!(
                store
                    .get_summary(id)
                    .unwrap()
                    .unwrap()
                    .content
                    .unwrap()
                    .summary,
                format!("committed {id}")
            );
        }
        let surface = store
            .get_summary(done.final_summary_id.as_deref().unwrap())
            .unwrap()
            .unwrap()
            .content
            .unwrap()
            .summary;
        assert!(
            !surface
                .to_lowercase()
                .contains("no veo ningún archivo adjunto")
        );
        assert!(!surface.contains("UNIQUE-HIST-EXCLUDED"));
        for index in 0..50 {
            let marker = format!("case12-{index:02}.md");
            assert!(
                surface.contains(&marker),
                "selected source missing from provenance: {marker}"
            );
        }
        assert!(!surface.contains("historical-old.md"));
        drop(store);
        drop(fresh);

        let again = app(tmp.path());
        let post_calls = Arc::new(Mutex::new(0));
        let reused = again
            .resume_summary_operation_with(
                &project.id,
                &operation_id,
                &Echo {
                    calls: Arc::clone(&post_calls),
                    levels: Arc::new(Mutex::new(Vec::new())),
                },
            )
            .unwrap();
        assert_eq!(*post_calls.lock().unwrap(), 0);
        assert_eq!(reused.report.remote_calls, 0);
        assert_eq!(again.test_activity().embedding_inference, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let reopened = store.summary_operation(&operation_id).unwrap().unwrap();
        assert_eq!(
            reopened.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(reopened.selected_ids, selected);
        assert_eq!(reopened.compatibility_fingerprint, fingerprint);
        assert_eq!(reopened.retries, 0);
        assert!(
            again
                .retry_summary_operation_with(
                    &project.id,
                    &operation_id,
                    &Echo {
                        calls: Arc::new(Mutex::new(0)),
                        levels: Arc::new(Mutex::new(Vec::new())),
                    }
                )
                .is_err()
        );
    }

    fn mark_started_unknown(root: &std::path::Path, pid: &ProjectId, import_id: &str) {
        let mut store = KnowledgeStore::open(root, pid).unwrap();
        store
            .update_accepted_import_agent_state(
                import_id,
                project_knowledge::AcceptedImportAgentState::StartedOutcomeUnknown,
            )
            .unwrap();
    }

    #[test]
    fn r1_completed_recovery_rejects_stale_artifact_after_selected_document_change() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r1.md");
        std::fs::write(&path, "original UNIQUE-R1").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let import_operation = accepted.operation_id.clone();
        let material_id = accepted.material_ids[0].clone();
        let calls = Arc::new(Mutex::new(0));
        assert_eq!(
            state
                .send_summary_run_with(
                    accepted.inputs,
                    &Multi {
                        calls: Arc::clone(&calls)
                    },
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
                .unwrap()
                .status,
            "completed"
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store
            .summary_operation_for_turn(
                state.open_project(&project.id).unwrap().messages[0]
                    .id
                    .as_str(),
            )
            .unwrap()
            .unwrap();
        let old_artifact = op.final_summary_id.clone().unwrap();
        let old_surface = store
            .get_summary(&old_artifact)
            .unwrap()
            .unwrap()
            .content
            .unwrap()
            .summary;
        drop(store);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        store
            .index(
                &project_knowledge::MaterialSource {
                    material_id: MaterialId::parse(&material_id).unwrap(),
                    source_name: "r1.md".into(),
                    relative_path: format!("inputs/{material_id}/r1.md"),
                    media_type: None,
                },
                b"revised UNIQUE-R1-NEW",
            )
            .unwrap();
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let recovered = fresh.resume_accepted_import_operation(&project.id, &import_operation);
        assert_eq!(
            recovered.unwrap_err().code,
            crate::error::ErrorCode::SummaryIncompatible
        );
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().indexing, 0);
        assert_eq!(fresh.test_activity().k6_calls, 0);
        assert_eq!(fresh.test_activity().provider_calls, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let after = store.summary_operation(&op.operation_id).unwrap().unwrap();
        assert_eq!(
            after.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(
            store
                .get_summary(&old_artifact)
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            old_surface
        );
    }

    #[test]
    fn r2_completed_without_final_summary_id_is_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r2.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let import_operation = accepted.operation_id.clone();
        assert_eq!(
            state
                .send_summary_run_with(
                    accepted.inputs,
                    &Multi {
                        calls: Arc::new(Mutex::new(0))
                    },
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
                .unwrap()
                .status,
            "completed"
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let opid = store
            .summary_operation_for_turn(
                state.open_project(&project.id).unwrap().messages[0]
                    .id
                    .as_str(),
            )
            .unwrap()
            .unwrap()
            .operation_id;
        store
            .execute_sql_for_tests("UPDATE summary_operations SET final_summary_id=NULL")
            .unwrap();
        let node_count = store.summary_ids().unwrap().len();
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        let resume = fresh.resume_summary_operation_with(
            &project.id,
            &opid,
            &Multi {
                calls: Arc::clone(&calls),
            },
        );
        assert_eq!(
            resume.unwrap_err().code,
            crate::error::ErrorCode::SummaryArtifactCorrupt
        );
        assert_eq!(*calls.lock().unwrap(), 0);
        let recovered = fresh.resume_accepted_import_operation(&project.id, &import_operation);
        assert!(recovered.is_err());
        assert_eq!(*calls.lock().unwrap(), 0);
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().k6_calls, 0);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(store.summary_ids().unwrap().len(), node_count);
        assert_eq!(
            store.summary_operation(&opid).unwrap().unwrap().status,
            project_knowledge::SummaryOperationStatus::Failed
        );
    }

    #[test]
    fn r3_completed_with_missing_artifact_row_is_fail_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r3.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let import_operation = accepted.operation_id.clone();
        assert_eq!(
            state
                .send_summary_run_with(
                    accepted.inputs,
                    &Multi {
                        calls: Arc::new(Mutex::new(0))
                    },
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
                .unwrap()
                .status,
            "completed"
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store
            .summary_operation_for_turn(
                state.open_project(&project.id).unwrap().messages[0]
                    .id
                    .as_str(),
            )
            .unwrap()
            .unwrap();
        let final_id = op.final_summary_id.clone().unwrap();
        store
            .execute_sql_for_tests(&format!(
                "DELETE FROM summary_sources WHERE summary_id='{final_id}';
                 DELETE FROM summary_chunks WHERE summary_id='{final_id}';
                 DELETE FROM summaries WHERE summary_id='{final_id}';"
            ))
            .unwrap();
        drop(store);
        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        assert_eq!(
            fresh
                .resume_summary_operation_with(
                    &project.id,
                    &op.operation_id,
                    &Multi {
                        calls: Arc::clone(&calls)
                    }
                )
                .unwrap_err()
                .code,
            crate::error::ErrorCode::SummaryArtifactCorrupt
        );
        assert_eq!(*calls.lock().unwrap(), 0);
        assert!(
            fresh
                .resume_accepted_import_operation(&project.id, &import_operation)
                .is_err()
        );
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().k6_calls, 0);
    }

    #[test]
    fn r4_final_artifact_persist_failure_never_marks_completed() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r4.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        state.fail_next_final_summary_artifact();
        let result = state.send_summary_run_with(
            accepted.inputs,
            &Multi {
                calls: Arc::new(Mutex::new(0)),
            },
            crate::intent::SummaryExecutionKind::WholeCorpus,
        );
        assert!(result.is_err());
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store
            .summary_operation_for_turn(
                state.open_project(&project.id).unwrap().messages[0]
                    .id
                    .as_str(),
            )
            .unwrap()
            .unwrap();
        assert_ne!(
            op.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(op.status, project_knowledge::SummaryOperationStatus::Failed);
        assert_eq!(
            op.failure_class.as_deref(),
            Some("final_artifact_persist_failed")
        );
    }

    #[test]
    fn r8_failed_ordinary_resume_does_not_call_provider() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r8.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let identities = accepted
            .inputs
            .selected_material_ids
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        store
            .create_summary_operation(
                "r8-failed",
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &accepted.inputs.selected_material_ids,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        store
            .finish_summary_operation(
                "r8-failed",
                project_knowledge::SummaryOperationStatus::Failed,
                Some("summary_failure"),
                None,
                0,
                0,
                1,
                0,
            )
            .unwrap();
        drop(store);
        let calls = Arc::new(Mutex::new(0));
        assert!(
            state
                .resume_summary_operation_with(
                    &project.id,
                    "r8-failed",
                    &Multi {
                        calls: Arc::clone(&calls)
                    }
                )
                .is_err()
        );
        assert_eq!(*calls.lock().unwrap(), 0);
        let retry_calls = Arc::new(Mutex::new(0));
        assert!(
            state
                .retry_summary_operation_with(
                    &project.id,
                    "r8-failed",
                    &Multi {
                        calls: Arc::clone(&retry_calls)
                    }
                )
                .is_ok()
        );
        assert!(*retry_calls.lock().unwrap() > 0);
    }

    #[test]
    fn r10_cancel_wins_before_node_commit_discards_late_provider_result() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r10.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let fake = LateSuccessAfterCancel {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
        };
        let worker_state = Arc::clone(&state);
        let (result_tx, result_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = result_tx.send(worker_state.send_summary_run_with(
                accepted.inputs,
                &fake,
                crate::intent::SummaryExecutionKind::WholeCorpus,
            ));
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("late provider never started");
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let turn = state.open_project(&project.id).unwrap().messages[0]
            .id
            .clone();
        let running = store.summary_operation_for_turn(&turn).unwrap().unwrap();
        let active = running.active_node_id.clone().unwrap();
        drop(store);
        state.cancel_agent(&project.id).unwrap();
        release_tx.send(()).unwrap();
        let run = result_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("late provider did not terminate")
            .unwrap();
        worker.join().unwrap();
        assert_eq!(run.status, "cancelled");
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(
            store
                .summary_operation(&running.operation_id)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
        assert!(store.get_summary(&active).unwrap().is_none());
    }

    #[test]
    fn r11_retry_count_is_monotonic_across_cancel_fail_and_complete() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        for (id, finish) in [
            (
                "r11-cancel",
                project_knowledge::SummaryOperationStatus::Cancelled,
            ),
            (
                "r11-failed",
                project_knowledge::SummaryOperationStatus::Failed,
            ),
            (
                "r11-completed",
                project_knowledge::SummaryOperationStatus::Completed,
            ),
        ] {
            store
                .create_summary_operation(
                    id,
                    None,
                    "selected_sources",
                    &[],
                    "fp",
                    Some("opencode/big-pickle"),
                )
                .unwrap();
            store.summary_operation_begin_node(id, "n").unwrap();
            store.summary_operation_checkpoint(id, 0, 0, 7, 0).unwrap();
            store
                .finish_summary_operation(id, finish, None, None, 0, 0, 0, 0)
                .unwrap();
            assert!(
                store.summary_operation(id).unwrap().unwrap().retries >= 7,
                "{id}"
            );
        }
    }

    #[test]
    fn r12_none_model_identity_is_incompatible_with_selected_model() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r12.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let identities = accepted
            .inputs
            .selected_material_ids
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint =
            super::summary_operation_fingerprint("selected_sources", &identities, None);
        store
            .create_summary_operation(
                "r12-none",
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &accepted.inputs.selected_material_ids,
                &fingerprint,
                None,
            )
            .unwrap();
        drop(store);
        let calls = Arc::new(Mutex::new(0));
        let resume = state.resume_summary_operation_with(
            &project.id,
            "r12-none",
            &Multi {
                calls: Arc::clone(&calls),
            },
        );
        assert!(resume.is_err());
        assert_eq!(*calls.lock().unwrap(), 0);
        mark_started_unknown(&root, &pid, &accepted.operation_id);
        drop(state);
        let fresh = app(tmp.path());
        let recovered = fresh.resume_accepted_import_operation(&project.id, &accepted.operation_id);
        assert_eq!(
            recovered.unwrap_err().code,
            crate::error::ErrorCode::SummaryIncompatible
        );
        assert_eq!(fresh.test_activity().k6_calls, 0);
    }

    #[test]
    fn case_01_production_recovery_resumes_missing_nodes_without_reembedding() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..3)
            .map(|i| {
                let p = tmp.path().join(format!("case1p-{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let selected = accepted.inputs.selected_material_ids.clone();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        store
            .create_summary_operation(
                &operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut docs = plan
            .pending
            .into_iter()
            .filter(|node| node.level == project_knowledge::SummaryLevel::Document)
            .collect::<Vec<_>>();
        let mut committed = docs.remove(0);
        let content = SummaryContent {
            summary: "committed document".into(),
            topics: vec![],
            decisions: vec![],
            action_items: vec![],
            questions: vec![],
        };
        committed.state = project_knowledge::SummaryState::Ready;
        committed.content = Some(content.clone());
        committed.output_fingerprint = project_knowledge::fingerprint_output(&content);
        store.store_summary(&committed).unwrap();
        store
            .summary_operation_begin_node(&operation_id, "safe-checkpoint")
            .unwrap();
        store
            .summary_operation_checkpoint(&operation_id, 1, 1, 0, 1)
            .unwrap();
        drop(store);
        mark_started_unknown(&root, &pid, &accepted.operation_id);
        let import_operation = accepted.operation_id.clone();
        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        fresh.set_summarizer(Multi {
            calls: Arc::clone(&calls),
        });
        let recovered = fresh
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(
            *calls.lock().unwrap(),
            4,
            "two missing documents plus batch and global only"
        );
    }

    #[test]
    fn case_02_production_recovery_requests_remaining_batches_and_global() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..11)
            .map(|i| {
                let p = tmp.path().join(format!("case2p-{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let selected = accepted.inputs.selected_material_ids.clone();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        store
            .create_summary_operation(
                &operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let plan = project_knowledge::plan_project_summaries(
            &store.summary_document_levels().unwrap(),
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut committed_batch = None;
        for mut node in plan.pending {
            if node.level == project_knowledge::SummaryLevel::Document
                || (node.level == project_knowledge::SummaryLevel::Batch
                    && committed_batch.is_none())
            {
                let content = SummaryContent {
                    summary: "committed".into(),
                    topics: vec![],
                    decisions: vec![],
                    action_items: vec![],
                    questions: vec![],
                };
                node.state = project_knowledge::SummaryState::Ready;
                node.content = Some(content.clone());
                node.output_fingerprint = project_knowledge::fingerprint_output(&content);
                if node.level == project_knowledge::SummaryLevel::Batch {
                    committed_batch = Some(node.summary_id.clone());
                }
                store.store_summary(&node).unwrap();
            }
        }
        store
            .summary_operation_begin_node(&operation_id, "safe")
            .unwrap();
        store
            .summary_operation_checkpoint(&operation_id, 12, 12, 0, 0)
            .unwrap();
        drop(store);
        mark_started_unknown(&root, &pid, &accepted.operation_id);
        let import_operation = accepted.operation_id.clone();
        drop(state);
        let fresh = app(tmp.path());
        let levels = Arc::new(Mutex::new(Vec::new()));
        fresh.set_summarizer(LevelRecording {
            levels: Arc::clone(&levels),
        });
        let recovered = fresh
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(
            *levels.lock().unwrap(),
            vec![
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Batch,
                project_knowledge::SummaryLevel::Global
            ]
        );
        assert!(committed_batch.is_some());
    }

    #[test]
    fn case_12_production_recovery_excludes_historical_and_second_restart_is_idle() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        let historical = tmp.path().join("historical-old.md");
        std::fs::write(&historical, "UNIQUE-HIST-EXCLUDED old material").unwrap();
        state
            .add_material_from_path(&project.id, historical.to_str().unwrap())
            .unwrap();
        let paths = (0..50)
            .map(|index| {
                let path = tmp.path().join(format!("case12p-{index:02}.md"));
                std::fs::write(&path, format!("UNIQUE-F{index:02} selected body {index}")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let selected = accepted.inputs.selected_material_ids.clone();
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let identities = selected
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        store
            .create_summary_operation(
                &operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &selected,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        let chunk_counts: std::collections::BTreeMap<String, usize> = store
            .summary_document_levels()
            .unwrap()
            .into_iter()
            .collect();
        let mut seen = std::collections::HashSet::new();
        let levels = selected
            .iter()
            .filter_map(|material_id| store.document_for_material(material_id).ok().flatten())
            .filter(|document_id| seen.insert(document_id.clone()))
            .filter_map(|document_id| {
                chunk_counts
                    .get(&document_id)
                    .copied()
                    .filter(|count| *count > 0)
                    .map(|count| (document_id, count))
            })
            .collect::<Vec<_>>();
        let plan = project_knowledge::plan_project_summaries(
            &levels,
            &store.summary_existing().unwrap(),
            project_knowledge::BatchOptions::default(),
            None,
        )
        .unwrap();
        let mut committed_docs = 0usize;
        let mut committed_batch = false;
        let mut committed_ids = Vec::new();
        for mut node in plan.pending {
            let can_commit_doc =
                node.level == project_knowledge::SummaryLevel::Document && committed_docs < 20;
            let can_commit_batch = node.level == project_knowledge::SummaryLevel::Batch
                && !committed_batch
                && node.source_ids.iter().all(|source| {
                    store
                        .get_summary(source)
                        .ok()
                        .flatten()
                        .is_some_and(|existing| {
                            existing.state == project_knowledge::SummaryState::Ready
                        })
                });
            if !can_commit_doc && !can_commit_batch {
                continue;
            }
            let content = SummaryContent {
                summary: format!("committed {}", node.summary_id),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            node.state = project_knowledge::SummaryState::Ready;
            node.content = Some(content.clone());
            node.output_fingerprint = project_knowledge::fingerprint_output(&content);
            committed_ids.push(node.summary_id.clone());
            store.store_summary(&node).unwrap();
            if can_commit_doc {
                committed_docs += 1;
            }
            if can_commit_batch {
                committed_batch = true;
            }
        }
        store
            .summary_operation_begin_node(&operation_id, "safe")
            .unwrap();
        store
            .summary_operation_checkpoint(
                &operation_id,
                committed_ids.len(),
                committed_ids.len(),
                0,
                0,
            )
            .unwrap();
        drop(store);
        mark_started_unknown(&root, &pid, &accepted.operation_id);
        let import_operation = accepted.operation_id.clone();
        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        fresh.set_summarizer(Echo {
            calls: Arc::clone(&calls),
            levels: Arc::new(Mutex::new(Vec::new())),
        });
        let recovered = fresh
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(recovered.status, "completed");
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        let surface = recovered.message.unwrap();
        assert!(!surface.contains("UNIQUE-HIST-EXCLUDED"));
        assert!(!surface.contains("historical-old.md"));
        drop(fresh);
        let again = app(tmp.path());
        let second = again
            .resume_accepted_import_operation(&project.id, &import_operation)
            .unwrap();
        assert_eq!(second.status, "completed");
        assert_eq!(again.test_activity().embedding_inference, 0);
        assert_eq!(again.test_activity().k6_calls, 0);
        assert_eq!(again.test_activity().provider_calls, 0);
    }

    #[test]
    fn r6_r7_production_retry_claims_once() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("r6.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let (started_tx, started_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let fake = BlockingControlled {
            started: started_tx,
            release: Arc::new(Mutex::new(release_rx)),
            sessions: Arc::new(Mutex::new(Vec::new())),
            expected_level: project_knowledge::SummaryLevel::Document,
            session_id: "r6-session".into(),
        };
        let worker_state = Arc::clone(&state);
        let worker_fake = fake.clone();
        let inputs = accepted.inputs;
        let import_operation = accepted.operation_id.clone();
        let _worker = thread::spawn(move || {
            let _ = worker_state.send_summary_run_with(
                inputs,
                &worker_fake,
                crate::intent::SummaryExecutionKind::WholeCorpus,
            );
        });
        started_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("retry-required fence never started");
        let mut store = KnowledgeStore::open(&root, &pid).unwrap();
        let running = store
            .summary_operation_for_turn(
                state.open_project(&project.id).unwrap().messages[0]
                    .id
                    .as_str(),
            )
            .unwrap()
            .unwrap();
        store
            .reconcile_summary_operation_after_restart(&running.operation_id)
            .unwrap();
        drop(store);
        mark_started_unknown(&root, &pid, &import_operation);
        let fresh = Arc::new(app(tmp.path()));
        let ordinary = Arc::new(Mutex::new(0));
        fresh.set_summarizer(Multi {
            calls: Arc::clone(&ordinary),
        });
        assert!(
            fresh
                .resume_accepted_import_operation(&project.id, &import_operation)
                .is_err()
        );
        assert_eq!(*ordinary.lock().unwrap(), 0);
        let calls = Arc::new(Mutex::new(0));
        fresh.set_summarizer(Multi {
            calls: Arc::clone(&calls),
        });
        let barrier = Arc::new(Barrier::new(3));
        let a = Arc::clone(&fresh);
        let b = Arc::clone(&fresh);
        let ba = Arc::clone(&barrier);
        let bb = Arc::clone(&barrier);
        let project_a = project.id.clone();
        let project_b = project.id.clone();
        let op_a = import_operation.clone();
        let op_b = import_operation.clone();
        let one = thread::spawn(move || {
            ba.wait();
            a.retry_accepted_import_operation(&project_a, &op_a)
        });
        let two = thread::spawn(move || {
            bb.wait();
            b.retry_accepted_import_operation(&project_b, &op_b)
        });
        barrier.wait();
        let r1 = one.join().unwrap();
        let r2 = two.join().unwrap();
        assert!(r1.is_ok() ^ r2.is_ok());
        assert!(*calls.lock().unwrap() > 0);
        let winner_calls = *calls.lock().unwrap();
        assert!(
            winner_calls <= 3,
            "exactly one owner should execute the remaining K6 nodes, got {winner_calls}"
        );
    }

    /// Creates the durable summary operation for the accepted turn and marks it
    /// `failed` with a retryable `failure_class`. Ordinary resume must refuse it;
    /// explicit retry must be able to claim it.
    fn mark_summary_failed_for_turn(
        root: &std::path::Path,
        pid: &ProjectId,
        accepted: &AcceptedStagedTurn,
        operation_id: &str,
    ) {
        let mut store = KnowledgeStore::open(root, pid).unwrap();
        let identities = accepted
            .inputs
            .selected_material_ids
            .iter()
            .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
            .collect::<Vec<_>>();
        let fingerprint = super::summary_operation_fingerprint(
            "selected_sources",
            &identities,
            Some("opencode/big-pickle"),
        );
        store
            .create_summary_operation(
                operation_id,
                accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                "selected_sources",
                &accepted.inputs.selected_material_ids,
                &fingerprint,
                Some("opencode/big-pickle"),
            )
            .unwrap();
        store
            .finish_summary_operation(
                operation_id,
                project_knowledge::SummaryOperationStatus::Failed,
                Some("summary_failure"),
                None,
                0,
                0,
                0,
                0,
            )
            .unwrap();
    }

    /// MEDIUM 3.A: a `failed` operation resumed through the real production
    /// recovery seam must refuse (explicit retry required) and perform no
    /// provider, K6, or embedding work, with no state mutation to `completed`.
    #[test]
    fn failed_ordinary_resume_at_product_seam_requires_explicit_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("failed-a.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let embeddings_after_index = state.test_activity().embedding_inference;
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        mark_summary_failed_for_turn(&root, &pid, &accepted, &operation_id);
        let node_ids_before = KnowledgeStore::open(&root, &pid)
            .unwrap()
            .summary_ids()
            .unwrap();
        let retries_before = KnowledgeStore::open(&root, &pid)
            .unwrap()
            .summary_operation(&operation_id)
            .unwrap()
            .unwrap()
            .retries;

        let calls = Arc::new(Mutex::new(0));
        state.set_summarizer(Multi {
            calls: Arc::clone(&calls),
        });
        let resume = state.resume_accepted_import_operation(&project.id, &accepted.operation_id);
        let error = resume.expect_err("failed ordinary resume must refuse");
        assert!(
            error.message.contains("reintentá"),
            "the failure must clearly indicate explicit retry is required: {}",
            error.message
        );

        assert_eq!(
            *calls.lock().unwrap(),
            0,
            "ordinary resume must not call the provider"
        );
        assert_eq!(state.test_activity().provider_calls, 0);
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(
            state.test_activity().embedding_inference,
            embeddings_after_index,
            "ordinary resume must not re-embed"
        );

        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let after = store.summary_operation(&operation_id).unwrap().unwrap();
        assert_eq!(
            after.status,
            project_knowledge::SummaryOperationStatus::Failed,
            "no mutation to Completed"
        );
        assert_eq!(
            after.retries, retries_before,
            "ordinary resume must not claim retry"
        );
        assert_eq!(
            store.summary_ids().unwrap(),
            node_ids_before,
            "no new summary nodes"
        );
    }

    /// MEDIUM 3.B: an explicit retry through the production seam claims the
    /// `failed` row exactly once, reuses Ready nodes, persists exactly one final
    /// artifact, and a second resume/retry after `completed` performs zero
    /// provider work.
    #[test]
    fn failed_explicit_retry_at_product_seam_completes_reusing_ready_nodes() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let paths = (0..3)
            .map(|i| {
                let p = tmp.path().join(format!("failed-b-{i}.md"));
                std::fs::write(&p, format!("body {i}")).unwrap();
                p.to_string_lossy().to_string()
            })
            .collect::<Vec<_>>();
        let accepted = state
            .send_staged_message_persist(&project.id, "resumime cada archivo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let embeddings_after_index = state.test_activity().embedding_inference;
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));

        // Pre-commit one document node as Ready so the retry must reuse it.
        {
            let mut store = KnowledgeStore::open(&root, &pid).unwrap();
            let identities = accepted
                .inputs
                .selected_material_ids
                .iter()
                .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
                .collect::<Vec<_>>();
            let fingerprint = super::summary_operation_fingerprint(
                "selected_sources",
                &identities,
                Some("opencode/big-pickle"),
            );
            store
                .create_summary_operation(
                    &operation_id,
                    accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                    "selected_sources",
                    &accepted.inputs.selected_material_ids,
                    &fingerprint,
                    Some("opencode/big-pickle"),
                )
                .unwrap();
            let plan = project_knowledge::plan_project_summaries(
                &store.summary_document_levels().unwrap(),
                &store.summary_existing().unwrap(),
                project_knowledge::BatchOptions::default(),
                None,
            )
            .unwrap();
            let mut doc = plan
                .pending
                .into_iter()
                .find(|node| node.level == project_knowledge::SummaryLevel::Document)
                .unwrap();
            let content = SummaryContent {
                summary: "ready doc reused".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            doc.state = project_knowledge::SummaryState::Ready;
            doc.content = Some(content.clone());
            doc.output_fingerprint = project_knowledge::fingerprint_output(&content);
            store.store_summary(&doc).unwrap();
            store
                .finish_summary_operation(
                    &operation_id,
                    project_knowledge::SummaryOperationStatus::Failed,
                    Some("summary_failure"),
                    None,
                    0,
                    0,
                    0,
                    0,
                )
                .unwrap();
        }

        let calls = Arc::new(Mutex::new(0));
        let levels = Arc::new(Mutex::new(Vec::new()));
        state.set_summarizer(Echo {
            calls: Arc::clone(&calls),
            levels: Arc::clone(&levels),
        });

        let run = state
            .retry_accepted_import_operation(&project.id, &accepted.operation_id)
            .unwrap();
        assert_eq!(run.status, "completed");

        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store.summary_operation(&operation_id).unwrap().unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(
            op.retries, 1,
            "Failed -> Running CAS claimed exactly once; retry count monotonic"
        );
        let final_summary_id = op.final_summary_id.clone().unwrap();
        let artifact = store.get_summary(&final_summary_id).unwrap().unwrap();
        assert_eq!(artifact.state, project_knowledge::SummaryState::Ready);
        assert!(artifact.content.is_some(), "final artifact readable");
        drop(store);

        // Ready nodes reused: only the two missing documents are re-synthesized.
        let seen = levels.lock().unwrap();
        assert_eq!(
            seen.iter()
                .filter(|level| **level == project_knowledge::SummaryLevel::Document)
                .count(),
            2,
            "the Ready document must be reused, not re-synthesized"
        );
        assert!(seen.contains(&project_knowledge::SummaryLevel::Global));

        // Second resume after Completed performs zero provider work.
        let provider_calls_after_retry = *calls.lock().unwrap();
        assert!(provider_calls_after_retry > 0);
        let second = state
            .resume_accepted_import_operation(&project.id, &accepted.operation_id)
            .unwrap();
        assert_eq!(second.status, "completed");
        assert_eq!(
            *calls.lock().unwrap(),
            provider_calls_after_retry,
            "no provider work after Completed"
        );
        assert_eq!(
            state.test_activity().embedding_inference,
            embeddings_after_index,
            "retry/recovery must not re-embed"
        );
        assert_eq!(state.test_activity().raw_attachment_forwarding, 0);
        assert_eq!(state.test_activity().k6_calls, 0);

        // final_summary_id remains durable and readable.
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert!(store.get_summary(&final_summary_id).unwrap().is_some());
    }

    /// MEDIUM 3.C: two concurrent explicit retries against the same `failed`
    /// operation must let exactly one own the SQLite CAS and the provider work,
    /// with no duplicate final artifact.
    #[test]
    fn concurrent_failed_retry_claims_exactly_once() {
        let tmp = tempfile::tempdir().unwrap();
        let state = Arc::new(app(tmp.path()));
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("failed-c.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        mark_summary_failed_for_turn(&root, &pid, &accepted, &operation_id);

        let calls = Arc::new(Mutex::new(0));
        state.set_summarizer(Multi {
            calls: Arc::clone(&calls),
        });

        let barrier = Arc::new(Barrier::new(3));
        let a = Arc::clone(&state);
        let b = Arc::clone(&state);
        let ba = Arc::clone(&barrier);
        let bb = Arc::clone(&barrier);
        let project_a = project.id.clone();
        let project_b = project.id.clone();
        let op_a = accepted.operation_id.clone();
        let op_b = accepted.operation_id.clone();
        let one = thread::spawn(move || {
            ba.wait();
            a.retry_accepted_import_operation(&project_a, &op_a)
        });
        let two = thread::spawn(move || {
            bb.wait();
            b.retry_accepted_import_operation(&project_b, &op_b)
        });
        barrier.wait();
        let r1 = one.join().unwrap();
        let r2 = two.join().unwrap();
        assert!(
            r1.is_ok() ^ r2.is_ok(),
            "exactly one Failed retry may own the CAS"
        );

        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let op = store.summary_operation(&operation_id).unwrap().unwrap();
        assert_eq!(
            op.status,
            project_knowledge::SummaryOperationStatus::Completed
        );
        assert_eq!(
            op.retries, 1,
            "exactly one CAS claim increments the retry count"
        );
        let final_id = op.final_summary_id.clone().unwrap();
        assert!(store.get_summary(&final_id).unwrap().is_some());
        let finals = store
            .summary_ids()
            .unwrap()
            .into_iter()
            .filter(|id| id.starts_with("summary-operation-final-"))
            .count();
        assert_eq!(finals, 1, "no duplicate final artifact");
        assert!(
            *calls.lock().unwrap() > 0,
            "the winner executed provider work"
        );
    }

    /// MEDIUM 1: a persisted final artifact whose `completed` CAS is lost to a
    /// legal terminal transition (here `cancelled`) is never silently ignored,
    /// never deleted, and never re-synthesized on reopen.
    #[test]
    fn persisted_final_artifact_with_lost_completed_cas_is_terminal_not_resynthesized() {
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("lost-cas.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime este archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let operation_id =
            super::summary_operation_id(accepted.inputs.turn_id.as_ref().map(|id| id.as_str()));
        let final_summary_id = format!(
            "summary-operation-final-{}",
            &super::sha256_hex(operation_id.as_bytes())[..24]
        );

        {
            let mut store = KnowledgeStore::open(&root, &pid).unwrap();
            let identities = accepted
                .inputs
                .selected_material_ids
                .iter()
                .map(|id| format!("{id}:{}", store.document_for_material(id).unwrap().unwrap()))
                .collect::<Vec<_>>();
            let fingerprint = super::summary_operation_fingerprint(
                "selected_sources",
                &identities,
                Some("opencode/big-pickle"),
            );
            store
                .create_summary_operation(
                    &operation_id,
                    accepted.inputs.turn_id.as_ref().map(|id| id.as_str()),
                    "selected_sources",
                    &accepted.inputs.selected_material_ids,
                    &fingerprint,
                    Some("opencode/big-pickle"),
                )
                .unwrap();
            store
                .summary_operation_begin_node(&operation_id, "n")
                .unwrap();
            store
                .finish_summary_operation(
                    &operation_id,
                    project_knowledge::SummaryOperationStatus::Cancelled,
                    None,
                    None,
                    0,
                    0,
                    0,
                    0,
                )
                .unwrap();

            let content = SummaryContent {
                summary: "final surface".into(),
                topics: vec![],
                decisions: vec![],
                action_items: vec![],
                questions: vec![],
            };
            let node = project_knowledge::SummaryNode {
                summary_id: final_summary_id.clone(),
                level: project_knowledge::SummaryLevel::Global,
                state: project_knowledge::SummaryState::Ready,
                failure: None,
                content: Some(content.clone()),
                source_ids: vec![],
                source_chunk_ids: vec![],
                parent_summary_id: None,
                input_fingerprint: format!("final-surface:{operation_id}"),
                output_fingerprint: project_knowledge::fingerprint_output(&content),
                generation_id: "operation-final-v1".into(),
                model_id: None,
                provider_id: None,
                contract_version: project_knowledge::SUMMARY_CONTRACT_VERSION.to_owned(),
                created_at: super::unix_seconds_now(),
                updated_at: super::unix_seconds_now(),
            };
            let outcome = store
                .commit_completed_summary_artifact(&operation_id, &node, 1, 0, 0, 1)
                .unwrap();
            assert_eq!(
                outcome,
                project_knowledge::SummaryCompletionOutcome::LostToTerminal(
                    project_knowledge::SummaryOperationStatus::Cancelled
                )
            );
            assert_eq!(
                store.get_summary(&final_summary_id).unwrap().unwrap().state,
                project_knowledge::SummaryState::Ready,
                "the persisted artifact is never deleted"
            );
            let after = store.summary_operation(&operation_id).unwrap().unwrap();
            assert_eq!(
                after.status,
                project_knowledge::SummaryOperationStatus::Cancelled
            );
            assert_eq!(after.final_summary_id, None);
        }

        drop(state);
        let fresh = app(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        fresh.set_summarizer(Multi {
            calls: Arc::clone(&calls),
        });
        let resume = fresh.resume_accepted_import_operation(&project.id, &accepted.operation_id);
        assert!(resume.is_err());
        assert_eq!(*calls.lock().unwrap(), 0);
        assert_eq!(fresh.test_activity().provider_calls, 0);
        assert_eq!(fresh.test_activity().k6_calls, 0);
        assert_eq!(fresh.test_activity().embedding_inference, 0);

        let store = KnowledgeStore::open(&root, &pid).unwrap();
        assert_eq!(
            store
                .get_summary(&final_summary_id)
                .unwrap()
                .unwrap()
                .content
                .unwrap()
                .summary,
            "final surface"
        );
        assert_eq!(
            store
                .summary_operation(&operation_id)
                .unwrap()
                .unwrap()
                .status,
            project_knowledge::SummaryOperationStatus::Cancelled
        );
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
                .send_summary_run_with(
                    inputs,
                    &summarizer,
                    crate::intent::SummaryExecutionKind::WholeCorpus,
                )
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
        let prompt = "haceme un resumen detallado de cada archivo ordenado por fecha";
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
        let run = state
            .send_summary_run_with(
                inputs,
                &summarizer,
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        for name in names {
            assert!(surface.contains(name));
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

    /// One-source-of-truth regression (architecture §15/§16): the K6 terminal
    /// consumes the normalized route, never re-deriving the summary scope from
    /// prompt wording. "resumime cada archivo" would be SelectedPerSource to the
    /// legacy detector, but the explicitly supplied WholeCorpus route must run
    /// the project scope.
    #[test]
    fn k6_consumes_normalized_route_not_prompt_wording() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();
        let path = tmp.path().join("one.md");
        std::fs::write(&path, "body").unwrap();
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime cada archivo",
                &[path.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        let run = state
            .send_summary_run_with(
                accepted.inputs,
                &Multi {
                    calls: Arc::new(Mutex::new(0)),
                },
                crate::intent::SummaryExecutionKind::WholeCorpus,
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        let pid = ProjectId::parse(&project.id).unwrap();
        let store =
            KnowledgeStore::open(tmp.path().join("projects").join(&project.id), &pid).unwrap();
        let op = store
            .summary_operation_for_turn(run.turn_id.as_deref().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(
            op.scope, "project",
            "K6 must consume the normalized route, never prompt wording"
        );
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

    /// A provider that resolves a valid generation but fails every embedding
    /// call. It exercises the failure path end-to-end without the bundled ONNX
    /// model, so the readiness invariant can be asserted at the app seam.
    struct FailingEmbeddings {
        generation: EmbeddingGeneration,
    }
    impl EmbeddingProvider for FailingEmbeddings {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, _query: &str) -> project_knowledge::Result<Vec<f32>> {
            Err(project_knowledge::KnowledgeError::Inference(
                "synthetic failure".to_owned(),
            ))
        }
        fn embed_passages(
            &mut self,
            _passages: &[String],
        ) -> project_knowledge::Result<Vec<Vec<f32>>> {
            Err(project_knowledge::KnowledgeError::Inference(
                "synthetic failure".to_owned(),
            ))
        }
    }

    /// CASE 2 (app level): a failed embedding must never report a material as
    /// semantically ready. Even though lexical indexing completed, the resolved
    /// generation means readiness stays generation-scoped, so `materialsReady`
    /// is 0 and the operation records a truthful failure.
    #[test]
    fn failed_embedding_keeps_materials_not_ready_at_the_app_seam() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = (0..3)
            .map(|index| {
                let path = corpus.join(format!("nota-{index}.md"));
                std::fs::write(&path, format!("# Nota {index}\n\ncontenido {index}\n")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();

        state.set_local_embedding_provider(FailingEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );

        let view = state.open_project(&project.id).unwrap();
        let op = view.accepted_import.clone().unwrap();
        assert_eq!(op.lexical_completed, 3, "lexical indexing still completed");
        assert_eq!(
            op.materials_ready, 0,
            "a failed embedding must never report materials ready"
        );
        assert_eq!(op.embeddings_created, 0);

        // The resolved generation must remain durable even on failure (the
        // signal that keeps readiness generation-scoped rather than degraded).
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let durable = store
            .accepted_import_operation(&accepted.operation_id)
            .unwrap()
            .expect("operation exists");
        assert!(
            durable.embedding_generation_id.is_some(),
            "the resolved generation must remain durable even on failure"
        );
    }

    /// Regression for the counter-unit defect: after the embedding pass,
    /// `index_accepted_material_batch` must keep `embedding_completed` /
    /// `embeddings_created` / `embeddings_reused` in chunk/vector units. The
    /// previous code wrote the Material `indexed` count into
    /// `embedding_completed`, so a 50-material / 31778-chunk operation regressed
    /// from 31778/31778 to 50/31778 and progress moved backwards.
    #[test]
    fn accepted_import_embedding_progress_stays_in_chunk_units_after_app_indexing() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        // Three materials, each long enough to produce several chunks, so the
        // chunk/vector count is strictly greater than the Material count.
        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = (0..3)
            .map(|index| {
                let path = corpus.join(format!("nota-{index}.md"));
                let body = (0..4)
                    .map(|part| {
                        format!(
                            "# Nota {index} parte {part}\n\nEl fragmento {part} del documento {index} contiene texto suficiente para superar el límite de fragmentación y generar varios fragmentos por archivo.\n\n"
                        )
                    })
                    .collect::<String>();
                std::fs::write(&path, body).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();
        let material_count = accepted.material_ids.len();
        assert_eq!(material_count, 3);

        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );

        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store
            .accepted_import_operation(&accepted.operation_id)
            .unwrap()
            .expect("operation exists");

        // 1. After embedding the chunk-level total exceeds the material count.
        assert!(
            operation.chunks_total > material_count,
            "expected more chunks ({}) than materials ({material_count})",
            operation.chunks_total
        );
        // 2. The monotonic chunk-unit invariant holds.
        assert!(operation.embedding_completed <= operation.embeddings_total);
        // 3. A completed embedding run preserves the exact chunk/vector count,
        //    never the Material count.
        assert_eq!(operation.embedding_completed, operation.embeddings_total);
        assert_eq!(operation.embedding_completed, operation.embeddings_created);
        assert_eq!(operation.embeddings_reused, 0);
        assert_eq!(
            operation.embeddings_created + operation.embeddings_reused,
            operation.chunks_total
        );
        assert_ne!(
            operation.embedding_completed, material_count,
            "embedding counters must never hold the material count"
        );
        // 4. Material-level counters remain independently correct.
        assert_eq!(operation.lexical_completed, material_count);
        assert_eq!(operation.copied, material_count);

        // 5. The exact durable state the UI polls after the embedding pass.
        let polled = state
            .open_project(&project.id)
            .unwrap()
            .accepted_import
            .expect("polled operation");
        assert_eq!(polled.embeddings_total, operation.embeddings_total);
        assert_eq!(polled.embedding_completed, operation.embedding_completed);
        assert_eq!(polled.embeddings_created, operation.embeddings_created);
        assert_eq!(polled.chunks_total, operation.chunks_total);
        assert!(polled.embedding_completed <= polled.embeddings_total);
        assert!(polled.embedding_completed > polled.lexical_completed);
    }

    /// REPRO: a large 50-material corpus (many chunks each) through the exact
    /// production `index_accepted_material_batch` seam, with a real (fake)
    /// provider, must persist embeddings without a `persist` failure.
    #[test]
    fn large_corpus_embeddings_persist_through_app_batch_seam() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = (0..50)
            .map(|index| {
                let path = corpus.join(format!("nota-{index:02}.md"));
                let body = (0..200)
                    .map(|part| {
                        format!(
                            "# Parte {part}\n\nEl fragmento {part} del documento {index} contiene texto de relleno suficientemente largo como para superar el límite de fragmentación y generar muchos fragmentos por archivo en el corpus de prueba.\n\n"
                        )
                    })
                    .collect::<String>();
                std::fs::write(&path, body).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();

        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );

        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store
            .accepted_import_operation(&accepted.operation_id)
            .unwrap()
            .expect("operation exists");
        assert!(
            operation.embeddings_created > 0,
            "large corpus must persist embeddings (embeddings_created={})",
            operation.embeddings_created
        );
        assert_eq!(
            operation.embeddings_created + operation.embeddings_reused,
            operation.chunks_total
        );
        let stats = store.corpus_stats().unwrap();
        assert!(stats.embeddings_ready > 0, "embeddings_ready must be > 0");
    }

    /// CASE 1 + CASE 7 (app level): the polled `acceptedImport.materialsReady`
    /// counts fully usable materials for the active embedding generation, and a
    /// reopen of a brand-new `AppState` over the same data dir recomputes the
    /// count from durable state without re-running the embedding phase.
    #[test]
    fn accepted_import_materials_ready_is_truthful_and_survives_restart_without_reembedding() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = (0..50)
            .map(|index| {
                let path = corpus.join(format!("nota-{index:02}.md"));
                std::fs::write(
                    &path,
                    format!("# Nota {index}\n\ncontenido breve {index}\n"),
                )
                .unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();

        // Before the local pipeline runs, nothing is usable: lexically-ready
        // materials with pending embeddings must not count (CASE 12).
        let before = state.open_project(&project.id).unwrap();
        let op_before = before.accepted_import.clone().unwrap();
        assert_eq!(op_before.total, 50);
        assert_eq!(op_before.materials_ready, 0);

        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );

        let polled = state.open_project(&project.id).unwrap();
        let op = polled.accepted_import.clone().unwrap();
        assert_eq!(op.materials_ready, 50, "every accepted material is usable");
        assert_eq!(op.total, 50);
        assert_eq!(op.lexical_completed, 50);

        // Restart proof (CASE 7): a brand-new AppState over the same data dir
        // recomputes `materialsReady` from durable Knowledge state and must not
        // re-run the embedding phase merely to display progress.
        let restarted = app(tmp.path());
        let reopened = restarted.open_project(&project.id).unwrap();
        assert_eq!(
            reopened.accepted_import.clone().unwrap().materials_ready,
            50
        );
        assert_eq!(
            restarted.test_activity.lock().unwrap().embedding_inference,
            0,
            "reopening must not trigger inference"
        );
    }

    /// CASE 4: unsupported files are filtered during staging, stay outside the
    /// accepted-operation total, and never inflate `materialsReady`.
    #[test]
    fn unsupported_staging_files_stay_outside_the_accepted_total() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let mut paths = Vec::new();
        for index in 0..2 {
            let path = corpus.join(format!("bueno-{index}.md"));
            std::fs::write(&path, format!("# Nota {index}\n\ncontenido {index}\n")).unwrap();
            paths.push(path.to_string_lossy().to_string());
        }
        // A directory is "unsupported" at staging (not a regular file).
        let dir = corpus.join("carpeta");
        std::fs::create_dir_all(&dir).unwrap();
        paths.push(dir.to_string_lossy().to_string());
        // A file over the size limit is also "unsupported" at staging.
        let huge = corpus.join("enorme.md");
        std::fs::write(
            &huge,
            vec![b'x'; usize::try_from(MAX_IMPORT_FILE_BYTES + 1).unwrap()],
        )
        .unwrap();
        paths.push(huge.to_string_lossy().to_string());

        let staged = state.stage_attachment_paths(&paths);
        let unsupported = staged
            .items
            .iter()
            .filter(|item| item.status == "unsupported")
            .count();
        assert_eq!(
            unsupported, 2,
            "both invalid sources are rejected at staging"
        );

        // The frontend only sends `ready`/`duplicate_in_selection` paths to the
        // accepted-turn command, exactly like `attachments_stage_paths` ->
        // `agent_send_staged` in the app. Unsupported files are filtered before
        // acceptance and therefore stay outside the operation total.
        let accepted_paths = paths
            .iter()
            .zip(&staged.items)
            .filter(|(_, item)| item.status == "ready" || item.status == "duplicate_in_selection")
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        assert_eq!(accepted_paths.len(), 2);

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &accepted_paths, &[])
            .unwrap();
        assert!(accepted.turn_id().is_some());
        let view = state.open_project(&project.id).unwrap();
        let op = view.accepted_import.clone().unwrap();
        assert_eq!(
            op.total, 2,
            "unsupported files never join the accepted total"
        );
        assert_eq!(view.materials.len(), 2);
    }

    /// CASE 6: a cancelled/interrupted operation keeps the last truthful count;
    /// no unfinished material is ever claimed ready.
    #[test]
    fn cancelled_operation_keeps_the_last_truthful_materials_ready_count() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = app(tmp.path());
        let project = state.create_project("P").unwrap();

        let corpus = tmp.path().join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let paths: Vec<String> = (0..10)
            .map(|index| {
                let path = corpus.join(format!("nota-{index:02}.md"));
                std::fs::write(&path, format!("# Nota {index}\n\ncontenido {index}\n")).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect();

        let accepted = state
            .send_staged_message_persist(&project.id, "Procesá todo", &paths, &[])
            .unwrap();
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        // Interrupted: only the first five materials run through the real
        // batch seam (lexical + embedding); the rest stay pending/unknown.
        let five = accepted
            .material_ids
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>();
        let pid = ProjectId::parse(&project.id).unwrap();
        state.index_accepted_material_batch(&pid, &five, Some(&accepted.operation_id), false);
        drop(pid);

        let view = state.open_project(&project.id).unwrap();
        let op = view.accepted_import.clone().unwrap();
        assert_eq!(op.materials_ready, 5, "only the five finished files count");
        assert_eq!(op.total, 10);
        assert!(
            op.materials_ready < op.total,
            "unfinished files are never ready"
        );
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
            .index_embeddings_for_materials(&mut embeddings, EMBEDDING_BATCH_SIZE, &material_ids)
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
            .send_summary_run_with(
                accepted.inputs,
                &summarizer,
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            )
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

#[cfg(test)]
mod inventory_local_tests {
    use super::*;
    use project_agent::FakeAgentEngine;
    use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
    use project_tunnel::FakeTunnel;

    fn connector() -> FakeProviderConnector {
        FakeProviderConnector::new()
            .with_provider(ProviderDetail {
                id: "opencode".into(),
                name: "Gratis".into(),
                auth_methods: Vec::new(),
                connections: Vec::new(),
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

    fn inventory_app(
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

    /// A pure inventory turn must never reach the embedding provider, the
    /// agent engine, or the provider connector: `embedding_inference == 0`,
    /// `provider_calls == 0`. Only the local knowledge-context preparation
    /// (which opens the persisted store) counts as activity.
    #[test]
    fn inventory_turn_generates_zero_embeddings_and_zero_provider_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let state = inventory_app(tmp.path());
        let project = state.create_project("InvZero").unwrap();
        for name in ["uno.md", "dos.md", "tres.md"] {
            let src = tmp.path().join(name);
            fs::write(&src, format!("contenido de {name}\n")).unwrap();
            state
                .add_material_from_path(&project.id, src.to_str().unwrap())
                .unwrap();
            let _ = fs::remove_file(&src);
        }
        let before = state.test_activity();
        let run = state
            .send_message(&project.id, "listame todos los archivos", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.expect("inventory answer");
        assert!(message.contains("Tenés 3 materiales registrados y listos en Knowledge"));
        assert!(message.contains("uno.md") && message.contains("tres.md"));
        let after = state.test_activity();
        // The inventory turn itself must add zero embedding inference, zero
        // provider calls, zero K6 calls, and zero raw attachment forwarding.
        // (Material indexing performed before the send may have probed the
        // embedding provider, which is why the delta is asserted, not the
        // absolute counters.)
        assert_eq!(
            after.embedding_inference - before.embedding_inference,
            0,
            "inventory must generate no query embeddings"
        );
        assert_eq!(
            after.provider_calls - before.provider_calls,
            0,
            "inventory must never invoke the provider"
        );
        assert_eq!(after.k6_calls - before.k6_calls, 0);
        assert_eq!(
            after.raw_attachment_forwarding - before.raw_attachment_forwarding,
            0
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(0));
        assert_eq!(metrics.retrieval_mode, None);
        assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
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

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod contextual_followup_tests {
    use super::*;
    use project_agent::FakeAgentEngine;
    use project_agent::model::{
        AgentBackendInfo, AgentKnowledgeContext, AgentProject, AgentPrompt, AgentSession,
        AgentStatus, AgentTask,
    };
    use project_knowledge::{
        EmbeddingGeneration, EmbeddingProvider, KnowledgeStore, ModelManifest, RemoteSummarizer,
        SummaryFailure, SummaryOutput, SummaryRequest, SummaryUsage,
    };
    use project_opencode::OpenCodeBackend;
    use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
    use project_tunnel::FakeTunnel;
    use std::path::PathBuf;

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

    struct RecordingEngine(FakeAgentEngine, Arc<Mutex<Vec<String>>>);

    impl project_agent::AgentEngine for RecordingEngine {
        fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
            self.0.ensure_ready()
        }
        fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
            self.0.open_session(project)
        }
        fn send(
            &self,
            session: &AgentSession,
            req: &AgentPrompt,
        ) -> project_agent::AgentResult<AgentTask> {
            self.1.lock().unwrap().push(req.text.clone());
            self.0.send(session, req)
        }
        fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
            self.0.cancel(session)
        }
        fn status(&self) -> AgentStatus {
            self.0.status()
        }
        fn shutdown(&self) -> project_agent::AgentResult<()> {
            self.0.shutdown()
        }
    }

    /// Controlled provider seam with an intentionally stateful cached session
    /// and separately allocated fresh sessions. Knowledge synthesis that
    /// serializes evidence must use the fresh path and must not replace the
    /// conversational cache.
    #[derive(Clone, Default)]
    struct SessionIsolationEngine {
        cached: Arc<Mutex<Option<AgentSession>>>,
        fresh_counter: Arc<Mutex<usize>>,
        sent: Arc<Mutex<Vec<CapturedProviderRequest>>>,
        fail_next_send: Arc<Mutex<bool>>,
        cancelled: Arc<Mutex<Vec<String>>>,
        session_prefix: Arc<Mutex<String>>,
        conversational_generation: Arc<Mutex<u32>>,
    }

    /// What crossed the real AgentEngine boundary. Keeping the structured
    /// Knowledge context lets the regression assert the same request proves
    /// both fresh-session isolation and bounded RAG serialization.
    #[derive(Clone, Debug)]
    struct CapturedProviderRequest {
        session_id: String,
        text: String,
        knowledge: Option<AgentKnowledgeContext>,
        model: Option<project_agent::ModelRef>,
    }

    impl project_agent::AgentEngine for SessionIsolationEngine {
        fn ensure_ready(&self) -> project_agent::AgentResult<AgentBackendInfo> {
            Ok(AgentBackendInfo {
                version: "test".to_owned(),
            })
        }

        fn open_session(&self, project: &AgentProject) -> project_agent::AgentResult<AgentSession> {
            let prefix = self.session_prefix.lock().unwrap().clone();
            let generation = *self.conversational_generation.lock().unwrap();
            let id = match (prefix.is_empty(), generation) {
                (true, 0) => "cached-project-session".to_owned(),
                (true, n) => format!("cached-project-session-{n}"),
                (false, 0) => format!("{prefix}-cached-project-session"),
                (false, n) => format!("{prefix}-cached-project-session-{n}"),
            };
            let mut cached = self.cached.lock().unwrap();
            let session = cached.get_or_insert_with(|| AgentSession {
                id,
                project_id: project.project_id.clone(),
            });
            Ok(AgentSession {
                id: session.id.clone(),
                project_id: session.project_id.clone(),
            })
        }

        fn invalidate_cached_session(&self, _project_id: &str) {
            *self.cached.lock().unwrap() = None;
            *self.conversational_generation.lock().unwrap() += 1;
        }

        fn open_fresh_session(
            &self,
            project: &AgentProject,
        ) -> project_agent::AgentResult<AgentSession> {
            let mut counter = self.fresh_counter.lock().unwrap();
            *counter += 1;
            Ok(AgentSession {
                id: format!("fresh-normal-{}", *counter),
                project_id: project.project_id.clone(),
            })
        }

        fn send(
            &self,
            session: &AgentSession,
            req: &AgentPrompt,
        ) -> project_agent::AgentResult<AgentTask> {
            self.sent.lock().unwrap().push(CapturedProviderRequest {
                session_id: session.id.clone(),
                text: req.text.clone(),
                knowledge: req.knowledge.clone(),
                model: req.model.clone(),
            });
            if *self.fail_next_send.lock().unwrap() {
                *self.fail_next_send.lock().unwrap() = false;
                return Err(project_agent::AgentError::TaskFailed(
                    "injected ephemeral failure".into(),
                ));
            }
            Ok(AgentTask {
                id: format!("{}-task", session.id),
                status: project_agent::TaskStatus::Completed,
                artifacts: Vec::new(),
                message: Some("Respuesta de prueba.".to_owned()),
                usage: project_agent::RemoteUsage::default(),
            })
        }

        fn cancel(&self, session: &AgentSession) -> project_agent::AgentResult<()> {
            self.cancelled.lock().unwrap().push(session.id.clone());
            Ok(())
        }

        fn status(&self) -> AgentStatus {
            AgentStatus::Ready
        }

        fn shutdown(&self) -> project_agent::AgentResult<()> {
            Ok(())
        }
    }

    fn isolation_knowledge_state(
        tmp: &tempfile::TempDir,
        intent: crate::intent::Intent,
    ) -> (
        SessionIsolationEngine,
        AppState<SessionIsolationEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    ) {
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: Arc::new(Mutex::new(0)),
                intent,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        (engine, state)
    }

    fn recording_app(
        base: &std::path::Path,
        engine: RecordingEngine,
    ) -> AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
        AppState::with_components(
            base.to_path_buf(),
            engine,
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        )
    }

    /// A ready FakeAgentEngine that returns a non-empty assistant message, plus
    /// the engine-call recorder. Turns that legitimately reach the agent engine
    /// (ordinary chat, scoped exhaustive, thematic) need this.
    fn ready_engine() -> (FakeAgentEngine, Arc<Mutex<Vec<String>>>) {
        let fake = FakeAgentEngine::new();
        fake.set_message("Respuesta del asistente de prueba.".to_owned());
        (fake, Arc::new(Mutex::new(Vec::new())))
    }

    fn add_file<E: AgentEngine>(
        app: &AppState<E, FakeTunnel, FakeProviderConnector, FakeRestarter>,
        base: &std::path::Path,
        project_id: &str,
        name: &str,
        body: &str,
    ) -> String {
        let src = base.join(name);
        fs::write(&src, body).unwrap();
        let material = app
            .add_material_from_path(project_id, src.to_str().unwrap())
            .unwrap();
        let _ = fs::remove_file(&src);
        material.id
    }

    /// Fake per-item summarizer: one `summarize` call per aggregate request,
    /// returns exactly one `Mxxx:` line per requested label. Counts calls.
    #[derive(Clone)]
    struct PerItemFake {
        calls: Arc<Mutex<usize>>,
    }

    impl RemoteSummarizer for PerItemFake {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            *self.calls.lock().unwrap() += 1;
            let mut text = String::new();
            for item in &request.labels {
                text.push_str(&format!(
                    "{}: Resumen de {}\n",
                    item.label, item.source_name
                ));
            }
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    cost_usd: Some(0.01),
                    provider_actual: true,
                },
            })
        }
    }

    /// A per-item summarizer that deliberately omits the last `omit_last` keys
    /// of every batch, to prove a partial provider output is never presented as
    /// a fully successful turn.
    struct OmittingPerItemFake {
        calls: Arc<Mutex<usize>>,
        omit_last: usize,
    }

    impl RemoteSummarizer for OmittingPerItemFake {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            *self.calls.lock().unwrap() += 1;
            let keep = request.labels.len().saturating_sub(self.omit_last);
            let mut text = String::new();
            for item in request.labels.iter().take(keep) {
                text.push_str(&format!(
                    "{}: Resumen de {}\n",
                    item.label, item.source_name
                ));
            }
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    cost_usd: Some(0.01),
                    provider_actual: true,
                },
            })
        }
    }

    /// A per-item summarizer that observes the durable `synthesizing` flag of
    /// the newest accepted-import operation at provider-call time, proving the
    /// synthesis phase is marked before the remote boundary runs.
    struct SynthesizingObserver {
        base: PathBuf,
        project_id: Arc<Mutex<Option<String>>>,
        observed: Arc<Mutex<Vec<bool>>>,
    }

    impl RemoteSummarizer for SynthesizingObserver {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            let project_id = self
                .project_id
                .lock()
                .unwrap()
                .clone()
                .expect("project id set");
            let pid = ProjectId::parse(&project_id).unwrap();
            let root = self.base.join("projects").join(&project_id);
            let store = KnowledgeStore::open(&root, &pid).unwrap();
            let operation = store.latest_accepted_import_operation().unwrap().unwrap();
            self.observed.lock().unwrap().push(operation.synthesizing);
            let mut text = String::new();
            for item in &request.labels {
                text.push_str(&format!(
                    "{}: Resumen de {}\n",
                    item.label, item.source_name
                ));
            }
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage::default(),
            })
        }
    }

    fn count_lines(message: &str) -> usize {
        message.lines().count()
    }

    /// Case A + §20: inventory 50 -> "resumí cada uno" -> exactly 50 entries,
    /// exactly 1 aggregate remote call, no re-embedding, no top-K shrink, no
    /// global Fuentes block, exact source associations preserved.
    #[test]
    fn inventory_50_then_resumi_cada_uno_produces_50_entries_with_one_remote_call() {
        let tmp = tempfile::tempdir().unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), calls.clone()),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("FollowUpA").unwrap();
        for index in 0..50 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("material-{index:03}.md"),
                &format!("Contenido del material {index} sobre gramática.\n"),
            );
        }
        let inventory = state
            .send_message(&project.id, "listar los archivos de forma cronológica", &[])
            .unwrap();
        assert_eq!(inventory.status, "completed");
        assert!(
            inventory
                .message
                .as_deref()
                .unwrap()
                .contains("Tenés 50 materiales")
        );

        let before = state.test_activity();
        let run = state
            .send_message(
                &project.id,
                "haceme un resumen de no más de 20 palabras por cada uno",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.expect("per-item answer");
        assert_eq!(
            count_lines(&message),
            50,
            "exactly one result slot per referent material: {message}"
        );
        for index in 0..50 {
            let name = format!("material-{index:03}.md");
            assert!(
                message.contains(&format!("{name}: Resumen de")),
                "exact source association must survive: {name}"
            );
        }
        assert!(
            !message.contains("Fuentes:"),
            "per-item provenance is per item; a global Fuentes block is forbidden: {message}"
        );

        // No re-embedding and no agent/provider calls for the follow-up turn.
        let after = state.test_activity();
        assert_eq!(
            after.embedding_inference - before.embedding_inference,
            0,
            "the per-item path must never re-embed"
        );
        assert_eq!(
            after.provider_calls - before.provider_calls,
            0,
            "the per-item path must not run a normal chat provider call"
        );
        let engine_calls = calls.lock().unwrap();
        assert_eq!(
            engine_calls.len(),
            0,
            "the OpenCode agent engine must never run for a per-item summary"
        );
        drop(engine_calls);

        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(50));
        assert_eq!(metrics.base_intent.as_deref(), Some("normal"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("per_item_summary"));
        assert_eq!(metrics.remote_calls, Some(1));
        assert_eq!(metrics.input_tokens, Some(100));
        assert_eq!(metrics.output_tokens, Some(50));
        assert_eq!(metrics.cost_usd, Some(0.01));
        assert_eq!(metrics.source.as_deref(), Some("provider_actual"));
        assert_eq!(
            metrics.semantic_provider_state.as_deref(),
            Some("not_requested")
        );
        assert_eq!(metrics.retrieval_mode, None);
        assert_eq!(metrics.local_mode.as_deref(), Some("per_item_summary"));
        assert_eq!(metrics.retrieval_candidate_count, None);
        assert_eq!(metrics.selected_evidence_count, None);
    }

    /// §22: the 50-material per-item summary makes exactly ONE aggregate
    /// remote call (never one per material).
    #[test]
    fn fifty_materials_produce_exactly_one_aggregate_remote_call() {
        let tmp = tempfile::tempdir().unwrap();
        let summarizer_calls = Arc::new(Mutex::new(0));
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: summarizer_calls.clone(),
        });
        let project = state.create_project("CallCount").unwrap();
        for index in 0..50 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("m-{index:03}.md"),
                "contenido\n",
            );
        }
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*summarizer_calls.lock().unwrap(), 1);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(1));
    }

    /// §22 batching: a forced small batch proves the remote-call count is
    /// ceil(materials / batch), never the material count.
    #[test]
    fn forced_batching_produces_a_small_bounded_number_of_remote_calls() {
        let tmp = tempfile::tempdir().unwrap();
        let summarizer_calls = Arc::new(Mutex::new(0));
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: summarizer_calls.clone(),
        });
        state.set_per_item_options(crate::per_item::PerItemExecutionOptions {
            max_items_per_call: 25,
            max_words_per_item: 40,
            max_serialized_bytes_per_call: usize::MAX,
        });
        let project = state.create_project("Batch").unwrap();
        for index in 0..50 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("b-{index:03}.md"),
                "contenido\n",
            );
        }
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            *summarizer_calls.lock().unwrap(),
            2,
            "50 materials with a 25-per-call batch must use exactly 2 calls"
        );
        let message = run.message.unwrap();
        assert_eq!(count_lines(&message), 50);
    }

    /// Case G: 100+ explicit materials -> no top-K truncation and bounded calls.
    #[test]
    fn one_hundred_plus_materials_keep_exact_cardinality() {
        let tmp = tempfile::tempdir().unwrap();
        let summarizer_calls = Arc::new(Mutex::new(0));
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: summarizer_calls.clone(),
        });
        let project = state.create_project("Hundred").unwrap();
        for index in 0..105 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("h-{index:03}.md"),
                "contenido\n",
            );
        }
        state
            .send_message(&project.id, "listame todos los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.unwrap();
        assert_eq!(count_lines(&message), 105, "no top-K truncation");
        assert_eq!(
            *summarizer_calls.lock().unwrap(),
            2,
            "105 materials at 100-per-call use exactly 2 aggregate calls"
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.referent_count, Some(105));
        assert_eq!(metrics.remote_calls, Some(2));
    }

    /// Case H: a referent material deleted before the follow-up keeps an
    /// explicit failure slot; cardinality is never silently reduced.
    #[test]
    fn deleted_material_keeps_an_explicit_slot() {
        let tmp = tempfile::tempdir().unwrap();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("Deleted").unwrap();
        add_file(&state, tmp.path(), &project.id, "a.md", "contenido a\n");
        add_file(&state, tmp.path(), &project.id, "b.md", "contenido b\n");
        add_file(&state, tmp.path(), &project.id, "c.md", "contenido c\n");
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        // Remove material b from the project and its Knowledge linkage.
        let material_b = state
            .open_project(&project.id)
            .unwrap()
            .materials
            .iter()
            .find(|material| material.display_name == "b.md")
            .unwrap()
            .id
            .clone();
        state.remove_material(&project.id, &material_b).unwrap();
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.unwrap();
        assert_eq!(count_lines(&message), 3, "cardinality must stay 3");
        assert!(message.contains("Este archivo ya no está disponible en Knowledge."));
        assert!(message.contains("1. a.md:"), "{message}");
        assert!(message.contains("3. c.md:"), "{message}");
    }

    /// Case D + §15: the referent survives an app restart and the follow-up
    /// resolves without re-embedding or provider work.
    #[test]
    fn referent_survives_restart_and_followup_resolves_locally() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let project = {
            let state = recording_app(
                &base,
                RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
            );
            let project = state.create_project("RestartFollowUp").unwrap();
            for index in 0..12 {
                add_file(
                    &state,
                    &base,
                    &project.id,
                    &format!("r-{index:02}.md"),
                    "contenido\n",
                );
            }
            state
                .send_message(&project.id, "listame los archivos", &[])
                .unwrap();
            project
        };
        // Simulated restart: a brand-new AppState over the same base dir.
        let state = recording_app(
            &base,
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.unwrap();
        assert_eq!(count_lines(&message), 12);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_count, Some(12));
        assert!(metrics.origin_turn_id.is_some());
    }

    /// Case E + §18: an unrelated question never inherits a previous referent.
    #[test]
    fn unrelated_question_never_inherits_the_previous_referent() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, calls) = ready_engine();
        let state = recording_app(tmp.path(), RecordingEngine(fake, calls.clone()));
        let project = state.create_project("Unrelated").unwrap();
        for index in 0..5 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("u-{index}.md"),
                "contenido\n",
            );
        }
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "¿Cuál es la capital de Francia?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert!(
            !calls.lock().unwrap().is_empty(),
            "an unrelated question must follow the ordinary agent path"
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_ne!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_count, None);
        assert_eq!(metrics.turn_kind, None);
    }

    /// Case K: "cada uno" with no prior referent -> no crash, no invented set.
    #[test]
    fn cada_uno_without_prior_referent_is_ordinary_and_safe() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("NoPrior").unwrap();
        add_file(&state, tmp.path(), &project.id, "solo.md", "contenido\n");
        let run = state
            .send_message(&project.id, "resumí cada uno", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_ne!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.local_mode.as_deref(), None);
    }

    /// Case I: pure KnowledgeInventory keeps remote_calls=0 and still captures
    /// a durable MaterialSet referent.
    #[test]
    fn inventory_turn_is_zero_remote_and_persists_a_material_set_referent() {
        let tmp = tempfile::tempdir().unwrap();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("InvReferent").unwrap();
        for index in 0..4 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("i-{index}.md"),
                "contenido\n",
            );
        }
        let run = state
            .send_message(&project.id, "listame todos los archivos", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(0));
        assert_eq!(metrics.retrieval_mode, None);
        assert_eq!(metrics.local_mode.as_deref(), Some("inventory"));
        assert_eq!(metrics.contextual_followup, None);
        // The MaterialSet referent is durably persisted in project.json.
        let disk = fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&disk).unwrap();
        let messages = json["messages"].as_array().unwrap();
        let user = messages
            .iter()
            .find(|message| message["role"] == "user")
            .unwrap();
        assert_eq!(user["turnReferent"]["kind"], "materialSet");
        assert_eq!(
            user["turnReferent"]["materialIds"]
                .as_array()
                .unwrap()
                .len(),
            4
        );
        assert_eq!(user["turnReferent"]["sourceNames"][0], "i-0.md");
        assert_eq!(user["turnReferent"]["producedBy"], "inventory");
        assert!(
            !user["turnReferent"]["originTurnId"]
                .as_str()
                .unwrap()
                .is_empty()
        );
    }

    /// Case J: a corpus-grounded open question uses compact retrieval when the
    /// semantic classifier says so — not because of a local question-word list.
    #[test]
    fn ordinary_semantic_question_keeps_normal_behavior() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::NormalSemantic,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let project = state.create_project("Normal").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "n.md",
            "Se explicó el presente continuo en la clase.\n",
        );
        let run = state
            .send_message(
                &project.id,
                "¿Qué explicó Delfina sobre presente continuo?",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
        assert_eq!(metrics.contextual_followup, None);
    }

    #[test]
    fn normal_semantic_production_path_excludes_huge_cached_provider_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        let project = state.create_project("SessionIsolation").unwrap();
        let stale_user = "HUGE-STALE-PROVIDER-TRANSCRIPT-SENTINEL-XYZ ".repeat(4_000);
        let stale_assistant = "HUGE-STALE-ASSISTANT-ANSWER-SENTINEL-XYZ ".repeat(4_000);
        let cached = engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        engine
            .send(
                &cached,
                &AgentPrompt {
                    text: stale_user,
                    model: None,
                    knowledge: None,
                    conversation_context: None,
                },
            )
            .unwrap();
        engine
            .send(
                &cached,
                &AgentPrompt {
                    text: stale_assistant,
                    model: None,
                    knowledge: None,
                    conversation_context: None,
                },
            )
            .unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "knowledge.md",
            "El incidente INC-12345 fue cerrado en OpenShift.\n",
        );
        // This is deliberately much larger than the bounded K4 request. It
        // must never be forwarded whole as a raw attachment or corpus body.
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "unrelated-long.md",
            &format!("{}\n", "UNRELATED-WHOLE-CORPUS-RAW-BODY ".repeat(8_000)),
        );
        state.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::NormalSemantic,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let current_prompt = "INC-12345";
        state
            .send_message(&project.id, current_prompt, &[])
            .unwrap();
        let sent = engine.sent.lock().unwrap();
        let request = sent.last().unwrap();
        assert_ne!(request.session_id, "cached-project-session");
        assert!(request.session_id.starts_with("fresh-normal-"));
        assert!(request.text.contains(current_prompt));
        assert!(
            !request
                .text
                .contains("HUGE-STALE-PROVIDER-TRANSCRIPT-SENTINEL-XYZ")
        );
        assert!(
            !request
                .text
                .contains("HUGE-STALE-ASSISTANT-ANSWER-SENTINEL-XYZ")
        );
        let knowledge = request
            .knowledge
            .as_ref()
            .expect("NormalSemantic request must carry bounded Knowledge evidence");
        assert_eq!(knowledge.retrieval_mode.as_deref(), Some("normal"));
        assert!(!knowledge.entries.is_empty());
        assert!(knowledge.entries.len() <= 8);
        assert!(knowledge.evidence_budget_used <= knowledge.evidence_budget_limit);
        assert!(knowledge.evidence_budget_limit <= 2_700);
        assert!(knowledge.entries.iter().any(|entry| {
            entry.source_name == "knowledge.md" && entry.text.contains("INC-12345")
        }));
        assert!(
            request
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert!(request.text.contains("INC-12345"));
        assert!(
            request.text.len() < 20_000,
            "whole corpus body must stay out"
        );
        assert!(!request.text.contains("materials/"));
        let prompt_context = crate::session_log::list()
            .into_iter()
            .rev()
            .filter_map(|entry| entry.prompt_context)
            .find(|context| context.conversation_id == project.id)
            .unwrap();
        assert!(prompt_context.fresh_session);
        assert_eq!(prompt_context.conversation_history_est_tokens, 0);
        assert_eq!(prompt_context.raw_attachment_count, 0);
        assert_eq!(
            prompt_context.rag_attachment_count,
            knowledge.entries.len(),
            "local RAG attachment telemetry must equal serialized entries"
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.selected_evidence_count,
            Some(knowledge.entries.len())
        );
        drop(sent);
    }

    /// Production-faithful regression: an exhaustive result persists only its
    /// locally grounded lexical sources, and a date-only "de esas" follow-up
    /// searches that exact durable subset rather than the wider corpus.
    #[test]
    fn exhaustive_result_set_scopes_date_followup_and_survives_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let project = {
            let (fake, _calls) = ready_engine();
            let state = recording_app(
                &base,
                RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
            );
            let project = state.create_project("ExhaustiveResultSet").unwrap();
            let first = add_file(
                &state,
                &base,
                &project.id,
                "a-julio.md",
                "En julio se trabajó presente continuo.\n",
            );
            let second = add_file(
                &state,
                &base,
                &project.id,
                "b-agosto.md",
                "En agosto se trabajó presente continuo.\n",
            );
            // This is deliberately in August but not in Q1's result set.
            add_file(
                &state,
                &base,
                &project.id,
                "c-agosto-ajena.md",
                "En agosto se trabajó pasado simple.\n",
            );
            let q1 = state
                .send_message(
                    &project.id,
                    "¿Qué reuniones mencionan presente continuo?",
                    &[],
                )
                .unwrap();
            assert_eq!(q1.status, "completed");
            let disk =
                fs::read_to_string(base.join("projects").join(&project.id).join("project.json"))
                    .unwrap();
            let json: serde_json::Value = serde_json::from_str(&disk).unwrap();
            let user = json["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| message["messageId"] == q1.turn_id.clone().unwrap())
                .unwrap();
            assert_eq!(user["turnReferent"]["kind"], "materialSet");
            assert_eq!(user["turnReferent"]["producedBy"], "exhaustive_result");
            let ids = user["turnReferent"]["materialIds"].as_array().unwrap();
            assert_eq!(ids.len(), 2);
            assert!(ids.iter().any(|id| id.as_str() == Some(first.as_str())));
            assert!(ids.iter().any(|id| id.as_str() == Some(second.as_str())));
            project
        };

        // A completely fresh AppState proves referent durability and that this
        // continuity is independent of a provider transcript or re-embedding.
        let (fake, calls) = ready_engine();
        let state = recording_app(&base, RecordingEngine(fake, calls));
        let before_embeddings = KnowledgeStore::open(
            base.join("projects").join(&project.id),
            &ProjectId::parse(&project.id).unwrap(),
        )
        .unwrap()
        .corpus_stats()
        .unwrap()
        .embeddings_ready;
        let q2 = state
            .send_message(&project.id, "¿Y cuáles de esas fueron en agosto?", &[])
            .unwrap();
        assert_eq!(q2.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(2));
        assert_eq!(
            metrics.materials_inspected,
            Some(2),
            "must not widen to all materials"
        );
        assert_eq!(metrics.lexical_hits, Some(1));
        let after_embeddings = KnowledgeStore::open(
            base.join("projects").join(&project.id),
            &ProjectId::parse(&project.id).unwrap(),
        )
        .unwrap()
        .corpus_stats()
        .unwrap()
        .embeddings_ready;
        assert_eq!(
            after_embeddings, before_embeddings,
            "follow-up must not re-embed"
        );
    }
    /// Case B + §14: "de esos, cuáles mencionan Kubernetes?" resolves the 50
    /// MaterialSet and runs CorpusExhaustive scoped to exactly those 50.
    #[test]
    fn scoped_exhaustive_followup_uses_the_exact_previous_material_set() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("ScopedExh").unwrap();
        for index in 0..50 {
            let body = if index % 10 == 0 {
                format!("Esta reunión {index} menciona Kubernetes y también OpenShift.\n")
            } else {
                format!("Contenido {index} sin el término buscado.\n")
            };
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("s-{index:02}.md"),
                &body,
            );
        }
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "de esos, cuáles mencionan Kubernetes?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(50));
        assert_eq!(metrics.base_intent.as_deref(), Some("exhaustive"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("scoped_exhaustive"));
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
        // The scope is the 50-material referent, not the whole corpus.
        assert_eq!(metrics.eligible_materials, Some(50));
        assert_eq!(metrics.materials_inspected, Some(50));
        // 5 of the 50 materials carry the needle.
        assert_eq!(metrics.lexical_hits, Some(5));
        assert!(metrics.selected_evidence_count.unwrap_or(0) > 0);
    }

    /// CASE A (the reported bug): an OLDER MaterialSet and a NEWER ThemeSet
    /// coexist, and "de esos, cuáles mencionan Kubernetes?" is a scoped
    /// presence/exhaustive action over materials. The resolver must select the
    /// MaterialSet (not the newer ThemeSet), inspect only those 50 material ids,
    /// and report exhaustive coverage over exactly that scope.
    #[test]
    fn scoped_presence_prefers_material_set_over_newer_theme_set() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("Ambiguous").unwrap();
        for index in 0..50 {
            let mut body =
                format!("Reunión {index}. Se trabajó el presente continuo y Google Workspace.\n");
            if index % 10 == 0 {
                body.push_str("También se mencionó Kubernetes para el despliegue del clúster.\n");
            }
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("a-{index:02}.md"),
                &body,
            );
        }
        // TURN 1 (older): inventory -> durable MaterialSet(50).
        let inventory = state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        assert_eq!(inventory.status, "completed");
        let inventory_turn_id = inventory.turn_id.clone().expect("inventory turn id");

        // TURN 2 (newer): thematic -> durable ThemeSet.
        let thematic = state
            .send_message(&project.id, "¿Cuáles son los temas recurrentes?", &[])
            .unwrap();
        assert_eq!(thematic.status, "completed");
        let thematic_metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(thematic_metrics.retrieval_mode.as_deref(), Some("thematic"));
        let thematic_turn_id = thematic.turn_id.clone().expect("thematic turn id");
        assert_ne!(inventory_turn_id, thematic_turn_id);

        // TURN 3: the ambiguous neutral cue + presence action. It must resolve
        // the OLDER MaterialSet, never the NEWER ThemeSet.
        let run = state
            .send_message(&project.id, "de esos, ¿cuáles mencionan Kubernetes?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(50));
        assert_eq!(
            metrics.origin_turn_id.as_deref(),
            Some(inventory_turn_id.as_str()),
            "the follow-up must bind the OLDER MaterialSet turn, not the newer thematic turn"
        );
        assert_eq!(metrics.base_intent.as_deref(), Some("exhaustive"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("scoped_exhaustive"));
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
        assert_eq!(metrics.eligible_materials, Some(50));
        assert_eq!(metrics.materials_inspected, Some(50));
        assert_eq!(metrics.lexical_hits, Some(5));
        assert_eq!(metrics.exhaustive_coverage.as_deref(), Some("complete"));
    }

    /// CASE F: only a ThemeSet exists and the query asks for a scoped material
    /// presence scan. There is no compatible MaterialSet, so the resolver must
    /// NOT reinterpret theme names as files and must NOT force a ThemeSet bind.
    /// Ordinary exhaustive routing (or clarification) continues instead.
    #[test]
    fn theme_set_only_never_binds_a_scoped_material_presence_scan() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("ThemeOnly").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "t-a.md",
            "Se trabajó el presente continuo y Google Workspace.\n",
        );
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "t-b.md",
            "El presente continuo apareció en varias actividades. Google Workspace se usó para compartir.\n",
        );
        let thematic = state
            .send_message(&project.id, "¿Cuáles son los temas recurrentes?", &[])
            .unwrap();
        assert_eq!(thematic.status, "completed");

        // No inventory MaterialSet was ever persisted; only the ThemeSet exists.
        let run = state
            .send_message(&project.id, "de esos, ¿cuáles mencionan Kubernetes?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_ne!(
            metrics.referent_type.as_deref(),
            Some("theme_set"),
            "a scoped material presence scan must never bind a ThemeSet"
        );
        // No compatible MaterialSet exists, so the resolver does not bind a
        // ThemeSet and ordinary routing continues (never a contextual follow-up,
        // never a forced/reinterpreted scope).
        assert_ne!(metrics.contextual_followup, Some(true));
        assert_ne!(metrics.turn_kind.as_deref(), Some("per_theme_detail"));
        assert_eq!(
            metrics.retrieval_mode, None,
            "a dangling referential cue with no compatible MaterialSet falls back to ordinary chat"
        );
    }

    /// PERSISTENCE/RESTART: both a MaterialSet and a ThemeSet persist, the
    /// project reopens from disk, and the ambiguous follow-up still selects the
    /// MaterialSet by action compatibility (no referent reconstruction from
    /// model text, no resolution-triggered re-embedding).
    #[test]
    fn ambiguous_followup_survives_restart_and_selects_material_set() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let (project, inventory_turn_id) = {
            let (fake, _engine_calls) = ready_engine();
            let state = recording_app(
                &base,
                RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
            );
            let project = state.create_project("RestartAmbiguous").unwrap();
            for index in 0..50 {
                let mut body = format!(
                    "Reunión {index}. Se trabajó el presente continuo y Google Workspace.\n"
                );
                if index % 10 == 0 {
                    body.push_str("También se mencionó Kubernetes para el despliegue.\n");
                }
                add_file(
                    &state,
                    &base,
                    &project.id,
                    &format!("ra-{index:02}.md"),
                    &body,
                );
            }
            let inventory = state
                .send_message(&project.id, "listame los archivos", &[])
                .unwrap();
            let inventory_turn_id = inventory.turn_id.expect("inventory turn id");
            state
                .send_message(&project.id, "¿Cuáles son los temas recurrentes?", &[])
                .unwrap();
            (project, inventory_turn_id)
        };

        // Simulated restart: brand-new AppState over the same durable base.
        let (fake, engine_calls) = ready_engine();
        let state = recording_app(&base, RecordingEngine(fake, engine_calls.clone()));
        let activity_before = state.test_activity();

        let run = state
            .send_message(&project.id, "de esos, ¿cuáles mencionan Kubernetes?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(50));
        assert_eq!(metrics.base_intent.as_deref(), Some("exhaustive"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("scoped_exhaustive"));
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
        assert_eq!(metrics.eligible_materials, Some(50));
        assert_eq!(metrics.materials_inspected, Some(50));
        assert_eq!(metrics.lexical_hits, Some(5));

        // The referent is resolved from persisted structure, not reconstructed
        // from model text: origin_turn_id is the durable inventory turn persisted
        // BEFORE the restart.
        assert_eq!(
            metrics.origin_turn_id.as_deref(),
            Some(inventory_turn_id.as_str()),
            "resolution must reuse the persisted MaterialSet, not the newer ThemeSet nor model text"
        );

        // Resolution alone never re-ingests or re-embeds the corpus: the only
        // post-restart turn is the scoped exhaustive scan over already-READY
        // materials.
        let activity_after = state.test_activity();
        assert_eq!(
            activity_after.indexing, activity_before.indexing,
            "resolution must never re-ingest the corpus"
        );
        assert!(
            engine_calls.lock().unwrap().len() <= 1,
            "the follow-up must not re-run ingestion or invent referents"
        );
    }

    /// Case C + §13: a CorpusThematic follow-up reuses the EXACT ThemeSet;
    /// discovery is never re-run and the persisted themes are unchanged.
    #[test]
    fn thematic_followup_keeps_the_exact_original_theme_set() {
        let tmp = tempfile::tempdir().unwrap();
        let (fake, _engine_calls) = ready_engine();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(fake, Arc::new(Mutex::new(Vec::new()))),
        );
        let project = state.create_project("ThemeFollowUp").unwrap();
        // Two documents sharing recurring themes so CorpusThematic yields a set.
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "r-a.md",
            "En la reunión se trabajó el presente continuo con los alumnos. El presente continuo se practicó en ejercicios orales. También se habló de Google Workspace para organizar las clases.\n",
        );
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "r-b.md",
            "Volvimos sobre el presente continuo porque costaba. El presente continuo apareció en varias actividades. Google Workspace se usó para compartir materiales.\n",
        );
        let thematic = state
            .send_message(
                &project.id,
                "¿Cuáles son los temas principales que aparecen repetidamente en las 2 reuniones?",
                &[],
            )
            .unwrap();
        assert_eq!(thematic.status, "completed");

        // Capture the ThemeSet persisted by the thematic turn.
        let disk = fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&disk).unwrap();
        let user_messages: Vec<&serde_json::Value> = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .collect();
        let thematic_turn = user_messages.last().unwrap();
        assert_eq!(thematic_turn["turnReferent"]["kind"], "themeSet");
        let original_keys: Vec<String> = thematic_turn["turnReferent"]["themeKeys"]
            .as_array()
            .unwrap()
            .iter()
            .map(|key| key.as_str().unwrap().to_owned())
            .collect();
        assert!(!original_keys.is_empty());
        assert!(original_keys.contains(&"presente continuo".to_owned()));

        let followup = state
            .send_message(
                &project.id,
                "para cada uno de los temas recurrentes que acabás de identificar, indicame por separado: 1. el tema; 2. las reuniones exactas donde aparece; 3. el nombre exacto de cada archivo que aporta evidencia.",
                &[],
            )
            .unwrap();
        assert_eq!(followup.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("theme_set"));
        assert_eq!(metrics.referent_count, Some(original_keys.len()));
        assert_eq!(metrics.base_intent.as_deref(), Some("thematic"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("per_theme_detail"));
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));

        // The ThemeSet persisted by turn 1 is unchanged after the follow-up.
        let disk_after = fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json_after: serde_json::Value = serde_json::from_str(&disk_after).unwrap();
        let after_keys: Vec<String> = json_after["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|message| message["role"] == "user")
            .map(|turn| turn["turnReferent"]["themeKeys"].clone())
            .map(|keys| {
                keys.as_array()
                    .unwrap()
                    .iter()
                    .map(|key| key.as_str().unwrap().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(
            after_keys, original_keys,
            "the ThemeSet must never be replaced"
        );
    }

    /// Case L: English variants of the human scenario.
    #[test]
    fn english_followup_variants_resolve_to_per_item_summary() {
        let tmp = tempfile::tempdir().unwrap();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("English").unwrap();
        for index in 0..8 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("e-{index}.md"),
                "content\n",
            );
        }
        state
            .send_message(&project.id, "list all uploaded files", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "summarize each one", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(count_lines(&run.message.unwrap()), 8);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.turn_kind.as_deref(), Some("per_item_summary"));
        assert_eq!(metrics.referent_count, Some(8));
    }

    /// Case M: accent / no-accent variants resolve identically.
    #[test]
    fn accent_variants_resolve_identically() {
        let tmp = tempfile::tempdir().unwrap();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("Accents").unwrap();
        for index in 0..6 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("ac-{index}.md"),
                "contenido\n",
            );
        }
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        for query in [
            "resumime los archivos que acabás de listar",
            "resumime los archivos que acabas de listar",
            "resumime los archivos que listaste",
        ] {
            let run = state.send_message(&project.id, query, &[]).unwrap();
            assert_eq!(run.status, "completed");
            let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
            assert_eq!(
                metrics.turn_kind.as_deref(),
                Some("per_item_summary"),
                "{query}"
            );
            assert_eq!(metrics.referent_count, Some(6), "{query}");
        }
    }

    /// The per-item output honors the requested word limit deterministically.
    #[test]
    fn per_item_summary_word_limit_is_enforced() {
        let tmp = tempfile::tempdir().unwrap();
        let state = recording_app(
            tmp.path(),
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        // Fake returns 10 words per entry; a 5-word limit truncates.
        #[derive(Clone)]
        struct Wordy;
        impl RemoteSummarizer for Wordy {
            fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
                Ok(SummaryOutput {
                    text: request
                        .labels
                        .iter()
                        .map(|label| {
                            format!(
                                "{}: uno dos tres cuatro cinco seis siete ocho nueve diez\n",
                                label.label
                            )
                        })
                        .collect(),
                    model_id: Some("fake".into()),
                    provider_id: Some("fake".into()),
                    usage: SummaryUsage::default(),
                })
            }
        }
        state.set_per_item_summarizer(Wordy);
        let project = state.create_project("WordLimit").unwrap();
        add_file(&state, tmp.path(), &project.id, "w1.md", "contenido\n");
        add_file(&state, tmp.path(), &project.id, "w2.md", "contenido\n");
        state
            .send_message(&project.id, "listame los archivos", &[])
            .unwrap();
        let run = state
            .send_message(&project.id, "resumí cada uno en 5 palabras", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let message = run.message.unwrap();
        assert!(message.contains("uno dos tres cuatro cinco"));
        assert!(
            !message.contains("seis"),
            "word limit must be enforced: {message}"
        );
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

    /// Runs one turn through the exact packaged-UI staged seam used by
    /// `agent_send_staged`: `send_staged_message_persist` then
    /// `run_accepted_staged_turn`, which reaches
    /// `run_accepted_staged_turn_inner`.
    fn run_staged_turn(
        state: &AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
        project_id: &str,
        prompt: &str,
        staged_paths: &[String],
    ) -> crate::AgentRunView {
        let accepted = state
            .send_staged_message_persist(project_id, prompt, staged_paths, &[])
            .unwrap();
        state.run_accepted_staged_turn(accepted).unwrap()
    }

    /// Builds the state used by the staged precedence tests. The production
    /// build wires a real K6 backend; without one the `summarize_intent` gate
    /// is inert and the colliding phrases would never exercise the defect, so
    /// the gate points at a backend that is only invoked when routing is wrong.
    fn staged_state(
        base: &std::path::Path,
    ) -> AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
        let mut state = recording_app(
            base,
            RecordingEngine(FakeAgentEngine::new(), Arc::new(Mutex::new(Vec::new()))),
        );
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), base.join("oc-config"), 0);
        state.summarizer_backend = Some(Arc::new(backend));
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state
    }

    fn staged_material_paths(base: &std::path::Path, count: usize) -> Vec<String> {
        let corpus = base.join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        (0..count)
            .map(|index| {
                let path = corpus.join(format!("staged-{index:02}.md"));
                std::fs::write(
                    &path,
                    format!("Contenido del material {index} de la clase sobre gramática.\n"),
                )
                .unwrap();
                path.to_string_lossy().to_string()
            })
            .collect()
    }

    /// The blocking production-seam regression: in
    /// `run_accepted_staged_turn_inner`, a contextual per-item follow-up must
    /// beat the older K6 summary gate. "resumime los archivos que listaste"
    /// trips `detect_summary_intent` (its "los archivos" per-source marker
    /// yields `Project` with an empty composer), so pre-fix it routes to
    /// `send_summary_run`; post-fix it must land on PerItemSummary. This test
    /// fails against the pre-fix staged precedence (k6_calls > 0 and
    /// `turn_kind != per_item_summary`).
    #[test]
    fn staged_colliding_followup_phrase_routes_to_per_item_summary_not_k6() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let per_item_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: per_item_calls.clone(),
        });
        let project = state.create_project("StagedPerItem").unwrap();
        let paths = staged_material_paths(tmp.path(), 5);

        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(inventory.status, "completed");
        let k6_before = state.test_activity().k6_calls;
        let provider_calls_before = state.test_activity().provider_calls;

        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime los archivos que listaste",
            &[],
        );
        assert_eq!(run.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("material_set"));
        assert_eq!(metrics.referent_count, Some(5));
        assert_eq!(metrics.turn_kind.as_deref(), Some("per_item_summary"));
        assert_eq!(metrics.retrieval_mode, None);
        assert_eq!(metrics.local_mode.as_deref(), Some("per_item_summary"));
        assert_eq!(
            state.test_activity().k6_calls,
            k6_before,
            "K6 must never run"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            provider_calls_before,
            "the per-item path must not run a normal chat provider call"
        );
        let message = run.message.expect("per-item answer");
        assert_eq!(
            count_lines(&message),
            5,
            "exact N outputs, no cardinality shrink: {message}"
        );
        for index in 0..5 {
            let name = format!("staged-{index:02}.md");
            assert!(
                message.contains(&format!("{name}: Resumen de {name}")),
                "exact source association must survive: {name}"
            );
        }
        assert!(
            !message.contains("Fuentes:"),
            "per-item provenance is per item: {message}"
        );
        assert_eq!(
            *per_item_calls.lock().unwrap(),
            1,
            "5 materials at the 100-per-call default use exactly 1 aggregate per-item call"
        );
        assert_eq!(metrics.remote_calls, Some(1));
    }

    /// Dispatch-equivalence regression (architecture §7/§13): the formerly
    /// divergent routing seams must now converge on the same normalized
    /// `ClassifierDecision` for equivalent context. `send_message` and the
    /// accepted-staged pipeline (`agent_send_staged`) are the two Rust-testable
    /// seams; the Tauri `agent_send` command now delegates to the same
    /// `dispatch_message_run` seam by construction.
    ///
    /// The direct and staged seams run against separate projects with
    /// equivalent context (two READY current-turn materials each), so the K6
    /// summary-operation lifecycle of one run cannot perturb the other.
    #[test]
    fn dispatch_entry_points_reach_the_same_normalized_decision() {
        for prompt in [
            "Haceme un resumen general de estos archivos.",
            "Haceme un resumen general de estos archivos, destacando los temas principales.",
            "Resumime cada archivo por separado.",
        ] {
            let _session_log_guard = crate::session_log::test_guard();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_theme_state(tmp.path());
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });

            // Non-staged seam: two READY materials attached on `send_message`.
            let direct_project = state.create_project("DispatchEquivDirect").unwrap();
            let material_a = add_file(
                &state,
                tmp.path(),
                &direct_project.id,
                "equiv-a.md",
                "Contenido A sobre gramática.\n",
            );
            let material_b = add_file(
                &state,
                tmp.path(),
                &direct_project.id,
                "equiv-b.md",
                "Contenido B sobre vocabulario.\n",
            );
            let direct = state
                .send_message(
                    &direct_project.id,
                    prompt,
                    &[material_a.clone(), material_b.clone()],
                )
                .unwrap();
            assert_eq!(direct.status, "completed", "direct run for {prompt:?}");
            let direct_decision = state.last_routing_decision().expect("direct decision");

            // Accepted-staged seam: two staged READY materials on the same prompt.
            let staged_project = state.create_project("DispatchEquivStaged").unwrap();
            let staged_paths = staged_theme_files(tmp.path());
            let staged = run_staged_turn(&state, &staged_project.id, prompt, &staged_paths);
            assert_eq!(staged.status, "completed", "staged run for {prompt:?}");
            let staged_decision = state.last_routing_decision().expect("staged decision");

            assert_eq!(
                direct_decision.decision.intent, staged_decision.decision.intent,
                "intent must converge for {prompt:?}"
            );
            assert_eq!(
                direct_decision.decision.reason_code, staged_decision.decision.reason_code,
                "reason must converge for {prompt:?}"
            );
        }
    }

    /// The remaining supported per-item wordings must ALL beat the K6 gate on
    /// the same staged seam: the previously-safe neutral wording, the "cada
    /// archivo" per-source collision, and the English "each file" collision.
    #[test]
    fn staged_per_item_wordings_all_route_to_per_item_summary_not_k6() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("StagedWordings").unwrap();
        let paths = staged_material_paths(tmp.path(), 5);
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(inventory.status, "completed");
        let k6_before = state.test_activity().k6_calls;

        for query in [
            "haceme un resumen de no más de 20 palabras por cada uno",
            "resumí cada archivo",
            "summarize each file",
        ] {
            let run = run_staged_turn(&state, &project.id, query, &[]);
            assert_eq!(run.status, "completed", "{query}");
            let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
            assert_eq!(
                metrics.turn_kind.as_deref(),
                Some("per_item_summary"),
                "{query}"
            );
            assert_eq!(metrics.referent_count, Some(5), "{query}");
            assert_eq!(metrics.retrieval_mode, None, "{query}");
            assert_eq!(
                metrics.local_mode.as_deref(),
                Some("per_item_summary"),
                "{query}"
            );
            assert_eq!(
                count_lines(run.message.as_deref().unwrap()),
                5,
                "{query}: exact N outputs, no cardinality shrink"
            );
        }
        assert_eq!(
            state.test_activity().k6_calls,
            k6_before,
            "K6 must never run for any per-item wording"
        );
    }

    /// Builds a normalized route carrying exactly the given semantic intent,
    /// decoupled from any prompt wording (retrieval-authority injection).
    fn normalized_route(
        intent: crate::intent::Intent,
        reason: crate::intent::ReasonCode,
    ) -> crate::intent::BoundRoute {
        crate::intent::BoundRoute {
            decision: crate::intent::ClassifierDecision {
                intent,
                modifiers: Vec::new(),
                confidence: 1.0,
                reason_code: reason,
                provenance: crate::classifier::ClassifierProvenance::DeterministicBypass,
            },
            followup: None,
            creation_request: None,
            summary_depth: None,
        }
    }

    /// `dispatch_message_run` must never dispatch an unapplied route. When it is
    /// handed inputs with `route == None`, it resolves AND applies the route
    /// (follow-up scope, creation context, Knowledge preparation) before the
    /// dispatch step, so a route can never reach an engine partially bound.
    #[test]
    fn dispatch_message_run_binds_and_applies_a_missing_route_before_dispatch() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let project = state.create_project("DispatchHardening").unwrap();
        for index in 0..3 {
            add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("nota-{index}.md"),
                "Contenido del material sobre gramática.\n",
            );
        }
        // Persist the user turn (which normally resolves AND applies the route),
        // then strip the routing so the dispatch seam must resolve + apply it.
        let mut inputs = state
            .send_message_persist(&project.id, "listame todos los archivos", &[])
            .unwrap();
        inputs.route = None;
        inputs.knowledge = None;
        inputs.knowledge_metrics = None;
        inputs.creation = None;
        inputs.pending_referent = None;
        assert!(inputs.route.is_none());

        let run = state.dispatch_message_run(inputs).unwrap();
        assert_eq!(run.status, "completed");
        let decision = state
            .last_routing_decision()
            .expect("the dispatch seam records the resolved route");
        assert_eq!(
            decision.decision.intent,
            crate::intent::Intent::KnowledgeInventory,
            "the dispatch seam must resolve the missing route"
        );
        assert!(
            run.message
                .as_deref()
                .unwrap()
                .contains("Tenés 3 materiales"),
            "the applied route must prepare the inventory context, not dispatch unbound: {}",
            run.message.as_deref().unwrap()
        );
    }

    /// The contextual follow-up binding has a single authoritative owner: the
    /// bound route. `AgentRunInputs` carries no separate follow-up field, so the
    /// two can never diverge.
    #[test]
    fn followup_binding_has_a_single_authoritative_owner_on_the_route() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let project = state.create_project("FollowupOwner").unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(inventory.status, "completed");

        let accepted = state
            .send_staged_message_persist(&project.id, "resumí cada uno", &[], &[])
            .unwrap();
        let mut inputs = accepted.inputs;
        let route = state.resolve_route(
            &inputs.project_id,
            &inputs.prompt,
            &inputs.selected_material_ids,
            inputs.model.as_ref(),
        );
        assert!(route.followup.is_some(), "the follow-up binds on the route");
        state.apply_route(&mut inputs, &route).unwrap();
        inputs.route = Some(route);

        // The per-item follow-up is observable only through the bound route.
        assert!(inputs.is_per_item_followup());
        assert_eq!(
            inputs
                .route
                .as_ref()
                .unwrap()
                .followup
                .as_ref()
                .unwrap()
                .action,
            crate::referent::FollowUpAction::PerItemSummary,
            "the route is the single source of the follow-up action"
        );
    }

    /// Retrieval authority: once a normalized route is bound, the Knowledge
    /// preparation engine follows the route's intent, never the prompt wording.
    /// This mirrors the K6 `k6_consumes_normalized_route_not_prompt_wording`
    /// test for the retrieval engines.
    #[test]
    fn retrieval_preparation_follows_the_normalized_route_not_prompt_wording() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let project = state.create_project("RetrievalAuthority").unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(inventory.status, "completed");

        // A) CorpusExhaustive wins even though the wording is an ordinary open
        //    question that would classify as NormalSemantic.
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(
                &project.id,
                "¿Qué dijo Delfina sobre los horarios?",
                &[],
            )
            .unwrap();
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::CorpusExhaustive,
                    crate::intent::ReasonCode::PresenceQuery,
                ),
            )
            .unwrap();
        assert_eq!(
            inputs
                .knowledge_metrics
                .as_ref()
                .and_then(|m| m.retrieval_mode.clone()),
            Some("exhaustive".to_owned()),
            "A: exhaustive prep must follow the route, not the open-question wording"
        );

        // B) NormalSemantic wins even though the wording carries a presence
        //    needle that would classify as CorpusExhaustive.
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(
                &project.id,
                "¿En qué reuniones se habló de Kubernetes?",
                &[],
            )
            .unwrap();
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::NormalSemantic,
                    crate::intent::ReasonCode::SemanticQuestion,
                ),
            )
            .unwrap();
        assert_eq!(
            inputs
                .knowledge_metrics
                .as_ref()
                .and_then(|m| m.retrieval_mode.clone()),
            Some("normal".to_owned()),
            "B: normal prep must follow the route, not the presence wording"
        );

        // C) KnowledgeInventory uses inventory preparation (local answer, no
        //    retrieval mode) even though a fresh classification could differ.
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(&project.id, "listame todos los archivos", &[])
            .unwrap();
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::KnowledgeInventory,
                    crate::intent::ReasonCode::InventoryRequest,
                ),
            )
            .unwrap();
        assert_eq!(
            inputs
                .knowledge_metrics
                .as_ref()
                .and_then(|m| m.local_mode.clone()),
            Some("inventory".to_owned()),
            "C: inventory prep must follow the route"
        );
        assert_eq!(
            inputs
                .knowledge_metrics
                .as_ref()
                .and_then(|m| m.retrieval_mode.clone()),
            None,
            "C: inventory must never assign a retrieval mode"
        );
        assert!(
            inputs
                .knowledge
                .as_ref()
                .and_then(|k| k.local_answer.clone())
                .is_some_and(|answer| answer.contains("Tenés 3 materiales")),
            "C: inventory prep produced a local answer"
        );

        // D) CorpusThematic uses thematic preparation even though the wording is
        //    a whole-corpus summary request; the corpus has >= 2 READY documents
        //    so the structural readiness fallback does not apply.
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(
                &project.id,
                "Resumime todos los archivos.",
                &[],
            )
            .unwrap();
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::CorpusThematic,
                    crate::intent::ReasonCode::ThematicQuery,
                ),
            )
            .unwrap();
        assert_eq!(
            inputs
                .knowledge_metrics
                .as_ref()
                .and_then(|m| m.retrieval_mode.clone()),
            Some("thematic".to_owned()),
            "D: thematic prep must follow the route, not the summary wording"
        );

        // E) OrdinaryChat is a hard no: it must not retrieve and must drop any
        //    leftover Knowledge context already sitting on the inputs.
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(
                &project.id,
                "¿Qué dijo Delfina sobre los horarios?",
                &[],
            )
            .unwrap();
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::NormalSemantic,
                    crate::intent::ReasonCode::SemanticQuestion,
                ),
            )
            .unwrap();
        assert!(
            inputs.knowledge.is_some(),
            "E setup: NormalSemantic prepares Knowledge"
        );
        let retrieval_after_semantic = state.test_activity().retrieval;
        state
            .apply_route(
                &mut inputs,
                &normalized_route(
                    crate::intent::Intent::OrdinaryChat,
                    crate::intent::ReasonCode::OrdinaryChatFallback,
                ),
            )
            .unwrap();
        assert!(
            inputs.knowledge.is_none(),
            "E: OrdinaryChat strips Knowledge"
        );
        assert!(
            inputs.knowledge_metrics.is_none(),
            "E: OrdinaryChat strips Knowledge metrics"
        );
        assert_eq!(
            state.test_activity().retrieval,
            retrieval_after_semantic,
            "E: OrdinaryChat must not run retrieval"
        );
    }

    /// Counting classifier that always returns a fixed decision, used to prove
    /// invocation count and that the classifier result controls routing.
    struct CountingClassifier {
        calls: Arc<Mutex<usize>>,
        intent: crate::intent::Intent,
        reason: crate::intent::ReasonCode,
    }
    impl crate::classifier::IntentClassifier for CountingClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<crate::intent::ClassifierDecision, crate::classifier::IntentClassificationError>
        {
            *self.calls.lock().unwrap() += 1;
            Ok(crate::intent::ClassifierDecision {
                intent: self.intent,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: self.reason,
                provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    /// Per-call intent script so Knowledge → OrdinaryChat alternation is
    /// classifier-authored, not inferred from prompt wording.
    struct ScriptedClassifier {
        calls: Arc<Mutex<usize>>,
        intents: Vec<crate::intent::Intent>,
    }
    impl crate::classifier::IntentClassifier for ScriptedClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<crate::intent::ClassifierDecision, crate::classifier::IntentClassificationError>
        {
            let mut calls = self.calls.lock().unwrap();
            let intent = self
                .intents
                .get(*calls)
                .copied()
                .unwrap_or(crate::intent::Intent::OrdinaryChat);
            *calls += 1;
            Ok(crate::intent::ClassifierDecision {
                intent,
                modifiers: Vec::new(),
                confidence: 0.9,
                reason_code: crate::intent::ReasonCode::SemanticClassifier,
                provenance: crate::classifier::ClassifierProvenance::SemanticSuccess,
            })
        }
    }

    fn assert_ordinary_chat_no_knowledge_processing<E: AgentEngine>(
        state: &AppState<E, FakeTunnel, FakeProviderConnector, FakeRestarter>,
        project_id: &str,
        retrieval_before: usize,
        embedding_before: usize,
        provider_before: usize,
        k6_before: usize,
    ) {
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::OrdinaryChat
        );
        let activity = state.test_activity();
        assert_eq!(
            activity.retrieval, retrieval_before,
            "OrdinaryChat must not retrieve"
        );
        assert_eq!(
            activity.embedding_inference, embedding_before,
            "OrdinaryChat must not emit Knowledge query embeddings"
        );
        assert_eq!(
            activity.provider_calls,
            provider_before + 1,
            "OrdinaryChat is one chat inference"
        );
        assert_eq!(activity.k6_calls, k6_before, "OrdinaryChat is not K6");
        let metrics = state.last_turn_metrics(project_id).unwrap().unwrap();
        assert_eq!(
            metrics.retrieval_mode, None,
            "OrdinaryChat must not set retrieval_mode=normal"
        );
        let knowledge = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.contains("intent=ordinary_chat"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            knowledge.iter().any(|line| {
                line.contains("knowledge_used=false")
                    && line.contains("retrieval_mode=none")
                    && line.contains("query_embeddings=0")
            }),
            "OrdinaryChat knowledge telemetry: {knowledge:?}"
        );
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing.iter().any(|line| {
                line.contains("intent=ordinary_chat") && line.contains("knowledge_used=false")
            }),
            "OrdinaryChat routing telemetry: {routing:?}"
        );
    }

    /// Classifier that always fails, to prove deterministic fallback.
    struct ErroringClassifier;
    impl crate::classifier::IntentClassifier for ErroringClassifier {
        fn classify(
            &self,
            _input: &crate::classifier::ClassifierInput,
        ) -> Result<crate::intent::ClassifierDecision, crate::classifier::IntentClassificationError>
        {
            Err(crate::classifier::IntentClassificationError::new(
                crate::classifier::ClassifierFallbackReason::Unavailable,
            ))
        }
    }

    /// A Knowledge-relevant turn must invoke the semantic classifier exactly
    /// once, and the classifier result must control the normalized intent (the
    /// deterministic presence detector would have chosen CorpusExhaustive here).
    #[test]
    fn knowledge_turn_invokes_the_semantic_classifier_once_and_it_controls_intent() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: calls.clone(),
            intent: crate::intent::Intent::NormalSemantic,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let project = state.create_project("ClassSeamAB").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "nota.md",
            "Contenido sobre gramática.\n",
        );
        // The deterministic presence detector would route this needle to
        // CorpusExhaustive; the classifier forces NormalSemantic.
        let run = state
            .send_message(
                &project.id,
                "¿En qué reuniones se habló de Kubernetes?",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            *calls.lock().unwrap(),
            1,
            "classifier must be invoked exactly once"
        );
        let decision = state.last_routing_decision().expect("route");
        assert_eq!(
            decision.decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        assert_eq!(
            decision.decision.reason_code,
            crate::intent::ReasonCode::SemanticClassifier
        );
        // Deterministic preparation still follows the classifier's intent.
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
    }

    /// Classifier failure must never break routing: it falls back to the
    /// deterministic adapter.
    #[test]
    fn classifier_failure_falls_back_to_deterministic_routing() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_test_classifier(ErroringClassifier);
        let project = state.create_project("ClassSeamD").unwrap();
        add_file(&state, tmp.path(), &project.id, "a.md", "contenido A.\n");
        add_file(&state, tmp.path(), &project.id, "b.md", "contenido B.\n");
        let run = state
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let decision = state.last_routing_decision().expect("route");
        assert_eq!(
            decision.decision.intent,
            crate::intent::Intent::CorpusThematic
        );
        assert_eq!(
            decision.decision.reason_code,
            crate::intent::ReasonCode::ThematicQuery
        );
    }

    /// A semantic classifier result can never override a deterministic follow-up
    /// pre-gate: "resumí cada uno" resolves the prior MaterialSet before any
    /// classification, so the classifier is not even invoked.
    #[test]
    fn semantic_classifier_cannot_override_the_deterministic_followup_pre_gate() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("ClassSeamG").unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        // The inventory turn creates the MaterialSet referent (deterministic,
        // no classifier backend configured yet).
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(inventory.status, "completed");
        // Arm a classifier that would (wrongly) return CorpusExhaustive for
        // "resumí cada uno"; the follow-up pre-gate must win without calling it.
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: calls.clone(),
            intent: crate::intent::Intent::CorpusExhaustive,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let run = run_staged_turn(&state, &project.id, "resumí cada uno", &[]);
        assert_eq!(run.status, "completed");
        assert_eq!(
            *calls.lock().unwrap(),
            0,
            "the follow-up pre-gate must skip the classifier"
        );
        let decision = state.last_routing_decision().expect("route");
        assert_eq!(
            decision.decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        assert_eq!(
            decision.decision.reason_code,
            crate::intent::ReasonCode::ContextualFollowUp
        );
    }

    /// A semantic KnowledgeInventory decision must execute locally even when the
    /// prompt wording is in a language the legacy ES/EN keyword parser does not
    /// recognize. The classifier result is authoritative; execution must not
    /// re-derive the language. Injected without the classify trigger gate so this
    /// test covers execution once the semantic classifier has been consulted
    /// (Phase 1 no longer auto-invokes it merely because Knowledge exists).
    #[test]
    fn semantic_knowledge_inventory_multilingual_executes_locally_without_internal_error() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::KnowledgeInventory,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let project = state.create_project("MultilingualInv").unwrap();
        for (name, body) in [
            ("alfa.md", "Contenido A.\n"),
            ("bravo.md", "Contenido B.\n"),
        ] {
            add_file(&state, tmp.path(), &project.id, name, body);
        }
        let prompts = [
            "Listame todos los archivos registrados.",
            "List all registered files.",
            "Liste todos os arquivos registrados.",
            "Liste tous les fichiers enregistrés.",
            "Liste alle registrierten Dateien auf.",
            "Elenca tutti i file registrati.",
            "登録されているすべてのファイルを一覧表示してください",
            "列出所有已注册的文件",
            "اعرض جميع الملفات المسجلة",
        ];
        for prompt in prompts {
            let provider_calls_before = state.test_activity().provider_calls;
            let run = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(
                run.status, "completed",
                "prompt {prompt:?} must not Internal-error"
            );
            assert!(
                run.message
                    .as_deref()
                    .unwrap()
                    .contains("materiales registrados"),
                "prompt {prompt:?} must produce the local inventory answer: {}",
                run.message.as_deref().unwrap()
            );
            assert_eq!(
                state.test_activity().provider_calls,
                provider_calls_before,
                "prompt {prompt:?} must make zero final answer-provider calls"
            );
        }
    }

    /// Case A: no Knowledge index. "Hola" is structurally ordinary chat and
    /// must not consult the remote semantic classifier.
    #[test]
    fn ordinary_chat_without_knowledge_does_not_invoke_remote_classifier() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::CorpusExhaustive,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("NoKnowHola").unwrap();
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let run = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            *calls.lock().unwrap(),
            0,
            "without Knowledge the semantic classifier must be skipped"
        );
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing.iter().any(|line| {
                line.contains("knowledge_available=false")
                    && line.contains("classifier_invoked=false")
                    && line.contains("intent=ordinary_chat")
            }),
            "expected structural bypass: {routing:?}"
        );
    }

    /// Persisted Knowledge plus a trivial chat turn ("Hola") may consult the
    /// semantic classifier; if it returns OrdinaryChat there is no retrieval.
    #[test]
    fn knowledge_plus_ordinary_chat_classifier_can_return_ordinary_chat() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::OrdinaryChat,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("HollowChat").unwrap();
        add_file(&state, tmp.path(), &project.id, "a.md", "Contenido A.\n");
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let run = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing.iter().any(|line| {
                line.contains("knowledge_available=true")
                    && line.contains("knowledge_needed_for_turn=false")
                    && line.contains("knowledge_used=false")
                    && line.contains("classifier_invoked=true")
                    && line.contains("intent=ordinary_chat")
            }),
            "classifier may return OrdinaryChat while Knowledge is available: {routing:?}"
        );
    }

    #[test]
    fn general_open_question_is_not_locally_forced_to_knowledge() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::OrdinaryChat,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("OpenQ").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes.\n",
        );
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let run = state
            .send_message(&project.id, "¿Qué es Kubernetes?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
    }

    #[test]
    fn ordinary_chat_uses_conversational_session_without_knowledge_context() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: Arc::new(Mutex::new(0)),
                intent: crate::intent::Intent::OrdinaryChat,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("OrdinarySession").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes.\n",
        );
        let cached = engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        assert_eq!(cached.id, "cached-project-session");
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state
            .send_message(&project.id, "¿Qué es Kubernetes?", &[])
            .unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 0);
        let sent = engine.sent.lock().unwrap();
        let request = sent.last().unwrap();
        assert_eq!(request.session_id, "cached-project-session");
        assert!(request.knowledge.is_none());
        assert!(
            !request
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert!(!request.text.contains("Hoy hablamos de Kubernetes"));
        let prompt_context = crate::session_log::list()
            .into_iter()
            .rev()
            .filter_map(|entry| entry.prompt_context)
            .find(|context| context.conversation_id == project.id)
            .unwrap();
        assert!(!prompt_context.fresh_session);
        assert_eq!(prompt_context.rag_attachment_count, 0);
        assert_eq!(prompt_context.knowledge_context_est_tokens, 0);
    }

    #[test]
    fn knowledge_then_ordinary_chat_does_not_reuse_knowledge_processing() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: calls.clone(),
                intents: vec![
                    crate::intent::Intent::CorpusExhaustive,
                    crate::intent::Intent::OrdinaryChat,
                ],
            },
        ));
        let project = state.create_project("AltKnowChat").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Notas de Kubernetes y Docker en la reunión.\n",
        );

        let knowledge = state
            .send_message(
                &project.id,
                "¿Qué dijeron las reuniones sobre Kubernetes?",
                &[],
            )
            .unwrap();
        assert_eq!(knowledge.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::CorpusExhaustive
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("exhaustive")
        );
        assert_eq!(*calls.lock().unwrap(), 1);

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let ordinary = state
            .send_message(&project.id, "Explicame Docker en general.", &[])
            .unwrap();
        assert_eq!(ordinary.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
    }

    #[test]
    fn post_attachment_knowledge_then_ordinary_chat_does_not_carry_materials() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: calls.clone(),
                intents: vec![
                    crate::intent::Intent::NormalSemantic,
                    crate::intent::Intent::OrdinaryChat,
                ],
            },
        ));
        let project = state.create_project("PostAttach").unwrap();
        let mid = add_file(
            &state,
            tmp.path(),
            &project.id,
            "doc.md",
            "Documento adjunto sobre Kubernetes.\n",
        );

        let knowledge = state
            .send_message(
                &project.id,
                "Resumime estos documentos.",
                std::slice::from_ref(&mid),
            )
            .unwrap();
        assert_eq!(knowledge.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("normal")
        );
        let first = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(first.knowledge.is_some());
        assert_eq!(
            first
                .knowledge
                .as_ref()
                .and_then(|context| context.retrieval_mode.as_deref()),
            Some("normal")
        );
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 1);

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let thanks = state.send_message(&project.id, "Gracias.", &[]).unwrap();
        assert_eq!(thanks.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 2);
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 1);
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(second.session_id, "cached-project-session");
        assert!(second.knowledge.is_none());
        assert!(
            !second
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert!(!second.text.contains("Documento adjunto sobre Kubernetes"));
        assert!(!second.text.contains("materials/"));
    }

    #[test]
    fn normal_semantic_followup_uses_ephemeral_session_and_bounded_educai_history() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) =
            isolation_knowledge_state(&tmp, crate::intent::Intent::NormalSemantic);
        let project = state.create_project("NsFollowup").unwrap();
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "El incidente INC-12345 fue cerrado en Kubernetes y se relacionó con OpenShift.\n",
        );

        let provider_before = state.test_activity().provider_calls;
        let first = state.send_message(&project.id, "INC-12345", &[]).unwrap();
        assert_eq!(first.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        let first_req = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(first_req.session_id.starts_with("fresh-normal-"));
        assert!(
            first_req
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert!(!first_req.text.contains("<conversation_context>"));
        assert_eq!(state.test_activity().provider_calls, provider_before + 1);

        let second = state
            .send_message(&project.id, "¿Y cómo se relaciona eso con OpenShift?", &[])
            .unwrap();
        assert_eq!(second.status, "completed");
        assert_eq!(state.test_activity().provider_calls, provider_before + 2);
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 2);
        let second_req = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(second_req.session_id, "fresh-normal-2");
        assert!(second_req.text.contains("<conversation_context>"));
        assert!(second_req.text.contains("INC-12345"));
        assert!(
            second_req
                .text
                .contains("¿Y cómo se relaciona eso con OpenShift?")
        );
        assert!(!second_req.text.contains("HUGE-STALE"));
        let cached_sends: Vec<_> = engine
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.session_id == "cached-project-session")
            .cloned()
            .collect();
        assert!(
            cached_sends.iter().all(|request| {
                !request
                    .text
                    .contains("<knowledge_evidence trust=\"untrusted\">")
            }),
            "ephemeral Knowledge evidence must not land on the conversational session"
        );
        let prompt_context = crate::session_log::list()
            .into_iter()
            .rev()
            .filter_map(|entry| entry.prompt_context)
            .find(|context| context.conversation_id == project.id)
            .unwrap();
        assert!(prompt_context.fresh_session);
        assert!(prompt_context.conversation_history_est_tokens > 0);
    }

    #[test]
    fn exhaustive_and_thematic_synthesis_stay_off_the_conversational_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) =
            isolation_knowledge_state(&tmp, crate::intent::Intent::CorpusExhaustive);
        let project = state.create_project("ExhSession").unwrap();
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "La reunión cubrió Kubernetes, Docker y el calendario de agosto.\n",
        );
        let provider_before = state.test_activity().provider_calls;
        let exhaustive = state
            .send_message(&project.id, "¿Qué reuniones mencionan Kubernetes?", &[])
            .unwrap();
        assert_eq!(exhaustive.status, "completed");
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("exhaustive")
        );
        let exhaustive_req = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(exhaustive_req.session_id.starts_with("fresh-normal-"));
        assert!(
            exhaustive_req
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert_eq!(state.test_activity().provider_calls, provider_before + 1);

        drop(engine);
        let tmp_theme = tempfile::tempdir().unwrap();
        let (theme_engine, theme_state) =
            isolation_knowledge_state(&tmp_theme, crate::intent::Intent::CorpusThematic);
        let theme_project = theme_state.create_project("ThemeSession").unwrap();
        theme_engine
            .open_session(&AgentProject {
                project_id: theme_project.id.clone(),
                directory: tmp_theme
                    .path()
                    .join("projects")
                    .join(&theme_project.id)
                    .join("workspace"),
            })
            .unwrap();
        add_file(
            &theme_state,
            tmp_theme.path(),
            &theme_project.id,
            "r-a.md",
            "En la reunión se trabajó el presente continuo con los alumnos. El presente continuo se practicó en ejercicios orales.\n",
        );
        add_file(
            &theme_state,
            tmp_theme.path(),
            &theme_project.id,
            "r-b.md",
            "Volvimos sobre el presente continuo porque costaba. El presente continuo apareció en varias actividades.\n",
        );
        let theme_provider_before = theme_state.test_activity().provider_calls;
        let thematic = theme_state
            .send_message(&theme_project.id, "¿Cuáles son los temas recurrentes?", &[])
            .unwrap();
        assert_eq!(thematic.status, "completed");
        assert_eq!(
            theme_state
                .last_turn_metrics(&theme_project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("thematic")
        );
        let thematic_req = theme_engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(thematic_req.session_id.starts_with("fresh-normal-"));
        assert!(
            thematic_req
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );
        assert_eq!(
            theme_state.test_activity().provider_calls,
            theme_provider_before + 1
        );
        assert_eq!(
            theme_engine
                .sent
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.session_id == "cached-project-session")
                .count(),
            0
        );
    }

    #[test]
    fn ordinary_chat_then_knowledge_keeps_the_conversational_session_clean() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: Arc::new(Mutex::new(0)),
                intents: vec![
                    crate::intent::Intent::OrdinaryChat,
                    crate::intent::Intent::NormalSemantic,
                ],
            },
        ));
        let project = state.create_project("ChatThenKnow").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "El incidente INC-12345 fue cerrado en Kubernetes.\n",
        );
        let hello = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(hello.status, "completed");
        let first = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(first.session_id, "cached-project-session");
        assert!(first.knowledge.is_none());
        assert!(
            !first
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        );

        let knowledge = state.send_message(&project.id, "INC-12345", &[]).unwrap();
        assert_eq!(knowledge.status, "completed");
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 1);
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(second.session_id, "fresh-normal-1");
        assert!(second.knowledge.is_some());
        let cached = engine.open_session(&AgentProject {
            project_id: project.id.clone(),
            directory: tmp
                .path()
                .join("projects")
                .join(&project.id)
                .join("workspace"),
        });
        assert_eq!(cached.unwrap().id, "cached-project-session");
    }

    fn isolation_scripted(
        tmp: &tempfile::TempDir,
        intents: Vec<crate::intent::Intent>,
    ) -> (
        SessionIsolationEngine,
        AppState<SessionIsolationEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
    ) {
        let engine = SessionIsolationEngine::default();
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: Arc::new(Mutex::new(0)),
                intents,
            },
        ));
        (engine, state)
    }

    fn creation_material_body() -> String {
        let mut body = String::from(
            "# README\n\nMaterial de ejemplo para crear una presentación a partir del documento.\n\n",
        );
        for index in 0..20 {
            body.push_str(&format!(
                "## Parte {index}\n\nLa parte {index} agrega contenido suficiente para indexar el documento y usarlo como evidencia de creación.\n\n"
            ));
        }
        body
    }

    #[test]
    fn creation_with_knowledge_evidence_rotates_session_before_ordinary_chat() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(&tmp, vec![crate::intent::Intent::OrdinaryChat]);
        let project = state.create_project("CreateThenChat").unwrap();
        let material_id = add_file(
            &state,
            tmp.path(),
            &project.id,
            "README.md",
            &creation_material_body(),
        );
        let provider_before = state.test_activity().provider_calls;
        let creation = state
            .send_message(
                &project.id,
                "me podes armar una presentacion interactiva para presentar esto?",
                std::slice::from_ref(&material_id),
            )
            .unwrap();
        assert_eq!(creation.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::Creation
        );
        let creation_request = engine.sent.lock().unwrap().last().cloned().unwrap();
        let creation_session_id = creation_request.session_id.clone();
        assert!(
            creation_request
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">"),
            "{}",
            creation_request.text
        );
        assert!(creation_request.knowledge.is_some());
        let prompt_context = crate::session_log::list()
            .into_iter()
            .rev()
            .filter_map(|entry| entry.prompt_context)
            .find(|context| context.conversation_id == project.id)
            .unwrap();
        assert_eq!(prompt_context.session_role, "conversational");
        assert!(prompt_context.session_rotated);
        assert_eq!(
            prompt_context.rotation_reason,
            "creation_knowledge_evidence"
        );
        assert!(prompt_context.cache_invalidated);

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before_ordinary = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let ordinary = state.send_message(&project.id, "Gracias.", &[]).unwrap();
        assert_eq!(ordinary.status, "completed");
        let ordinary_request = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_ne!(creation_session_id, ordinary_request.session_id);
        assert!(ordinary_request.knowledge.is_none());
        assert!(!ordinary_request.text.contains("<knowledge_evidence"));
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before_ordinary,
            k6_before,
        );
        assert_eq!(
            state.test_activity().provider_calls,
            provider_before + 2,
            "rotation must not add provider calls"
        );
        let view = state.open_project(&project.id).unwrap();
        assert!(
            view.messages
                .iter()
                .any(|message| message.text.contains("presentacion")
                    || message.text.contains("presentación")
                    || message.text.contains("Gracias."))
        );
    }

    #[test]
    fn creation_with_knowledge_evidence_failure_rotates_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(&tmp, vec![crate::intent::Intent::OrdinaryChat]);
        let project = state.create_project("CreateFailChat").unwrap();
        let material_id = add_file(
            &state,
            tmp.path(),
            &project.id,
            "README.md",
            &creation_material_body(),
        );
        *engine.fail_next_send.lock().unwrap() = true;
        let failed = state
            .send_message(
                &project.id,
                "me podes armar una presentacion interactiva para presentar esto?",
                std::slice::from_ref(&material_id),
            )
            .unwrap();
        assert_eq!(failed.status, "failed");
        let tainted = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(tainted.text.contains("<knowledge_evidence"));
        let ordinary = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(ordinary.status, "completed");
        let next = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_ne!(tainted.session_id, next.session_id);
        assert!(next.knowledge.is_none());
        assert!(!next.text.contains("<knowledge_evidence"));
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::OrdinaryChat
        );
    }

    #[test]
    fn creation_without_knowledge_evidence_keeps_conversational_reuse() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(&tmp, vec![crate::intent::Intent::OrdinaryChat]);
        let project = state.create_project("CreatePlain").unwrap();
        state
            .send_message(&project.id, "crea una actividad interactiva", &[])
            .unwrap();
        let first = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(!first.text.contains("<knowledge_evidence"));
        assert!(first.knowledge.is_none());
        let prompt_context = crate::session_log::list()
            .into_iter()
            .rev()
            .filter_map(|entry| entry.prompt_context)
            .find(|context| context.conversation_id == project.id)
            .unwrap();
        assert!(!prompt_context.session_rotated);
        assert!(!prompt_context.cache_invalidated);
        state.send_message(&project.id, "Hola", &[]).unwrap();
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(first.session_id, second.session_id);
        assert!(second.knowledge.is_none());
    }

    #[test]
    fn ordinary_knowledge_ordinary_uses_clean_conversational_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::OrdinaryChat,
                crate::intent::Intent::NormalSemantic,
                crate::intent::Intent::OrdinaryChat,
            ],
        );
        let project = state.create_project("OkKo").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Las reuniones hablaron de Kubernetes INC-4242 y de Docker por separado.\n",
        );
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();

        let hello = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(hello.status, "completed");
        let first = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(first.session_id, "cached-project-session");
        assert!(first.knowledge.is_none());
        assert!(!first.text.contains("<knowledge_evidence"));
        assert!(!first.text.contains("<conversation_context>"));

        let provider_before_knowledge = state.test_activity().provider_calls;
        let knowledge = state.send_message(&project.id, "INC-4242", &[]).unwrap();
        assert_eq!(knowledge.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(second.session_id, "fresh-normal-1");
        assert_eq!(
            state.test_activity().provider_calls,
            provider_before_knowledge + 1
        );

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let ordinary = state
            .send_message(
                &project.id,
                "Gracias. Ahora explicame Docker en general.",
                &[],
            )
            .unwrap();
        assert_eq!(ordinary.status, "completed");
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let third = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(third.session_id, "cached-project-session");
        assert!(third.knowledge.is_none());
        assert!(!third.text.contains("<knowledge_evidence"));
        assert!(!third.text.contains("INC-4242"));
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 1);
        let cached_sends: Vec<_> = engine
            .sent
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.session_id == "cached-project-session")
            .cloned()
            .collect();
        assert!(cached_sends.iter().all(|request| {
            !request
                .text
                .contains("<knowledge_evidence trust=\"untrusted\">")
        }));
        let lifecycle = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[lifecycle]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            lifecycle.iter().any(|line| {
                line.contains("current_intent=ordinary_chat")
                    && line.contains("session_role=conversational")
                    && line.contains("knowledge_context_present=false")
            }),
            "{lifecycle:?}"
        );
        assert_eq!(state.test_activity().provider_calls, provider_before + 1);
    }

    #[test]
    fn knowledge_followup_uses_new_ephemeral_session_and_bounded_visible_history() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::NormalSemantic,
                crate::intent::Intent::NormalSemantic,
            ],
        );
        let project = state.create_project("FollowEphemeral").unwrap();
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Kubernetes se relacionó con OpenShift en la reunión INC-99.\n",
        );
        state.send_message(&project.id, "INC-99", &[]).unwrap();
        let first_id = engine
            .sent
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .session_id
            .clone();
        let provider_before = state.test_activity().provider_calls;
        state
            .send_message(&project.id, "¿Y cómo se relaciona eso con OpenShift?", &[])
            .unwrap();
        assert_eq!(state.test_activity().provider_calls, provider_before + 1);
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_ne!(second.session_id, first_id);
        assert!(second.session_id.starts_with("fresh-normal-"));
        assert!(second.text.contains("<conversation_context>"));
        assert!(second.text.contains("INC-99"));
        let context = second
            .text
            .split("<conversation_context>")
            .nth(1)
            .unwrap()
            .split("</conversation_context>")
            .next()
            .unwrap();
        assert!(!context.contains("¿Y cómo se relaciona eso con OpenShift?"));
        assert!(!context.contains("<knowledge_evidence"));
        assert_eq!(*engine.fresh_counter.lock().unwrap(), 2);
    }

    #[test]
    fn knowledge_then_distinct_intent_does_not_inherit_strategy() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: Arc::new(Mutex::new(0)),
                intents: vec![
                    crate::intent::Intent::NormalSemantic,
                    crate::intent::Intent::PerItemBatchAggregate,
                ],
            },
        ));
        let project = state.create_project("DistinctIntent").unwrap();
        let mid = add_file(
            &state,
            tmp.path(),
            &project.id,
            "k8s.md",
            "Notas de Kubernetes.\n",
        );
        let first = state
            .send_message(&project.id, "¿Qué dijeron sobre Kubernetes?", &[])
            .unwrap();
        assert_eq!(first.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("normal")
        );
        let provider_before = state.test_activity().provider_calls;
        let second = state
            .send_message(
                &project.id,
                "Ahora resumime cada archivo por separado.",
                std::slice::from_ref(&mid),
            )
            .unwrap();
        assert_eq!(second.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_ne!(metrics.retrieval_mode.as_deref(), Some("normal"));
        assert_eq!(
            state.test_activity().provider_calls,
            provider_before,
            "switching intents must not add a chat-answer provider call"
        );
    }

    #[test]
    fn mid_conversation_attachments_do_not_lock_knowledge_mode() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::NormalSemantic,
                crate::intent::Intent::OrdinaryChat,
                crate::intent::Intent::NormalSemantic,
            ],
        );
        let project = state.create_project("MidAttach").unwrap();
        state.send_message(&project.id, "Hola", &[]).unwrap();
        state
            .send_message(&project.id, "¿Qué es Kubernetes?", &[])
            .unwrap();
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::OrdinaryChat
        );
        let mid = add_file(
            &state,
            tmp.path(),
            &project.id,
            "doc.md",
            "Documento adjunto sobre Kubernetes y OpenShift.\n",
        );
        state
            .send_message(
                &project.id,
                "Resumime estos archivos.",
                std::slice::from_ref(&mid),
            )
            .unwrap();
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        let attach_req = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(attach_req.session_id.starts_with("fresh-normal-"));
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state.send_message(&project.id, "Gracias.", &[]).unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let thanks = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(thanks.session_id, "cached-project-session");
        assert!(!thanks.text.contains("Documento adjunto sobre Kubernetes"));
        state
            .send_message(&project.id, "¿Qué documentos mencionan OpenShift?", &[])
            .unwrap();
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        let later = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(later.session_id.starts_with("fresh-normal-"));
        assert_eq!(thanks.knowledge, None);
    }

    #[test]
    fn active_material_scope_replaces_a_with_b() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("ScopeAB").unwrap();
        let a = add_file(
            &state,
            tmp.path(),
            &project.id,
            "alpha.md",
            "Material A exclusivo ALPHA-TOKEN.\n",
        );
        let b = add_file(
            &state,
            tmp.path(),
            &project.id,
            "beta.md",
            "Material B exclusivo BETA-TOKEN.\n",
        );
        state
            .send_message(&project.id, "Tomá este set A.", std::slice::from_ref(&a))
            .unwrap();
        state
            .send_message(&project.id, "Ahora usá el set B.", std::slice::from_ref(&b))
            .unwrap();
        let run = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        let message = run.message.unwrap_or_default();
        assert!(
            message.contains("beta.md") || message.contains("BETA"),
            "{message}"
        );
        assert!(
            !message.contains("alpha.md") && !message.contains("ALPHA-TOKEN"),
            "set A must not leak into the B-scoped per-item turn: {message}"
        );
        let follow = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &[])
            .unwrap();
        assert_eq!(follow.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        let follow_message = follow.message.unwrap_or_default();
        assert!(
            !follow_message.contains("alpha.md") && !follow_message.contains("ALPHA-TOKEN"),
            "later per-item must stay on set B: {follow_message}"
        );
    }

    #[test]
    fn ephemeral_failure_does_not_destroy_conversational_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::OrdinaryChat,
                crate::intent::Intent::NormalSemantic,
                crate::intent::Intent::OrdinaryChat,
            ],
        );
        let project = state.create_project("FailEph").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Kubernetes en la reunión.\n",
        );
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(
            state.cancel_target_role(&project.id),
            Some("conversational")
        );
        *engine.fail_next_send.lock().unwrap() = true;
        let failed = state
            .send_message(&project.id, "INC-FAIL-K8S", &[])
            .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(
            engine
                .open_session(&AgentProject {
                    project_id: project.id.clone(),
                    directory: tmp
                        .path()
                        .join("projects")
                        .join(&project.id)
                        .join("workspace"),
                })
                .unwrap()
                .id,
            "cached-project-session"
        );
        assert_eq!(
            state.cancel_target_role(&project.id),
            Some("conversational")
        );
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state.send_message(&project.id, "Gracias.", &[]).unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let last = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.session_id, "cached-project-session");
        assert!(last.knowledge.is_none());
    }

    #[test]
    fn cancel_after_ephemeral_points_at_conversational_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::OrdinaryChat,
                crate::intent::Intent::NormalSemantic,
            ],
        );
        let project = state.create_project("CancelEph").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Kubernetes.\n",
        );
        state.send_message(&project.id, "Hola", &[]).unwrap();
        state.cancel_agent(&project.id).unwrap();
        assert_eq!(
            *engine.cancelled.lock().unwrap(),
            vec!["cached-project-session".to_owned()]
        );
        state.send_message(&project.id, "INC-1", &[]).unwrap();
        assert_eq!(
            state.cancel_target_role(&project.id),
            Some("conversational")
        );
        state.cancel_agent(&project.id).unwrap();
        assert_eq!(
            engine.cancelled.lock().unwrap().last().map(String::as_str),
            Some("cached-project-session")
        );
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("cancel_target_role=conversational")),
            "{logs:?}"
        );
    }

    #[test]
    fn reopen_does_not_reuse_previous_process_session_id() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().to_path_buf();
        let project_id = {
            let engine = SessionIsolationEngine::default();
            *engine.session_prefix.lock().unwrap() = "gen1".to_owned();
            let state = AppState::with_components(
                base.clone(),
                engine.clone(),
                FakeTunnel::new(),
                connector(),
                FakeRestarter::new(),
            );
            state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
                CountingClassifier {
                    calls: Arc::new(Mutex::new(0)),
                    intent: crate::intent::Intent::OrdinaryChat,
                    reason: crate::intent::ReasonCode::SemanticClassifier,
                },
            ));
            let project = state.create_project("ReopenSess").unwrap();
            add_file(
                &state,
                tmp.path(),
                &project.id,
                "reunion.md",
                "Kubernetes en notas persistentes.\n",
            );
            state.send_message(&project.id, "Hola", &[]).unwrap();
            let sent_id = engine
                .sent
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .session_id
                .clone();
            assert_eq!(sent_id, "gen1-cached-project-session");
            project.id
        };
        let engine = SessionIsolationEngine::default();
        *engine.session_prefix.lock().unwrap() = "gen2".to_owned();
        let state = AppState::with_components(
            base,
            engine.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: Arc::new(Mutex::new(0)),
                intent: crate::intent::Intent::OrdinaryChat,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        state.send_message(&project_id, "Seguimos.", &[]).unwrap();
        let sent = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(sent.session_id, "gen2-cached-project-session");
        assert_ne!(sent.session_id, "gen1-cached-project-session");
        let project = state.open_project(&project_id).unwrap();
        assert!(
            project
                .messages
                .iter()
                .any(|message| message.text.contains("Hola"))
        );
    }

    #[test]
    fn model_switch_is_applied_per_turn_on_conversational_and_ephemeral_sessions() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let engine = SessionIsolationEngine::default();
        let connector = connector().with_model(ModelSummary {
            provider_id: "opencode".into(),
            model_id: "other-model".into(),
            name: "other-model".into(),
            free: true,
            recommended: false,
            deprecated: false,
        });
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            engine.clone(),
            FakeTunnel::new(),
            connector,
            FakeRestarter::new(),
        );
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: Arc::new(Mutex::new(0)),
                intents: vec![
                    crate::intent::Intent::OrdinaryChat,
                    crate::intent::Intent::NormalSemantic,
                ],
            },
        ));
        let project = state.create_project("ModelSwitch").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Kubernetes.\n",
        );
        state.send_message(&project.id, "Hola", &[]).unwrap();
        let first = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            first.model.as_ref().map(|model| model.model_id.as_str()),
            Some("big-pickle")
        );
        state
            .conversation_model_select(&project.id, "opencode", "other-model")
            .unwrap();
        state.send_message(&project.id, "INC-MODEL", &[]).unwrap();
        let second = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert!(second.session_id.starts_with("fresh-normal-"));
        assert_eq!(
            second.model.as_ref().map(|model| model.model_id.as_str()),
            Some("other-model")
        );
        assert_eq!(
            first.model.as_ref().map(|model| model.provider_id.as_str()),
            Some("opencode")
        );
        assert_eq!(
            second
                .model
                .as_ref()
                .map(|model| model.provider_id.as_str()),
            Some("opencode")
        );
    }

    #[test]
    fn exhaustive_local_then_ordinary_chat_has_no_residual_knowledge_session() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::KnowledgeInventory,
                crate::intent::Intent::OrdinaryChat,
            ],
        );
        let project = state.create_project("LocalThenChat").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Kubernetes aparece en este archivo.\n",
        );
        let first = state
            .send_message(&project.id, "¿En qué archivos aparece Kubernetes?", &[])
            .unwrap();
        assert_eq!(first.status, "completed");
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("inventory")
        );
        assert!(
            engine.sent.lock().unwrap().is_empty(),
            "local Knowledge must not open an answer session"
        );
        assert_eq!(state.cancel_target_role(&project.id), None);
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state.send_message(&project.id, "Gracias.", &[]).unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let last = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.session_id, "cached-project-session");
    }

    #[test]
    fn inventory_does_not_lock_the_next_turn_out_of_per_item() {
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("InvThenPerItem").unwrap();
        let mut ids = Vec::new();
        for index in 0..3 {
            ids.push(add_file(
                &state,
                tmp.path(),
                &project.id,
                &format!("i-{index}.md"),
                "contenido\n",
            ));
        }
        state
            .send_message(&project.id, "¿Cuántos archivos tengo?", &[])
            .unwrap();
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("inventory")
        );
        let run = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &ids)
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("per_item_batch_aggregate")
        );
    }

    #[test]
    fn multilingual_corpus_prompt_can_be_classified_as_knowledge() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::CorpusExhaustive,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("MultiKnow").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes en la reunión.\n",
        );
        for prompt in [
            "What did the meetings say about Kubernetes?",
            "会議でKubernetesについて何が話されましたか",
        ] {
            *calls.lock().unwrap() = 0;
            let run = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(run.status, "completed", "{prompt}");
            assert_eq!(*calls.lock().unwrap(), 1, "{prompt}");
            assert_eq!(
                state.last_routing_decision().unwrap().decision.intent,
                crate::intent::Intent::CorpusExhaustive,
                "{prompt}"
            );
        }
    }

    #[test]
    fn multilingual_ordinary_chat_classifier_is_respected_without_local_keywords() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::OrdinaryChat,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("MultiOrdinary").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes en la reunión.\n",
        );
        for prompt in [
            "What did the meetings say about Kubernetes?",
            "会議でKubernetesについて何が話されましたか",
        ] {
            *calls.lock().unwrap() = 0;
            let retrieval_before = state.test_activity().retrieval;
            let embedding_before = state.test_activity().embedding_inference;
            let provider_before = state.test_activity().provider_calls;
            let k6_before = state.test_activity().k6_calls;
            let run = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(run.status, "completed", "{prompt}");
            assert_eq!(*calls.lock().unwrap(), 1, "{prompt}");
            assert_eq!(
                state.last_routing_decision().unwrap().decision.intent,
                crate::intent::Intent::OrdinaryChat,
                "{prompt}"
            );
            assert_ordinary_chat_no_knowledge_processing(
                &state,
                &project.id,
                retrieval_before,
                embedding_before,
                provider_before,
                k6_before,
            );
        }
    }

    #[test]
    fn trusted_normal_semantic_is_not_reinterpreted_as_exhaustive_or_k6() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::NormalSemantic,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("KeepNormal").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes en la reunión.\n",
        );
        let run = state
            .send_message(
                &project.id,
                "¿En qué reuniones se habló de Kubernetes?",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("normal"));
        assert_ne!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    }

    #[test]
    fn persisted_knowledge_turn_still_classifies_and_keeps_knowledge_engines() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            CountingClassifier {
                calls: calls.clone(),
                intent: crate::intent::Intent::CorpusExhaustive,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            },
        ));
        let project = state.create_project("NeedKnowledge").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Hoy hablamos de Kubernetes y OpenShift.\n",
        );
        let run = state
            .send_message(
                &project.id,
                "¿En qué reuniones se habló de Kubernetes?",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::CorpusExhaustive
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("exhaustive"));
    }

    #[test]
    fn current_turn_attachment_plus_explicit_file_operation_is_knowledge() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("AttachOp").unwrap();
        let mid = add_file(
            &state,
            tmp.path(),
            &project.id,
            "doc.md",
            "Documento de prueba.\n",
        );
        let run = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &[mid])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
    }

    #[test]
    fn same_conversation_can_alternate_ordinary_and_knowledge() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let project = state.create_project("Alternate").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "reunion.md",
            "Notas de Kubernetes y Docker en la reunión.\n",
        );

        let ordinary = state.send_message(&project.id, "Hola", &[]).unwrap();
        assert_eq!(ordinary.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::OrdinaryChat
        );

        let knowledge = state
            .send_message(
                &project.id,
                "¿En qué reuniones se habló de Kubernetes?",
                &[],
            )
            .unwrap();
        assert_eq!(knowledge.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::CorpusExhaustive
        );

        let retrieval_before_ordinary = state.test_activity().retrieval;
        let embedding_before_ordinary = state.test_activity().embedding_inference;
        let provider_before_ordinary = state.test_activity().provider_calls;
        let k6_before_ordinary = state.test_activity().k6_calls;
        let ordinary_again = state
            .send_message(&project.id, "Explicame Docker en general.", &[])
            .unwrap();
        assert_eq!(ordinary_again.status, "completed");
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before_ordinary,
            embedding_before_ordinary,
            provider_before_ordinary,
            k6_before_ordinary,
        );

        let followup = state
            .send_message(&project.id, "de esos, ¿cuáles mencionan OpenShift?", &[])
            .unwrap();
        assert_eq!(followup.status, "completed");
        let followup_route = state.last_routing_decision().unwrap();
        assert_eq!(
            followup_route.decision.intent,
            crate::intent::Intent::CorpusExhaustive
        );
        assert_eq!(
            followup_route.decision.reason_code,
            crate::intent::ReasonCode::ContextualFollowUp
        );
    }

    /// Phase 6: switching engines in one conversation must not leave a residual
    /// Knowledge/session "mode". Each turn's BoundRoute is the only strategy.
    #[test]
    fn phase6_cross_engine_transitions_leave_no_residual_mode() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        #[derive(Clone)]
        struct Phase6K6Fake;
        impl RemoteSummarizer for Phase6K6Fake {
            fn summarize(
                &self,
                _request: &SummaryRequest,
            ) -> Result<SummaryOutput, SummaryFailure> {
                let content = project_knowledge::SummaryContent {
                    summary: "resumen K6 de prueba".into(),
                    topics: vec![],
                    decisions: vec![],
                    action_items: vec![],
                    questions: vec![],
                };
                Ok(SummaryOutput {
                    text: serde_json::to_string(&content).unwrap(),
                    model_id: Some("fake".into()),
                    provider_id: Some("fake".into()),
                    usage: SummaryUsage::default(),
                })
            }
        }
        state.set_summarizer(Phase6K6Fake);
        state.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ScriptedClassifier {
                calls: Arc::new(Mutex::new(0)),
                intents: vec![
                    crate::intent::Intent::NormalSemantic,
                    crate::intent::Intent::CorpusExhaustive,
                    crate::intent::Intent::PerItemBatchAggregate,
                    crate::intent::Intent::OrdinaryChat,
                    crate::intent::Intent::KnowledgeInventory,
                    crate::intent::Intent::WholeCorpusSummary,
                    crate::intent::Intent::OrdinaryChat,
                ],
            },
        ));
        let project = state.create_project("Phase6Cross").unwrap();
        let m1 = add_file(
            &state,
            tmp.path(),
            &project.id,
            "r1.md",
            "Kubernetes INC-CROSS aparece en esta reunión junto con Docker.\n",
        );
        let m2 = add_file(
            &state,
            tmp.path(),
            &project.id,
            "r2.md",
            "En otra reunión se volvió a hablar de Kubernetes INC-CROSS.\n",
        );

        let semantic = state
            .send_message(&project.id, "¿Qué dijeron sobre INC-CROSS?", &[])
            .unwrap();
        assert_eq!(semantic.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::NormalSemantic
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("normal")
        );

        let exhaustive = state
            .send_message(&project.id, "¿En qué archivos aparece Kubernetes?", &[])
            .unwrap();
        assert_eq!(exhaustive.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::CorpusExhaustive
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("exhaustive")
        );
        assert_ne!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .retrieval_mode
                .as_deref(),
            Some("normal")
        );

        let per_item = state
            .send_message(
                &project.id,
                "Resumime cada archivo por separado.",
                &[m1.clone(), m2.clone()],
            )
            .unwrap();
        assert_eq!(per_item.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::PerItemBatchAggregate
        );
        let per_item_metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(per_item_metrics.retrieval_mode, None);
        assert_eq!(
            per_item_metrics.local_mode.as_deref(),
            Some("per_item_batch_aggregate")
        );

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        let ordinary = state
            .send_message(&project.id, "Explicame Docker en general.", &[])
            .unwrap();
        assert_eq!(ordinary.status, "completed");
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );

        let inventory = state
            .send_message(&project.id, "listame todos los archivos", &[])
            .unwrap();
        assert_eq!(inventory.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::KnowledgeInventory
        );
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("inventory")
        );
        let inventory_provider = state.test_activity().provider_calls;
        let inventory_k6 = state.test_activity().k6_calls;

        let k6 = state
            .send_message(&project.id, "resumime todos los archivos", &[])
            .unwrap();
        assert_eq!(k6.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::WholeCorpusSummary
        );
        assert!(
            state.test_activity().k6_calls > inventory_k6,
            "K6 must run after inventory without inheriting inventory local_mode"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            inventory_provider,
            "K6 must not add a conversational answer call"
        );
        assert_ne!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("inventory")
        );

        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state
            .send_message(&project.id, "Gracias, ahora charlemos.", &[])
            .unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
    }

    #[test]
    fn phase6_thematic_followup_then_ordinary_chat_is_not_locked() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let (engine, state) = isolation_scripted(
            &tmp,
            vec![
                crate::intent::Intent::CorpusThematic,
                crate::intent::Intent::OrdinaryChat,
            ],
        );
        let project = state.create_project("ThemeThenChat").unwrap();
        engine
            .open_session(&AgentProject {
                project_id: project.id.clone(),
                directory: tmp
                    .path()
                    .join("projects")
                    .join(&project.id)
                    .join("workspace"),
            })
            .unwrap();
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "r-a.md",
            "En la reunión se trabajó el presente continuo. También Google Workspace.\n",
        );
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "r-b.md",
            "Volvimos sobre el presente continuo. Google Workspace se usó para compartir.\n",
        );
        let thematic = state
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        assert_eq!(thematic.status, "completed");
        assert_eq!(
            state.last_routing_decision().unwrap().decision.intent,
            crate::intent::Intent::CorpusThematic
        );
        let followup = state
            .send_message(&project.id, "para cada tema identificá los archivos", &[])
            .unwrap();
        assert_eq!(followup.status, "completed");
        let followup_route = state.last_routing_decision().unwrap();
        assert_eq!(
            followup_route.decision.intent,
            crate::intent::Intent::CorpusThematic
        );
        assert_eq!(
            followup_route.decision.reason_code,
            crate::intent::ReasonCode::ContextualFollowUp
        );
        let retrieval_before = state.test_activity().retrieval;
        let embedding_before = state.test_activity().embedding_inference;
        let provider_before = state.test_activity().provider_calls;
        let k6_before = state.test_activity().k6_calls;
        state
            .send_message(&project.id, "Ahora explicame Docker en general.", &[])
            .unwrap();
        assert_ordinary_chat_no_knowledge_processing(
            &state,
            &project.id,
            retrieval_before,
            embedding_before,
            provider_before,
            k6_before,
        );
        let last = engine.sent.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.session_id, "cached-project-session");
        assert!(last.knowledge.is_none());
        assert!(!last.text.contains("<knowledge_evidence"));
    }

    /// Semantic `Intent::Creation` must run the deterministic creation binding:
    /// a current-turn READY attachment grounds the turn, and with no target it
    /// degrades to a local clarification (never ungrounded chat, never Internal).
    #[test]
    fn semantic_creation_runs_deterministic_grounding() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::Creation,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });

        // A) classifier says Creation + current attachment -> grounded.
        let project = state.create_project("SemCreationAtt").unwrap();
        let mid = add_file(
            &state,
            tmp.path(),
            &project.id,
            "nota.md",
            "Contenido de nota.\n",
        );
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(
                &project.id,
                "Crée une page interactive",
                &[mid],
            )
            .unwrap();
        let route = state.resolve_route(
            &inputs.project_id,
            &inputs.prompt,
            &inputs.selected_material_ids,
            inputs.model.as_ref(),
        );
        assert_eq!(route.decision.intent, crate::intent::Intent::Creation);
        state.apply_route(&mut inputs, &route).unwrap();
        let meta = inputs
            .creation
            .expect("creation must be deterministically grounded");
        assert_eq!(meta.referent_type, "current_attachment");
        assert!(!meta.clarified);

        // B) classifier says Creation but deterministic grounding is impossible
        //    (no attachment, no prior referent) -> safe degrade to the existing
        //    product semantics (no creation context, no Internal error).
        let project2 = state.create_project("SemCreationNone").unwrap();
        add_file(
            &state,
            tmp.path(),
            &project2.id,
            "solo.md",
            "Contenido solo.\n",
        );
        let mut inputs = state
            .resolve_agent_inputs_without_knowledge(&project2.id, "Crée une page", &[])
            .unwrap();
        let route = state.resolve_route(
            &inputs.project_id,
            &inputs.prompt,
            &inputs.selected_material_ids,
            inputs.model.as_ref(),
        );
        assert_eq!(route.decision.intent, crate::intent::Intent::Creation);
        assert_eq!(
            route.creation_request.as_ref().unwrap().target_cue,
            crate::creation::CreationTargetCue::Unspecified,
            "the deterministic binding still runs and marks the target as unspecified"
        );
        state.apply_route(&mut inputs, &route).unwrap();
        assert!(
            inputs.creation.is_none(),
            "no target -> safe degrade to normal chat, never an Internal error"
        );
    }

    /// `[routing]` telemetry must distinguish a trusted semantic decision from a
    /// semantic fallback from a deterministic bypass, and classifier usage stays
    /// separate from final-answer usage.
    #[test]
    fn routing_telemetry_distinguishes_classifier_provenance() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();

        // Semantic success -> classifier_impl=opencode classifier_result=success.
        let success = staged_theme_state(tmp.path());
        success.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::NormalSemantic,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let project = success.create_project("TelemetrySuccess").unwrap();
        add_file(&success, tmp.path(), &project.id, "a.md", "Contenido A.\n");
        success
            .send_message(&project.id, "¿Qué dice el archivo?", &[])
            .unwrap();
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing
                .iter()
                .any(|line| line.contains("classifier_impl=opencode")
                    && line.contains("classifier_result=success")),
            "semantic success must be identified: {routing:?}"
        );

        crate::session_log::clear();

        // Semantic fallback -> classifier_impl=opencode classifier_result=fallback
        // + fallback_reason.
        let fallback = staged_theme_state(tmp.path());
        fallback.set_test_classifier(crate::classifier::SemanticIntentClassifier::new(
            ErroringClassifier,
        ));
        let project = fallback.create_project("TelemetryFallback").unwrap();
        add_file(&fallback, tmp.path(), &project.id, "a.md", "Contenido A.\n");
        fallback
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing
                .iter()
                .any(|line| line.contains("classifier_impl=opencode")
                    && line.contains("classifier_result=fallback")
                    && line.contains("classifier_fallback_reason=unavailable")),
            "semantic fallback must identify impl + result + reason: {routing:?}"
        );

        crate::session_log::clear();

        // Deterministic bypass -> classifier_impl=deterministic_adapter
        // classifier_result=bypass.
        let bypass = staged_theme_state(tmp.path());
        let project = bypass.create_project("TelemetryBypass").unwrap();
        add_file(&bypass, tmp.path(), &project.id, "a.md", "Contenido A.\n");
        bypass
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        let routing = crate::session_log::list()
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing.iter().any(|line| {
                line.contains("classifier_impl=deterministic_adapter")
                    && line.contains("classifier_result=bypass")
            }),
            "deterministic routing must be identified as a bypass: {routing:?}"
        );
        crate::session_log::clear();
    }

    /// The classifier scratch session is separate from the final-answer session,
    /// and the classifier receives no Knowledge evidence/document body.
    #[test]
    fn classifier_session_is_separate_and_receives_no_document_bodies() {
        let _session_log_guard = crate::session_log::test_guard();
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_response_finish("stop");
        server.set_prompt_response_text(
            r#"{"intent":"corpus_thematic","modifiers":[],"confidence":0.9}"#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let state = classifier_fake_state(tmp.path(), &server);
        let project = state.create_project("ClassSeamEF").unwrap();
        // A body with a distinctive sentinel that must never reach the classifier.
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "nota.md",
            "SENTINEL_DOCUMENT_BODY contenido sobre gramática.\n",
        );
        add_file(&state, tmp.path(), &project.id, "b.md", "contenido B.\n");
        let run = state
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        // Exactly one scratch session (the classifier's); the final answer uses
        // the injected FakeAgentEngine, not the OpenCode backend.
        assert_eq!(
            server.created_session_ids().len(),
            1,
            "classifier must create exactly one scratch session"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            1,
            "the final answer must run once through the injected engine"
        );
        let prompt = server
            .last_prompt_text()
            .expect("classifier prompt was sent");
        assert!(prompt.contains("¿Qué temas se repiten?"));
        assert!(prompt.contains("corpus_thematic"));
        assert!(
            !prompt.contains("SENTINEL_DOCUMENT_BODY"),
            "classifier must never receive a document body"
        );
    }

    /// Builds an AppState whose classifier backend points at a fake OpenCode
    /// server, so production-seam tests can run the real OpenCodeIntentClassifier
    /// through a scripted session.
    fn classifier_fake_state(
        base: &std::path::Path,
        server: &fake_opencode_server::FakeServer,
    ) -> AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
        let (ready, _calls) = ready_engine();
        let mut state = recording_app(
            base,
            RecordingEngine(ready, Arc::new(Mutex::new(Vec::new()))),
        );
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), base.join("oc-config"), 0);
        backend.set_base_url(server.base_url());
        state.classifier_backend = Some(Arc::new(backend));
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state
    }

    /// Staged state for the ThemeSet regression. Unlike `staged_state`, the
    /// agent engine is ready and returns a non-empty assistant message because a
    /// CorpusThematic turn legitimately reaches the provider synthesis boundary.
    fn staged_theme_state(
        base: &std::path::Path,
    ) -> AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter> {
        let (ready, _calls) = ready_engine();
        let mut state = recording_app(
            base,
            RecordingEngine(ready, Arc::new(Mutex::new(Vec::new()))),
        );
        let backend =
            OpenCodeBackend::new(PathBuf::from("/usr/bin/true"), base.join("oc-config"), 0);
        state.summarizer_backend = Some(Arc::new(backend));
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        state
    }

    fn staged_theme_files(base: &std::path::Path) -> Vec<String> {
        let corpus = base.join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let bodies = [
            (
                "r-a.md",
                "En la reunión se trabajó el presente continuo con los alumnos. El presente continuo se practicó en ejercicios orales. También se habló de Google Workspace para organizar las clases.\n",
            ),
            (
                "r-b.md",
                "Volvimos sobre el presente continuo porque costaba. El presente continuo apareció en varias actividades. Google Workspace se usó para compartir materiales.\n",
            ),
        ];
        bodies
            .iter()
            .map(|(name, body)| {
                let path = corpus.join(name);
                std::fs::write(&path, body).unwrap();
                path.to_string_lossy().to_string()
            })
            .collect()
    }

    /// Permanent staged-seam regression for the ThemeSet contextual follow-up.
    ///
    /// Exercises the exact packaged-application path
    /// `send_staged_message_persist` -> `run_accepted_staged_turn` ->
    /// `run_accepted_staged_turn_inner`.
    ///
    /// Turn 1 asks an UNSCOPED recurrence thematic question ("¿Cuáles son los
    /// temas recurrentes?"). Pre-fix it classified `NormalSemantic` (recurrence
    /// wording was not treated as corpus-wide thematic scope), so no ThemeSet
    /// was persisted and the human follow-up fell back to normal hybrid
    /// retrieval. It must now classify `CorpusThematic`, persist a ThemeSet on
    /// the thematic user turn, and let the follow-up reuse the exact persisted
    /// theme keys without rediscovery.
    #[test]
    fn staged_unscoped_recurrence_theme_persists_and_followup_reuses_theme_set() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let project = state.create_project("StagedRecurrence").unwrap();
        let paths = staged_theme_files(tmp.path());

        // Upload + index the two-material corpus through the staged seam.
        let upload = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(upload.status, "completed");

        // TURN 1: an UNSCOPED recurrence thematic question. Pre-fix this
        // classified NormalSemantic; it must now classify CorpusThematic and
        // persist a ThemeSet on the thematic user turn.
        let thematic = run_staged_turn(
            &state,
            &project.id,
            "¿Cuáles son los temas recurrentes?",
            &[],
        );
        assert_eq!(thematic.status, "completed");
        let thematic_metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(thematic_metrics.retrieval_mode.as_deref(), Some("thematic"));
        assert_eq!(thematic_metrics.contextual_followup, None);

        // Inspect project.json: exactly one ThemeSet, attached to the thematic
        // user message.
        let disk = fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_str(&disk).unwrap();
        let theme_sets: Vec<(String, Vec<String>)> = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .filter_map(|message| {
                if message["turnReferent"]["kind"] == "themeSet" {
                    let keys: Vec<String> = message["turnReferent"]["themeKeys"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|key| key.as_str().unwrap().to_owned())
                        .collect();
                    Some((message["text"].as_str().unwrap_or("").to_owned(), keys))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            theme_sets.len(),
            1,
            "the thematic turn must persist exactly one ThemeSet"
        );
        let (attached_to, original_keys) = theme_sets.into_iter().next().unwrap();
        assert_eq!(
            attached_to, "¿Cuáles son los temas recurrentes?",
            "the ThemeSet must be attached to the thematic user message"
        );
        assert!(
            !original_keys.is_empty(),
            "a ThemeSet must persist at least one theme key"
        );
        assert!(original_keys.contains(&"presente continuo".to_owned()));

        // TURN 2: the human follow-up must resolve to the EXACT persisted
        // ThemeSet and never fall back to NormalSemantic / hybrid retrieval.
        let followup = run_staged_turn(
            &state,
            &project.id,
            "para cada uno de los temas recurrentes que acabás de identificar, indicame las reuniones exactas donde aparece",
            &[],
        );
        assert_eq!(followup.status, "completed");
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.contextual_followup, Some(true));
        assert_eq!(metrics.referent_type.as_deref(), Some("theme_set"));
        assert_eq!(metrics.referent_count, Some(original_keys.len()));
        assert_eq!(metrics.base_intent.as_deref(), Some("thematic"));
        assert_eq!(metrics.turn_kind.as_deref(), Some("per_theme_detail"));
        assert_eq!(metrics.retrieval_mode.as_deref(), Some("thematic"));
        assert_ne!(
            metrics.retrieval_mode.as_deref(),
            Some("normal"),
            "the follow-up must not route to NormalSemantic / hybrid retrieval"
        );

        // The persisted ThemeSet is unchanged: the follow-up reuses the exact
        // keys and never redisovers a different theme set.
        let disk_after = fs::read_to_string(
            tmp.path()
                .join("projects")
                .join(&project.id)
                .join("project.json"),
        )
        .unwrap();
        let json_after: serde_json::Value = serde_json::from_str(&disk_after).unwrap();
        let after_keys: Vec<Vec<String>> = json_after["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] == "user")
            .filter_map(|message| {
                if message["turnReferent"]["kind"] == "themeSet" {
                    Some(
                        message["turnReferent"]["themeKeys"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|key| key.as_str().unwrap().to_owned())
                            .collect(),
                    )
                } else {
                    None
                }
            })
            .collect();
        assert!(
            !after_keys.is_empty(),
            "the original ThemeSet must survive the follow-up"
        );
        assert!(
            after_keys.iter().all(|keys| keys == &original_keys),
            "every persisted ThemeSet must keep the exact original keys, never a different rediscovered set: {after_keys:?}"
        );
    }

    /// Fake K6 summarizer that records the exact document source names the
    /// summary route hands it and returns deterministic per-source prose. It
    /// never touches the Knowledge store or the embedding provider, so a
    /// correct selected-per-source route can be asserted by the names it sees.
    #[derive(Clone)]
    struct SelectedSourceRecorder {
        calls: Arc<Mutex<usize>>,
        document_sources: Arc<Mutex<Vec<String>>>,
    }

    impl RemoteSummarizer for SelectedSourceRecorder {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            *self.calls.lock().unwrap() += 1;
            let text = match request.level {
                project_knowledge::SummaryLevel::Document => {
                    let name = request
                        .labels
                        .first()
                        .map(|label| label.source_name.clone())
                        .unwrap_or_default();
                    self.document_sources.lock().unwrap().push(name.clone());
                    format!("Resumen de {name}")
                }
                _ => "Síntesis agregada de las fuentes seleccionadas.".to_owned(),
            };
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(5),
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    cost_usd: Some(0.001),
                    provider_actual: true,
                },
            })
        }
    }

    /// Production-seam regression for the same-turn generic summary bug.
    ///
    /// 50 current-turn attachments + "me podrás hacer un resumen" must select
    /// exactly those 50 accepted READY materials and route through one bounded
    /// selected-set aggregate call, never K6/top-K retrieval, never raw
    /// forwarding, and never a re-embed after ingestion.
    #[test]
    fn staged_50_file_bare_summary_targets_current_turn_materials() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let project = state.create_project("StagedSummary50").unwrap();

        // An older, unrelated corpus material must never become the summary
        // target; the selected set stays the current-turn 50 files only.
        let historical = tmp.path().join("historical.md");
        std::fs::write(&historical, "HISTORICAL_ONLY_SENTINEL").unwrap();
        state
            .add_material_from_path(&project.id, historical.to_str().unwrap())
            .unwrap();

        let paths = staged_material_paths(tmp.path(), 50);
        let accepted = state
            .send_staged_message_persist(&project.id, "me podrás hacer un resumen", &paths, &[])
            .unwrap();
        assert_eq!(
            accepted.material_ids.len(),
            50,
            "current-turn accepted material count"
        );
        assert_eq!(
            accepted.inputs.selected_material_ids.len(),
            50,
            "selected material ids must be exactly the accepted set"
        );

        let embedding_before = state.test_activity().embedding_inference;
        let run = state.run_accepted_staged_turn(accepted).unwrap();
        assert_eq!(run.status, "completed");

        // Route proof: one bounded aggregate call, no K6/normal chat/top-K
        // retrieval, no raw attachment forwarding, no re-embed beyond ingestion.
        let activity = state.test_activity();
        assert_eq!(
            activity.k6_calls, 0,
            "generic selected summary must not run K6"
        );
        assert_eq!(activity.provider_calls, 0, "no normal chat provider call");
        assert_eq!(activity.retrieval, 0, "no top-K retrieval");
        assert_eq!(activity.raw_attachment_forwarding, 0, "no raw forwarding");
        assert_eq!(
            activity.embedding_inference,
            embedding_before + 1,
            "only the ingestion embedding round, never a re-embed for the summary"
        );

        assert_eq!(
            *aggregate_calls.lock().unwrap(),
            1,
            "50 items use one bounded aggregate call"
        );
        // The aggregate output has exactly one source-provenance slot for each
        // selected material, never the historical corpus material.
        let expected: Vec<String> = (0..50).map(|i| format!("staged-{i:02}.md")).collect();

        // The user-facing surface lists each selected source and never claims
        // there is nothing to summarize.
        let surface = run.message.expect("selected source surface");
        for name in &expected {
            assert!(
                surface.contains(name),
                "missing selected source {name}: {surface}"
            );
        }
        assert!(!surface.contains("historical"));
        assert!(!surface.contains("HISTORICAL_ONLY_SENTINEL"));

        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.retrieval_mode, None,
            "summary route is not a retrieval mode"
        );
        assert_eq!(
            metrics.retrieval_candidate_count, None,
            "no semantic top-K candidate set"
        );
        assert_eq!(
            metrics.selected_evidence_count, None,
            "no semantic evidence set"
        );
        assert_eq!(metrics.remote_calls, Some(1));
        assert_eq!(
            metrics.local_mode.as_deref(),
            Some("selected_batch_aggregate")
        );

        let first_totals = state
            .accumulated_conversation_usage(&project.id)
            .unwrap()
            .unwrap();
        assert_eq!(first_totals.remote_calls, Some(1));
        assert_eq!(first_totals.input_tokens, Some(100));
        assert_eq!(first_totals.output_tokens, Some(50));
        assert_eq!(first_totals.cost_usd, Some(0.01));

        // A later selected aggregate contributes once to durable totals; it
        // does not replace the first turn or introduce retrieval work.
        let second_path = tmp.path().join("second-selected.md");
        std::fs::write(&second_path, "Segundo material independiente.\n").unwrap();
        let second_paths = vec![second_path.to_string_lossy().to_string()];
        let second = run_staged_turn(
            &state,
            &project.id,
            "haceme un resumen de este archivo",
            &second_paths,
        );
        assert_eq!(second.status, "completed");
        assert_eq!(*aggregate_calls.lock().unwrap(), 2);
        let totals = state
            .accumulated_conversation_usage(&project.id)
            .unwrap()
            .unwrap();
        assert_eq!(totals.remote_calls, Some(2));
        assert_eq!(totals.input_tokens, Some(200));
        assert_eq!(totals.output_tokens, Some(100));
        assert_eq!(totals.cost_usd, Some(0.02));
        drop(state);
        let reopened = staged_state(tmp.path());
        let reopened_totals = reopened
            .accumulated_conversation_usage(&project.id)
            .unwrap()
            .unwrap();
        assert_eq!(reopened_totals, totals);

        // The route must be logged distinctly from NormalSemantic/hybrid.
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_scope=current_turn_materials")
                && entry
                    .message
                    .contains("summary_mode=selected_batch_aggregate")
                && entry.message.contains("selected_materials=50")),
            "distinct same-turn summary telemetry line must be recorded"
        );
    }

    /// CASE D: a bare generic summary with zero current-turn attachments must
    /// not bind arbitrary corpus material into K6. It stays ordinary chat.
    #[test]
    fn staged_bare_summary_without_attachments_does_not_bind_corpus() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("StagedZeroAttachment").unwrap();
        let paths = staged_theme_files(tmp.path());
        let upload = run_staged_turn(&state, &project.id, "listame los archivos", &paths);
        assert_eq!(upload.status, "completed");

        let k6_before = state.test_activity().k6_calls;
        let provider_before = state.test_activity().provider_calls;
        let run = run_staged_turn(&state, &project.id, "me podrás hacer un resumen", &[]);
        assert_eq!(run.status, "completed");
        assert_eq!(
            state.test_activity().k6_calls,
            k6_before,
            "zero-attachment generic summary must not run K6"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            provider_before + 1,
            "zero-attachment generic summary stays ordinary chat"
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.retrieval_mode, None,
            "zero-attachment generic summary is ordinary chat, not RAG"
        );
    }

    /// CASE C: a single current-turn attachment with a bare summary verb
    /// ("resumime esto") must target that one material.
    #[test]
    fn staged_one_file_bare_summary_targets_it() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        let project = state.create_project("StagedOneFile").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(&state, &project.id, "resumime esto", &paths);
        assert_eq!(run.status, "completed");
        let activity = state.test_activity();
        assert_eq!(activity.k6_calls, 0);
        assert_eq!(activity.provider_calls, 0);
        assert_eq!(activity.retrieval, 0);
        let surface = run.message.expect("one-source surface");
        assert!(surface.contains("staged-00.md"), "{surface}");
    }

    /// CASE A/B deictic wording with 50 files must also select the current-turn
    /// set through bounded selected-set aggregation, not K6 or normal chat.
    #[test]
    fn staged_50_file_deictic_summary_targets_current_turn_materials() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let project = state.create_project("StagedDeictic50").unwrap();
        let paths = staged_material_paths(tmp.path(), 50);
        let run = run_staged_turn(
            &state,
            &project.id,
            "haceme un resumen de estos archivos",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let activity = state.test_activity();
        assert_eq!(activity.k6_calls, 0);
        assert_eq!(activity.provider_calls, 0);
        assert_eq!(activity.retrieval, 0);
        assert_eq!(*aggregate_calls.lock().unwrap(), 1);
    }

    /// Deep per-document wording (explicit detail/analysis cue) retains the
    /// durable K6 SelectedPerSource topology; compact per-document wording
    /// routes to the bounded per-item aggregate, never K6.
    #[test]
    fn staged_explicit_per_document_summaries_split_deep_k6_vs_compact() {
        // DEEP -> K6 (k6_calls == 1, local_mode != per-item).
        for prompt in [
            "haceme un resumen detallado de cada documento",
            "hacé un resumen exhaustivo y profundo de cada archivo",
            "analizá detalladamente cada documento por separado",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            let project = state.create_project("StagedDeepK6").unwrap();
            let paths = staged_material_paths(tmp.path(), 2);
            let run = run_staged_turn(&state, &project.id, prompt, &paths);
            assert_eq!(run.status, "completed", "{prompt}");
            assert_eq!(state.test_activity().k6_calls, 1, "{prompt}");
            let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
            assert!(
                metrics.local_mode.as_deref() != Some("per_item_batch_aggregate")
                    && metrics.local_mode.as_deref() != Some("selected_batch_aggregate"),
                "{prompt}: deep per-source must not be captured by the compact/generic aggregate"
            );
        }

        // COMPACT -> bounded per-item aggregate (k6_calls == 0).
        for prompt in [
            "resumime cada archivo por separado",
            "resumí uno por uno todos los archivos",
            "resumime cada documento",
            "dame un resumen breve de cada documento",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            let project = state.create_project("StagedCompact").unwrap();
            let paths = staged_material_paths(tmp.path(), 2);
            let run = run_staged_turn(&state, &project.id, prompt, &paths);
            assert_eq!(run.status, "completed", "{prompt}");
            assert_eq!(state.test_activity().k6_calls, 0, "{prompt}");
            assert_eq!(
                state
                    .last_turn_metrics(&project.id)
                    .unwrap()
                    .unwrap()
                    .local_mode
                    .as_deref(),
                Some("per_item_batch_aggregate"),
                "{prompt}"
            );
        }
    }

    /// Compact per-item scale: exact cardinality, identity, stable ordering,
    /// no K6, no chat provider call, no re-embed, and a bounded remote-call
    /// count that never equals the material count.
    #[test]
    fn compact_per_item_scale_1_15_50_100_files_keeps_exact_cardinality() {
        for count in [1usize, 15, 50, 100] {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            let aggregate_calls = Arc::new(Mutex::new(0));
            state.set_per_item_summarizer(PerItemFake {
                calls: aggregate_calls.clone(),
            });
            let project = state
                .create_project(&format!("CompactScale{count}"))
                .unwrap();

            // A historical material that must never leak into the compact set.
            let historical = tmp.path().join("historical.md");
            std::fs::write(&historical, "HISTORICAL_ONLY_SENTINEL").unwrap();
            state
                .add_material_from_path(&project.id, historical.to_str().unwrap())
                .unwrap();

            let paths = staged_material_paths(tmp.path(), count);
            let run = run_staged_turn(
                &state,
                &project.id,
                "resumime cada archivo por separado",
                &paths,
            );
            assert_eq!(run.status, "completed", "count={count}");

            let activity = state.test_activity();
            assert_eq!(
                activity.k6_calls, 0,
                "compact never runs K6 (count={count})"
            );
            assert_eq!(
                activity.provider_calls, 0,
                "compact never runs a normal chat provider call (count={count})"
            );
            assert_eq!(
                activity.retrieval, 0,
                "compact never runs top-K retrieval (count={count})"
            );
            assert_eq!(
                activity.raw_attachment_forwarding, 0,
                "compact never forwards raw corpus (count={count})"
            );

            let surface = run.message.expect("compact surface");
            assert_eq!(
                count_lines(&surface),
                count,
                "exact one slot per selected material (count={count})"
            );
            for index in 0..count {
                let name = format!("staged-{index:02}.md");
                let line_prefix = format!("{}. {name}:", index + 1);
                assert_eq!(
                    surface.matches(&line_prefix).count(),
                    1,
                    "exact identity, once, in stable order (count={count}, {name})"
                );
            }
            assert!(!surface.contains("historical"), "count={count}");
            assert!(
                !surface.contains("HISTORICAL_ONLY_SENTINEL"),
                "count={count}"
            );

            let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
            assert_eq!(
                metrics.local_mode.as_deref(),
                Some("per_item_batch_aggregate"),
                "count={count}"
            );
            assert_eq!(metrics.retrieval_mode, None, "count={count}");
            assert_eq!(metrics.remote_calls, Some(1), "count={count}");
            assert_eq!(
                *aggregate_calls.lock().unwrap(),
                1,
                "one bounded aggregate call (count={count})"
            );
        }
    }

    /// 100 files with a forced small budget prove deterministic batching: the
    /// remote-call count is ceil(N / budget), never N, and cardinality is exact.
    #[test]
    fn compact_per_item_100_files_batches_deterministically() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        state.set_per_item_options(crate::per_item::PerItemExecutionOptions {
            max_items_per_call: 30,
            max_words_per_item: 40,
            max_serialized_bytes_per_call: usize::MAX,
        });
        let project = state.create_project("CompactBatch100").unwrap();
        let paths = staged_material_paths(tmp.path(), 100);
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(
            count_lines(run.message.as_deref().unwrap()),
            100,
            "exact 100 slots across batches"
        );
        // ceil(100 / 30) == 4 deterministic aggregate calls, never 100.
        assert_eq!(*aggregate_calls.lock().unwrap(), 4);
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(4));
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("per_item_batches=4")
                && entry.message.contains("selected_materials=100")),
            "batch telemetry must be recorded"
        );
    }

    /// A partial provider output (47 of 50 keys returned) must keep exact
    /// cardinality with explicit synthesis-failure slots and truthful telemetry
    /// (47 generated / 3 failed), never a silent 50/50 success.
    #[test]
    fn compact_per_item_partial_provider_output_is_explicitly_failed() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(OmittingPerItemFake {
            calls: aggregate_calls.clone(),
            omit_last: 3,
        });
        let project = state.create_project("PartialOutput").unwrap();
        let paths = staged_material_paths(tmp.path(), 50);
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");
        assert_eq!(state.test_activity().k6_calls, 0);

        let surface = run.message.expect("compact surface");
        assert_eq!(
            count_lines(&surface),
            50,
            "exact 50 output slots, cardinality never shrinks"
        );
        let generated = surface.matches("Resumen de ").count();
        let failed = surface
            .matches("No se pudo generar el resumen de este archivo en esta ejecución.")
            .count();
        assert_eq!(generated, 47, "exactly 47 generated summaries");
        assert_eq!(failed, 3, "exactly 3 explicit synthesis-failure slots");

        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_requested=50")
                && entry.message.contains("items_generated=47")
                && entry.message.contains("items_failed=3")),
            "telemetry must say 47 generated / 3 failed"
        );
    }

    /// Compact per-item production seam: one READY material plus the compact
    /// wording must run the bounded aggregate through the REAL OpenCode 1.18.25
    /// completion parser (not a fake summarizer), produce one generated slot,
    /// never K6, never a chat provider call, never a timeout, and never a retry.
    #[test]
    fn compact_per_item_production_seam_one_ready_material() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_response_text("M001: Resumen del material de gramática.");
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeam").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 1, "exact one slot: {surface}");
        assert!(
            surface.contains("Resumen del material de gramática."),
            "real generated summary must surface: {surface}"
        );
        assert!(!surface.contains("No se pudo generar el resumen"));

        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(state.test_activity().provider_calls, 0);
        assert_eq!(state.test_activity().retrieval, 0);

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=1")
                && entry.message.contains("items_failed=0")
                && entry.message.contains("query_embeddings=0")
        }));
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
        assert_eq!(
            server.created_session_ids().len(),
            1,
            "one bounded provider execution, no retry"
        );
        assert_eq!(
            server.prompt_async_paths().len(),
            1,
            "exactly one prompt_async per operation, no resend"
        );
    }

    /// Deep/K6 production seam: one READY material plus the deep wording must
    /// route to SelectedPerSource and complete its document remote node through
    /// the same shared completion parser, with no finish_stop_missing and a real
    /// generated document result.
    #[test]
    fn k6_selected_per_source_production_seam_one_ready_material() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_k6_echo_from_prompt(true);
        server.set_prompt_response_text(
            r#"{"summary":"Resumen usable del archivo seleccionado.","topics":[],"decisions":[],"action_items":[],"questions":[]}"#,
        );
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("K6Seam").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Analizá detalladamente cada documento por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        assert!(
            surface.contains("staged-00.md"),
            "document source must be named: {surface}"
        );
        assert!(
            !surface.contains("No se pudo procesar este archivo."),
            "document node must not degrade to failure copy: {surface}"
        );

        assert_eq!(state.test_activity().k6_calls, 1);
        assert_eq!(state.test_activity().provider_calls, 0);
        assert_eq!(state.test_activity().retrieval, 0);

        let logs = crate::session_log::list();
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("finish=stop")),
            "document node must emit a finish=stop telemetry line"
        );
    }

    /// Compact per-item production seam: a provider error on the newest
    /// assistant message must produce an explicit failure slot, never fake
    /// success and never a 120s timeout.
    #[test]
    fn compact_per_item_production_seam_provider_error_is_not_fake_success() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            "[]",
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","error":{"message":"context overflow"}},"parts":[{"type":"text","text":"partial"}]}]"#,
        ]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeamProviderError").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 1, "exact one slot: {surface}");
        assert!(
            surface.contains("No se pudo generar el resumen de este archivo en esta ejecución."),
            "provider error must produce an explicit failure slot: {surface}"
        );
        assert!(!surface.contains("Resumen de "));

        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(state.test_activity().provider_calls, 0);

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=0")
                && entry.message.contains("items_failed=1")
        }));
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("timeout_reason=provider_error")),
            "provider error must be surfaced, not a 120s timeout"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
    }

    /// Compact per-item production seam: `finish="length"` must not masquerade
    /// as a complete summary; it is an explicit truncated failure slot.
    #[test]
    fn compact_per_item_production_seam_length_is_not_fake_success() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            "[]",
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","finish":"length"},"parts":[{"type":"text","text":"truncado"}]}]"#,
        ]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeamLength").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 1, "exact one slot: {surface}");
        assert!(
            surface.contains("No se pudo generar el resumen de este archivo en esta ejecución."),
            "length-truncated output must be an explicit failure slot: {surface}"
        );
        assert!(!surface.contains("Resumen de "));

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=0")
                && entry.message.contains("items_failed=1")
        }));
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("timeout_reason=output_truncated")),
            "truncated length must be surfaced as output_truncated"
        );
    }

    /// Compact per-item production seam: `finish="content-filter"` must be an
    /// explicit failure slot, never filtered content presented as success.
    #[test]
    fn compact_per_item_production_seam_content_filter_is_not_fake_success() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            "[]",
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","finish":"content-filter"},"parts":[{"type":"text","text":"filtrado"}]}]"#,
        ]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeamContentFilter").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 1, "exact one slot: {surface}");
        assert!(
            surface.contains("No se pudo generar el resumen de este archivo en esta ejecución."),
            "content-filtered output must be an explicit failure slot: {surface}"
        );
        assert!(!surface.contains("Resumen de "));

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=0")
                && entry.message.contains("items_failed=1")
        }));
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("timeout_reason=content_filter")),
            "content-filter must be surfaced as content_filter"
        );
    }

    /// Deep/K6 production seam: a provider error on the document node must not
    /// become a document success and must not reuse any older completion.
    #[test]
    fn k6_selected_per_source_production_seam_provider_error_is_not_document_success() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        server.set_messages_sequence(&[
            "[]",
            r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1","error":{"message":"boom"}},"parts":[{"type":"text","text":"partial"}]}]"#,
        ]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("K6SeamProviderError").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Analizá detalladamente cada documento por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        assert!(
            surface.contains("staged-00.md"),
            "document source must be named: {surface}"
        );
        assert!(
            surface.contains("No se pudo procesar este archivo."),
            "provider error must not become a document success: {surface}"
        );
        assert!(!surface.contains("Resumen usable del archivo seleccionado."));

        assert_eq!(state.test_activity().k6_calls, 1);

        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("timeout_reason=provider_error")),
            "provider error must be surfaced, not a timeout"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
    }

    /// Compact per-item production seam for the human AppImage trace: OpenCode
    /// 1.18.25 returns a finish-absent, correctly parent-correlated assistant
    /// whose text is stable, so the bounded aggregate must complete via the
    /// quiescence fallback (`terminal_source=stable_text`), never the 120s
    /// `finish_stop_missing` timeout.
    #[test]
    fn compact_per_item_production_seam_quiescent_text_one_ready_material() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        let stable = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1"},"parts":[{"type":"text","text":"M001: Resumen del material de gramática."}]}]"#;
        server.set_messages_sequence(&["[]", stable]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeamStable").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 1, "exact one slot: {surface}");
        assert!(
            surface.contains("Resumen del material de gramática."),
            "real generated summary must surface: {surface}"
        );
        assert!(!surface.contains("No se pudo generar el resumen"));

        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(state.test_activity().provider_calls, 0);
        assert_eq!(state.test_activity().retrieval, 0);

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=1")
                && entry.message.contains("items_failed=0")
                && entry.message.contains("query_embeddings=0")
        }));
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("terminal_source=stable_text")),
            "stable-text completion must be recorded: {logs:?}"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
        assert_eq!(
            server.created_session_ids().len(),
            1,
            "one bounded provider execution, no retry"
        );
    }

    /// Compact per-item multi-file seam: a single aggregate call whose
    /// finish-absent response carries two stable `Mxxx` slots must complete via
    /// the quiescence fallback and preserve exact cardinality.
    #[test]
    fn compact_per_item_production_seam_quiescent_text_multiple_files() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        let stable = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1"},"parts":[{"type":"text","text":"M001: Resumen del material 0.\nM002: Resumen del material 1."}]}]"#;
        server.set_messages_sequence(&["[]", stable]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("CompactSeamStableMulti").unwrap();
        let paths = staged_material_paths(tmp.path(), 2);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 2, "exact two slots: {surface}");
        assert!(
            surface.contains("Resumen del material 0.")
                && surface.contains("Resumen del material 1."),
            "both generated summaries must surface: {surface}"
        );
        assert!(!surface.contains("No se pudo generar el resumen"));

        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(state.test_activity().provider_calls, 0);

        let logs = crate::session_log::list();
        assert!(logs.iter().any(|entry| {
            entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_generated=2")
                && entry.message.contains("items_failed=0")
        }));
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
        assert_eq!(server.created_session_ids().len(), 1, "one aggregate call");
    }

    /// Deep/K6 production seam: the K6 document node must finish via the same
    /// stable-text quiescence fallback (finish-absent assistant) without being
    /// misclassified as a document failure.
    #[test]
    fn k6_selected_per_source_production_seam_quiescent_text_one_document() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let mut state = staged_state(tmp.path());

        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        let stable = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1"},"parts":[{"type":"text","text":"{\"summary\":\"Resumen usable del archivo seleccionado.\",\"topics\":[],\"decisions\":[],\"action_items\":[],\"questions\":[]}"}]}]"#;
        server.set_messages_sequence(&["[]", stable]);
        let backend = OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        );
        backend.set_base_url(server.base_url());
        backend.ensure_ready().expect("fake OpenCode ready");
        state.summarizer_backend = Some(Arc::new(backend));

        let project = state.create_project("K6SeamStable").unwrap();
        let paths = staged_material_paths(tmp.path(), 1);
        let run = run_staged_turn(
            &state,
            &project.id,
            "Analizá detalladamente cada documento por separado.",
            &paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("selected source surface");
        assert!(
            surface.contains("staged-00.md"),
            "document source must be named: {surface}"
        );
        assert!(
            !surface.contains("No se pudo procesar este archivo."),
            "document node must not degrade to failure copy: {surface}"
        );

        assert_eq!(state.test_activity().k6_calls, 1);
        assert_eq!(state.test_activity().provider_calls, 0);

        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("terminal_source=stable_text")),
            "K6 document node must record the stable-text completion: {logs:?}"
        );
        assert!(
            !logs
                .iter()
                .any(|entry| entry.message.contains("timeout_reason=finish_stop_missing"))
        );
    }

    /// Classifier production seam: a valid classifier decision arrives as a
    /// finish-absent, correctly parent-correlated assistant whose text is
    /// stable. It must be accepted via the quiescence fallback, never the 30s
    /// finish-missing timeout, and routing must stay semantic.
    #[test]
    fn classifier_production_seam_quiescent_valid_text() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let server = fake_opencode_server::FakeServer::start();
        server.set_prompt_appends_response(false);
        let stable = r#"[{"info":{"id":"u1","role":"user"},"parts":[{"type":"text","text":"x"}]},{"info":{"id":"a1","role":"assistant","parentID":"u1"},"parts":[{"type":"text","text":"{\"intent\":\"corpus_thematic\",\"modifiers\":[],\"confidence\":0.9}"}]}]"#;
        server.set_messages_sequence(&["[]", stable]);
        let tmp = tempfile::tempdir().unwrap();
        let state = classifier_fake_state(tmp.path(), &server);
        let project = state.create_project("ClassifierStable").unwrap();
        add_file(&state, tmp.path(), &project.id, "a.md", "Contenido A.\n");
        add_file(&state, tmp.path(), &project.id, "b.md", "contenido B.\n");
        let run = state
            .send_message(&project.id, "¿Qué temas se repiten?", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(
            server.created_session_ids().len(),
            1,
            "classifier must create exactly one scratch session"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            1,
            "the final answer must run once through the injected engine"
        );
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("terminal_source=stable_text")),
            "classifier must record the stable-text completion: {logs:?}"
        );
        let routing = logs
            .into_iter()
            .filter(|entry| entry.message.starts_with("[routing]"))
            .map(|entry| entry.message)
            .collect::<Vec<_>>();
        assert!(
            routing
                .iter()
                .any(|line| line.contains("classifier_impl=opencode")
                    && line.contains("classifier_result=success")),
            "a valid quiescent classifier decision must be semantic success: {routing:?}"
        );
    }

    /// A per-item summarizer that fails exactly one aggregate call (1-indexed),
    /// so a multi-batch turn can prove one failed batch never erases the slots
    /// already produced by earlier successful batches.
    struct FailNthPerItemFake {
        calls: Arc<Mutex<usize>>,
        fail_on: usize,
    }
    impl RemoteSummarizer for FailNthPerItemFake {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            let n = {
                let mut calls = self.calls.lock().unwrap();
                *calls += 1;
                *calls
            };
            if n == self.fail_on {
                return Err(SummaryFailure::ExecutionFailed);
            }
            let mut text = String::new();
            for item in &request.labels {
                text.push_str(&format!(
                    "{}: Resumen de {}\n",
                    item.label, item.source_name
                ));
            }
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    cost_usd: Some(0.01),
                    provider_actual: true,
                },
            })
        }
    }

    /// Task 6: when batching produces multiple remote calls, one failed batch
    /// must not erase the successful earlier batches; only the affected items get
    /// explicit failure slots, and telemetry aggregates generated/failed.
    #[test]
    fn compact_per_item_multi_batch_partial_failure_keeps_earlier_slots() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(FailNthPerItemFake {
            calls: calls.clone(),
            fail_on: 2,
        });
        // 3 items per call, 5 items -> 2 batches (3 + 2); the 2nd batch fails.
        state.set_per_item_options(crate::per_item::PerItemExecutionOptions {
            max_items_per_call: 3,
            max_words_per_item: 40,
            max_serialized_bytes_per_call: usize::MAX,
        });
        let project = state.create_project("CompactMultiBatchFail").unwrap();
        let paths = staged_material_paths(tmp.path(), 5);
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(*calls.lock().unwrap(), 2, "two aggregate calls");

        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 5, "exact cardinality across batches");
        let generated = surface.matches("Resumen de ").count();
        let failed = surface
            .matches("No se pudo generar el resumen de este archivo en esta ejecución.")
            .count();
        assert_eq!(
            generated, 3,
            "first batch's 3 slots survive the failed batch"
        );
        assert_eq!(
            failed, 2,
            "the failed batch's 2 items get explicit failure slots"
        );

        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("items_requested=5")
                && entry.message.contains("items_generated=3")
                && entry.message.contains("items_failed=2")
                && entry.message.contains("per_item_batches=2")),
            "telemetry must aggregate 3 generated / 2 failed across 2 batches"
        );
    }

    /// A per-item summarizer that records the EXACT serialized byte size of every
    /// compact request it receives, so the production-seam test can prove every
    /// emitted batch stays within the default hard budget.
    struct BudgetObservingPerItemFake {
        calls: Arc<Mutex<usize>>,
        sizes: Arc<Mutex<Vec<usize>>>,
    }
    impl RemoteSummarizer for BudgetObservingPerItemFake {
        fn summarize(&self, request: &SummaryRequest) -> Result<SummaryOutput, SummaryFailure> {
            *self.calls.lock().unwrap() += 1;
            let size = crate::per_item::serialize_summary_prompt(
                &request.instruction,
                &request.labels,
                &request.evidence_texts,
            )
            .len();
            self.sizes.lock().unwrap().push(size);
            let mut text = String::new();
            for item in &request.labels {
                text.push_str(&format!(
                    "{}: Resumen de {}\n",
                    item.label, item.source_name
                ));
            }
            Ok(SummaryOutput {
                text,
                model_id: Some("fake".into()),
                provider_id: Some("fake".into()),
                usage: SummaryUsage {
                    input_tokens: Some(100),
                    output_tokens: Some(50),
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    cost_usd: Some(0.01),
                    provider_actual: true,
                },
            })
        }
    }

    /// Production-executor budget test: 100 READY items with representatives
    /// near the 800-character bound and long realistic source names must be
    /// packed into MULTIPLE bounded remote calls under the DEFAULT hard budget
    /// (never `usize::MAX`). Every actual serialized compact request must stay
    /// within the budget, cardinality must stay exact, order preserved, and no
    /// K6/re-embed/raw-forwarding may occur.
    #[test]
    fn compact_per_item_production_executor_splits_100_large_items_within_default_budget() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        let sizes = Arc::new(Mutex::new(Vec::new()));
        state.set_per_item_summarizer(BudgetObservingPerItemFake {
            calls: calls.clone(),
            sizes: sizes.clone(),
        });
        // Do NOT set per-item options: the default hard budget must govern.
        let project = state.create_project("CompactProdBudget").unwrap();
        // 100 large files with long, realistic source names. Each body is long
        // enough that the deterministic chunk representative approaches the
        // ~800-character bound (3 x 240-char chunks joined).
        let mut paths = Vec::new();
        for index in 0..100 {
            let name = format!("reunion-planificacion-trimestral-detallada-{index:03}.md");
            let body = format!(
                "Reunión {index}. {}\n",
                "Contenido representativo del documento con suficiente longitud para ".repeat(60)
            );
            let path = tmp.path().join(&name);
            std::fs::write(&path, body).unwrap();
            paths.push(path.to_string_lossy().to_string());
        }
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");

        let default_budget = crate::per_item::PER_ITEM_MAX_REQUEST_BYTES_PER_CALL;
        let call_count = *calls.lock().unwrap();
        assert!(
            call_count > 1,
            "100 large items must split into more than one bounded call"
        );
        assert!(
            call_count < 100,
            "provider-call count must stay bounded (<< N), got {call_count}"
        );
        let observed_sizes = sizes.lock().unwrap().clone();
        assert_eq!(observed_sizes.len(), call_count);
        for size in &observed_sizes {
            assert!(
                *size <= default_budget,
                "serialized request of {size} bytes exceeds the default budget {default_budget}"
            );
        }

        // No K6 lifecycle, no query embeddings, no re-index/re-embed, no raw
        // forwarding for the compact path.
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(state.test_activity().retrieval, 0);
        assert_eq!(state.test_activity().raw_attachment_forwarding, 0);
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_mode=per_item_batch_aggregate")
                && entry.message.contains("query_embeddings=0")
                && entry.message.contains("raw_forwarding=false")),
            "compact telemetry must record query_embeddings=0 and raw_forwarding=false"
        );

        // Exact output cardinality and preserved order.
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 100, "exact 100 output slots");
        for index in 0..100 {
            let name = format!("reunion-planificacion-trimestral-detallada-{index:03}.md");
            let line_prefix = format!("{}. {name}:", index + 1);
            assert_eq!(
                surface.matches(&line_prefix).count(),
                1,
                "exact identity, once, in stable order ({name})"
            );
        }

        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(
            metrics.local_mode.as_deref(),
            Some("per_item_batch_aggregate")
        );
        assert_eq!(metrics.remote_calls, Some(call_count));
    }

    /// After restart the active/persisted set survives and a no-attachment
    /// compact per-item summary reuses the same READY materials without
    /// re-embedding or re-indexing.
    #[test]
    fn compact_per_item_reuses_prior_set_after_restart_without_reembedding() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let project_id;
        {
            let state = staged_state(tmp.path());
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            let project = state.create_project("CompactRestart").unwrap();
            project_id = project.id.clone();
            let paths = staged_material_paths(tmp.path(), 5);
            let inventory = run_staged_turn(&state, &project_id, "listame los archivos", &paths);
            assert_eq!(inventory.status, "completed");
        }
        // Reopen: the durable material set must survive with no re-embedding.
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let embedding_before = state.test_activity().embedding_inference;
        let run = run_staged_turn(
            &state,
            &project_id,
            "resumime cada archivo por separado",
            &[],
        );
        assert_eq!(run.status, "completed");
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(
            state.test_activity().embedding_inference,
            embedding_before,
            "compact route must never re-embed after restart"
        );
        assert_eq!(
            count_lines(run.message.as_deref().unwrap()),
            5,
            "reused active set keeps exact cardinality"
        );
        assert_eq!(*aggregate_calls.lock().unwrap(), 1);
    }

    /// ISSUE 1 (A/B): Turn 1 attaches 4 files and asks a compact per-source
    /// summary; Turn 2 (same conversation, zero attachments, same compact
    /// wording in Spanish or French) must stay on the compact route — same 4
    /// outputs, no K6, no historical-project leakage.
    #[test]
    fn zero_attachment_compact_followup_stays_compact_across_languages() {
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
        ] {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            let aggregate_calls = Arc::new(Mutex::new(0));
            state.set_per_item_summarizer(PerItemFake {
                calls: aggregate_calls.clone(),
            });
            let project = state.create_project("CompactFollowup").unwrap();
            // A historical material that must never leak into the compact set.
            let historical = tmp.path().join("historical.md");
            std::fs::write(&historical, "HISTORICAL_ONLY_SENTINEL").unwrap();
            state
                .add_material_from_path(&project.id, historical.to_str().unwrap())
                .unwrap();

            let paths = staged_material_paths(tmp.path(), 4);
            // Turn 1: attach 4 files + compact wording.
            let turn1 = run_staged_turn(&state, &project.id, prompt, &paths);
            assert_eq!(turn1.status, "completed", "{prompt}");
            assert_eq!(state.test_activity().k6_calls, 0, "{prompt}");
            assert_eq!(
                count_lines(turn1.message.as_deref().unwrap()),
                4,
                "{prompt}"
            );

            // Turn 2: zero attachments + same compact wording.
            let turn2 = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(turn2.status, "completed", "{prompt}");
            assert_eq!(
                state.test_activity().k6_calls,
                0,
                "zero-attachment compact turn must not enter K6 ({prompt})"
            );
            let surface = turn2.message.expect("compact follow-up surface");
            assert_eq!(
                count_lines(&surface),
                4,
                "same 4 outputs on the zero-attachment turn ({prompt})"
            );
            for index in 0..4 {
                let name = format!("staged-{index:02}.md");
                assert!(
                    surface.contains(&name),
                    "turn 2 must summarize the same 4 sources ({prompt}, {name})"
                );
            }
            assert!(
                !surface.contains("historical") && !surface.contains("HISTORICAL_ONLY_SENTINEL"),
                "no historical-project leakage ({prompt})"
            );
            // Two aggregate calls total: one per turn, never K6.
            assert_eq!(*aggregate_calls.lock().unwrap(), 2, "{prompt}");
        }
    }

    /// ISSUE 1 (C): when the semantic classifier errors, the deterministic
    /// fallback must still route compact per-source wording to the compact route
    /// (never the whole-corpus K6 route) with zero attachments.
    #[test]
    fn zero_attachment_compact_classifier_failure_stays_compact() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        state.set_test_classifier(ErroringClassifier);
        let project = state.create_project("CompactClassifierFailure").unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        let turn1 = run_staged_turn(
            &state,
            &project.id,
            "Resumime cada archivo por separado.",
            &paths,
        );
        assert_eq!(turn1.status, "completed");
        assert_eq!(count_lines(turn1.message.as_deref().unwrap()), 3);
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(*aggregate_calls.lock().unwrap(), 1);
    }

    /// Task 4 (A): the true two-turn classifier-FAILURE shape. Turn 1
    /// attach+compact, Turn 2 zero-attachment compact, in ES and FR, both with
    /// the semantic classifier unavailable. The deterministic fallback must keep
    /// both turns on the compact route over the same active set — never K6,
    /// never historical leakage, never re-embedding/re-indexing.
    #[test]
    fn two_turn_compact_classifier_failure_stays_compact_across_languages() {
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
        ] {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            let aggregate_calls = Arc::new(Mutex::new(0));
            state.set_per_item_summarizer(PerItemFake {
                calls: aggregate_calls.clone(),
            });
            state.set_test_classifier(ErroringClassifier);
            let project = state.create_project("CompactTwoTurnFailure").unwrap();
            let historical = tmp.path().join("historical.md");
            std::fs::write(&historical, "HISTORICAL_ONLY_SENTINEL").unwrap();
            state
                .add_material_from_path(&project.id, historical.to_str().unwrap())
                .unwrap();

            let paths = staged_material_paths(tmp.path(), 4);
            let turn1 = run_staged_turn(&state, &project.id, prompt, &paths);
            assert_eq!(turn1.status, "completed", "{prompt}");
            assert_eq!(
                count_lines(turn1.message.as_deref().unwrap()),
                4,
                "{prompt}"
            );
            assert_eq!(state.test_activity().k6_calls, 0, "{prompt}");

            let embedding_before = state.test_activity().embedding_inference;
            let turn2 = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(turn2.status, "completed", "{prompt}");
            assert_eq!(
                state.test_activity().k6_calls,
                0,
                "classifier-failure zero-attachment compact must not enter K6 ({prompt})"
            );
            assert_eq!(
                state.test_activity().embedding_inference,
                embedding_before,
                "no re-embedding on the zero-attachment compact turn ({prompt})"
            );
            let surface = turn2.message.expect("compact follow-up surface");
            assert_eq!(count_lines(&surface), 4, "{prompt}");
            for index in 0..4 {
                let name = format!("staged-{index:02}.md");
                assert!(surface.contains(&name), "{prompt}: missing {name}");
            }
            assert!(
                !surface.contains("historical") && !surface.contains("HISTORICAL_ONLY_SENTINEL"),
                "no historical-project leakage ({prompt})"
            );
            // One aggregate call per turn, never K6.
            assert_eq!(*aggregate_calls.lock().unwrap(), 2, "{prompt}");
        }
    }

    /// Task 4 (B): the true two-turn shape with a SEMANTIC classifier SUCCESS
    /// that returns a conflicting WholeCorpusSummary for unequivocal compact
    /// wording. The compact depth clamp must keep both turns on the compact
    /// route over the same active set — never promoted to K6.
    #[test]
    fn two_turn_compact_conflicting_classifier_success_stays_compact_across_languages() {
        for prompt in [
            "Resumime cada archivo por separado.",
            "Résume chaque fichier séparément.",
        ] {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            let aggregate_calls = Arc::new(Mutex::new(0));
            state.set_per_item_summarizer(PerItemFake {
                calls: aggregate_calls.clone(),
            });
            let classifier_calls = Arc::new(Mutex::new(0));
            state.set_test_classifier(CountingClassifier {
                calls: classifier_calls.clone(),
                intent: crate::intent::Intent::WholeCorpusSummary,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            });

            let project = state.create_project("CompactTwoTurnConflict").unwrap();
            let paths = staged_material_paths(tmp.path(), 4);
            let turn1 = run_staged_turn(&state, &project.id, prompt, &paths);
            assert_eq!(turn1.status, "completed", "{prompt}");
            assert_eq!(
                count_lines(turn1.message.as_deref().unwrap()),
                4,
                "{prompt}"
            );
            assert_eq!(
                state.test_activity().k6_calls,
                0,
                "conflicting classifier success must be clamped to compact ({prompt})"
            );

            let turn2 = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(turn2.status, "completed", "{prompt}");
            assert_eq!(
                state.test_activity().k6_calls,
                0,
                "zero-attachment conflicting success must stay compact ({prompt})"
            );
            let surface = turn2.message.expect("compact follow-up surface");
            assert_eq!(count_lines(&surface), 4, "{prompt}");
            for index in 0..4 {
                let name = format!("staged-{index:02}.md");
                assert!(surface.contains(&name), "{prompt}: missing {name}");
            }
            assert_eq!(*aggregate_calls.lock().unwrap(), 2, "{prompt}");
            // The semantic classifier was actually consulted (and its conflicting
            // WholeCorpusSummary intent was clamped, not silently ignored).
            assert!(
                *classifier_calls.lock().unwrap() >= 1,
                "semantic classifier must have been consulted ({prompt})"
            );
        }
    }

    /// ISSUE 1 (D): empty conversation + compact wording + zero attachments is
    /// a truthful no-selection with zero provider summary calls and zero K6.
    #[test]
    fn empty_conversation_compact_zero_attachments_is_truthful_no_selection() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let project = state.create_project("EmptyCompact").unwrap();
        let run = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert!(
            run.message
                .as_deref()
                .unwrap()
                .contains("No tengo archivos seleccionados para resumir"),
            "truthful no-selection answer"
        );
        assert_eq!(state.test_activity().k6_calls, 0, "no K6");
        assert_eq!(*aggregate_calls.lock().unwrap(), 0, "zero summary calls");
    }

    /// ISSUE 1 (E): the project contains historical A,B plus active set C,D; a
    /// zero-attachment compact turn must summarize only C,D.
    #[test]
    fn zero_attachment_compact_uses_active_set_not_historical_knowledge() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        state.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let project = state.create_project("CompactHistorical").unwrap();

        let old_paths = vec![
            write_staged_file(tmp.path(), "old-a.md", "Contenido A.\n"),
            write_staged_file(tmp.path(), "old-b.md", "Contenido B.\n"),
        ];
        import_and_index(&state, &project.id, &old_paths);
        let new_paths = vec![
            write_staged_file(tmp.path(), "new-c.md", "Contenido C.\n"),
            write_staged_file(tmp.path(), "new-d.md", "Contenido D.\n"),
        ];
        import_and_index(&state, &project.id, &new_paths);

        let run = state
            .send_message(&project.id, "Resumime cada archivo por separado.", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("compact surface");
        assert_eq!(count_lines(&surface), 2, "only the active set C,D");
        assert!(surface.contains("new-c.md"), "{surface}");
        assert!(surface.contains("new-d.md"), "{surface}");
        assert!(!surface.contains("old-a.md"), "historical A must not leak");
        assert!(!surface.contains("old-b.md"), "historical B must not leak");
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(*aggregate_calls.lock().unwrap(), 1);
    }

    /// ISSUE 4: the compact per-item route marks a durable synthesis phase
    /// before the provider call and clears it on completion, so the frontend can
    /// replace the misleading "99% · N de N" import line with "Generando
    /// resúmenes...".
    #[test]
    fn compact_synthesis_marks_and_clears_the_durable_phase() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let project_id = Arc::new(Mutex::new(None));
        let observed = Arc::new(Mutex::new(Vec::new()));
        state.set_per_item_summarizer(SynthesizingObserver {
            base: tmp.path().to_path_buf(),
            project_id: project_id.clone(),
            observed: observed.clone(),
        });
        let project = state.create_project("SynthesisPhase").unwrap();
        *project_id.lock().unwrap() = Some(project.id.clone());
        let paths = staged_material_paths(tmp.path(), 4);
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");
        // The provider call observed the synthesis phase flag set.
        assert_eq!(
            observed.lock().unwrap().as_slice(),
            &[true],
            "synthesis phase must be marked before the provider call"
        );
        // After completion the durable operation clears the phase.
        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store.latest_accepted_import_operation().unwrap().unwrap();
        assert_eq!(
            operation.state,
            project_knowledge::AcceptedImportState::Completed
        );
        assert!(!operation.synthesizing, "phase must clear on completion");
    }

    /// Task 5: a crash after `synthesizing=true` (before the terminal finish)
    /// leaves a stale flag; reopening reconciles it to not-synthesizing WITHOUT
    /// re-sending any provider request, so the frontend never shows an endless
    /// "Generando resúmenes…" state and the interrupted operation is not
    /// presented as successfully completed.
    #[test]
    fn stale_synthesizing_flag_is_reconciled_on_reopen_without_resend() {
        let tmp = tempfile::tempdir().unwrap();
        let project_id;
        let operation_id;
        {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let state = staged_state(tmp.path());
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            let project = state.create_project("StaleSynth").unwrap();
            let paths = staged_material_paths(tmp.path(), 3);
            let run = run_staged_turn(
                &state,
                &project.id,
                "resumime cada archivo por separado",
                &paths,
            );
            assert_eq!(run.status, "completed");
            project_id = project.id.clone();
            // Simulate a crash window: mark the operation synthesizing + an
            // interrupted (pending_retry, outcome-unknown) state as if the
            // process died mid-phase.
            let pid = ProjectId::parse(&project_id).unwrap();
            let root = tmp.path().join("projects").join(&project_id);
            let mut store = KnowledgeStore::open(&root, &pid).unwrap();
            let op = store.latest_accepted_import_operation().unwrap().unwrap();
            operation_id = op.operation_id.clone();
            store
                .set_accepted_import_synthesizing(&operation_id, true)
                .unwrap();
            store
                .update_accepted_import_agent_state(
                    &operation_id,
                    project_knowledge::AcceptedImportAgentState::StartedOutcomeUnknown,
                )
                .unwrap();
            store
                .update_accepted_import_operation(
                    &operation_id,
                    op.turn_id.as_deref(),
                    project_knowledge::AcceptedImportState::PendingRetry,
                    op.copied,
                    op.lexical_completed,
                    op.embedding_completed,
                    op.failed,
                    op.embeddings_created,
                    op.embeddings_reused,
                )
                .unwrap();
        }

        // Reopen a fresh state: open_project must reconcile the stale flag.
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let fresh = staged_state(tmp.path());
        let aggregate_calls = Arc::new(Mutex::new(0));
        fresh.set_per_item_summarizer(PerItemFake {
            calls: aggregate_calls.clone(),
        });
        let view = fresh.open_project(&project_id).unwrap();
        let accepted = view.accepted_import.expect("accepted import present");
        assert!(
            !accepted.synthesizing,
            "stale synthesizing must be cleared on reopen"
        );
        assert_eq!(
            accepted.state, "pending_retry",
            "the interrupted state is preserved, never silently completed"
        );
        // No provider call was issued during reopen (reconcile is read-only).
        assert_eq!(
            *aggregate_calls.lock().unwrap(),
            0,
            "reopen must never resend a provider request"
        );
        assert_eq!(fresh.test_activity().k6_calls, 0);
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("stale_synthesizing_recovered")),
            "stale-synthesis recovery must be recorded"
        );
    }

    /// Task 5: a normal ERROR finish also clears the synthesis phase flag, so a
    /// failed turn can never leave a stale "Generando resúmenes…" state.
    #[test]
    fn compact_synthesis_error_clears_the_durable_phase() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        // Force the terminal metrics persistence to fail AFTER synthesis, so the
        // accepted-turn pipeline takes its Err path (which must clear the flag).
        state.fail_next_turn_metrics_persistence();
        let project = state.create_project("CompactErrorClear").unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "resumime cada archivo por separado",
                &paths,
                &[],
            )
            .unwrap();
        let result = state.run_accepted_staged_turn(accepted);
        assert!(
            result.is_err(),
            "metrics persistence failure must propagate as an error"
        );

        let pid = ProjectId::parse(&project.id).unwrap();
        let root = tmp.path().join("projects").join(&project.id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        let operation = store.latest_accepted_import_operation().unwrap().unwrap();
        assert!(
            !operation.synthesizing,
            "an error finish must clear the synthesis phase"
        );
        assert_eq!(
            operation.state,
            project_knowledge::AcceptedImportState::PendingRetry,
            "a failed turn stays retryable and never silently completed"
        );
    }

    /// CASE F: an ordinary semantic question with current-turn attachments must
    /// remain semantic (no summary conversion).
    #[test]
    fn staged_attachments_with_ordinary_semantic_question_stays_semantic() {
        let _session_log_guard = crate::session_log::test_guard();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        let project = state
            .create_project("StagedSemanticWithAttachments")
            .unwrap();
        let paths = staged_material_paths(tmp.path(), 3);
        let k6_before = state.test_activity().k6_calls;
        let provider_before = state.test_activity().provider_calls;
        let run = run_staged_turn(
            &state,
            &project.id,
            "¿qué dicen estos archivos sobre presente continuo?",
            &paths,
        );
        assert_eq!(run.status, "completed");
        assert_eq!(
            state.test_activity().k6_calls,
            k6_before,
            "semantic question must not route to K6 summary"
        );
        assert_eq!(
            state.test_activity().provider_calls,
            provider_before + 1,
            "semantic question reaches the normal provider boundary"
        );
    }

    /// Writes a distinct staged source file and returns its absolute path.
    fn write_staged_file(base: &std::path::Path, name: &str, body: &str) -> String {
        let path = base.join(name);
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().to_string()
    }

    /// A. Current-turn attachments must be the selected set for a per-source
    /// summary, and the scope-source telemetry must say `current_turn`.
    #[test]
    fn per_source_current_turn_attachments_are_selected() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("PerSourceCurrentTurn").unwrap();
        let paths = vec![
            write_staged_file(tmp.path(), "fresh-c.md", "Contenido C.\n"),
            write_staged_file(tmp.path(), "fresh-d.md", "Contenido D.\n"),
        ];
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        assert_eq!(run.status, "completed");
        assert_eq!(state.test_activity().k6_calls, 0);
        assert_eq!(
            state
                .last_turn_metrics(&project.id)
                .unwrap()
                .unwrap()
                .local_mode
                .as_deref(),
            Some("per_item_batch_aggregate")
        );
        let surface = run.message.expect("per-source surface");
        assert!(surface.contains("fresh-c.md"), "{surface}");
        assert!(surface.contains("fresh-d.md"), "{surface}");
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(
                |entry| entry.message.contains("summary_scope_source=current_turn")
                    && entry.message.contains("selected_materials=2")
            ),
            "current-turn scope source must be recorded"
        );
    }

    /// B. No current attachments + a compatible recent MaterialSet referent must
    /// reuse that exact set for a semantic per-source summary. Deep French
    /// wording ("Analyse ... en détail") avoids the deterministic follow-up
    /// pre-gate (its cues are ES/EN only) and carries an explicit deep cue, so
    /// the semantic classifier's PerSourceSummary is authoritative (never
    /// depth-clamped to compact).
    #[test]
    fn per_source_reuses_compatible_prior_material_set() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("PerSourcePrior").unwrap();
        let old_paths = vec![
            write_staged_file(tmp.path(), "old-a.md", "Contenido A.\n"),
            write_staged_file(tmp.path(), "old-b.md", "Contenido B.\n"),
        ];
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &old_paths);
        assert_eq!(inventory.status, "completed");

        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: calls.clone(),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let run = state
            .send_message(&project.id, "Analyse chaque document en détail.", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(state.test_activity().k6_calls, 1);
        let surface = run.message.expect("per-source surface");
        assert!(surface.contains("old-a.md"), "{surface}");
        assert!(surface.contains("old-b.md"), "{surface}");
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_scope_source=prior_material_set")
                && entry.message.contains("selected_materials=2")),
            "prior-material-set scope source must be recorded"
        );
    }

    /// C. Fresh current-turn attachments must win over an older MaterialSet
    /// referent for a per-source summary.
    #[test]
    fn per_source_fresh_attachments_win_over_prior_material_set() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        let project = state.create_project("PerSourceFreshWins").unwrap();
        let old_paths = vec![
            write_staged_file(tmp.path(), "old-a.md", "Contenido A.\n"),
            write_staged_file(tmp.path(), "old-b.md", "Contenido B.\n"),
        ];
        let inventory = run_staged_turn(&state, &project.id, "listame los archivos", &old_paths);
        assert_eq!(inventory.status, "completed");

        let new_paths = vec![
            write_staged_file(tmp.path(), "new-c.md", "Contenido C.\n"),
            write_staged_file(tmp.path(), "new-d.md", "Contenido D.\n"),
        ];
        let run = run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &new_paths,
        );
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("per-source surface");
        assert!(surface.contains("new-c.md"), "{surface}");
        assert!(surface.contains("new-d.md"), "{surface}");
        assert!(!surface.contains("old-a.md"), "{surface}");
        assert!(!surface.contains("old-b.md"), "{surface}");
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(
                |entry| entry.message.contains("summary_scope_source=current_turn")
                    && entry.message.contains("selected_materials=2")
            ),
            "fresh attachments must be reported as current_turn scope"
        );
    }

    /// D. No current attachments and no compatible set must produce a truthful
    /// local no-selection answer, never a silent completed documents=0.
    #[test]
    fn per_source_no_selection_is_a_truthful_local_answer() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_theme_state(tmp.path());
        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: calls.clone(),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let project = state.create_project("PerSourceNoSel").unwrap();
        // Persisted Knowledge but no accepted import operation and no referent.
        add_file(
            &state,
            tmp.path(),
            &project.id,
            "solo.md",
            "Contenido solo.\n",
        );
        let run = state
            .send_message(
                &project.id,
                "Analizá detalladamente cada documento por separado.",
                &[],
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(
            state.test_activity().k6_calls,
            0,
            "no-selection must never reach K6"
        );
        let message = run.message.expect("local no-selection answer");
        assert!(
            message.contains("No tengo archivos seleccionados para resumir"),
            "{message}"
        );
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.remote_calls, Some(0));
        assert_eq!(
            metrics.local_mode.as_deref(),
            Some("per_source_no_selection")
        );
        let logs = crate::session_log::list();
        assert!(
            logs.iter()
                .any(|entry| entry.message.contains("summary_scope_source=none")
                    && entry.message.contains("selected_materials=0")),
            "no-selection scope source must be recorded"
        );
    }

    /// E/F. Spanish and French deep per-source wording bind the same active
    /// accepted-import material set when the structural context is identical.
    #[test]
    fn per_source_spanish_and_french_bind_the_active_import_set_identically() {
        for prompt in [
            "Analizá detalladamente cada documento por separado.",
            "Analyse chaque document en détail.",
        ] {
            let _session_log_guard = crate::session_log::test_guard();
            crate::session_log::clear();
            let tmp = tempfile::tempdir().unwrap();
            let state = staged_state(tmp.path());
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });
            let project = state.create_project("PerSourceActiveImport").unwrap();
            let paths = (0..4)
                .map(|index| {
                    write_staged_file(
                        tmp.path(),
                        &format!("doc-{index}.md"),
                        &format!("Contenido del documento {index}.\n"),
                    )
                })
                .collect::<Vec<_>>();
            let accepted = state
                .send_staged_message_persist(&project.id, "subí estos archivos", &paths, &[])
                .unwrap();
            state.index_accepted_material_batch(
                &accepted.inputs.project_id,
                &accepted.material_ids,
                Some(&accepted.operation_id),
                false,
            );
            state.set_test_classifier(CountingClassifier {
                calls: Arc::new(Mutex::new(0)),
                intent: crate::intent::Intent::PerSourceSummary,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            });
            let run = state.send_message(&project.id, prompt, &[]).unwrap();
            assert_eq!(run.status, "completed", "{prompt}");
            let surface = run.message.expect("per-source surface");
            for index in 0..4 {
                let name = format!("doc-{index}.md");
                assert!(
                    surface.contains(&name),
                    "{prompt}: missing {name} in {surface}"
                );
            }
            let logs = crate::session_log::list();
            assert!(
                logs.iter().any(|entry| entry
                    .message
                    .contains("summary_scope_source=conversation_active_material_set")
                    && entry.message.contains("selected_materials=4")),
                "{prompt}: conversation-active-material-set scope source must be recorded"
            );
        }
    }

    /// G. The MaterialSet referent and the active accepted-import set persist,
    /// so a reopen resolves the same per-source scope without re-embedding or
    /// re-indexing.
    #[test]
    fn per_source_scope_survives_restart_without_reembedding() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let project_id = {
            let state = staged_state(tmp.path());
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });
            state.set_per_item_summarizer(PerItemFake {
                calls: Arc::new(Mutex::new(0)),
            });
            let project = state.create_project("PerSourceRestart").unwrap();
            let old_paths = vec![
                write_staged_file(tmp.path(), "prior-a.md", "Contenido A.\n"),
                write_staged_file(tmp.path(), "prior-b.md", "Contenido B.\n"),
            ];
            let inventory =
                run_staged_turn(&state, &project.id, "listame los archivos", &old_paths);
            assert_eq!(inventory.status, "completed");
            state.set_test_classifier(CountingClassifier {
                calls: Arc::new(Mutex::new(0)),
                intent: crate::intent::Intent::PerSourceSummary,
                reason: crate::intent::ReasonCode::SemanticClassifier,
            });
            let run = state
                .send_message(&project.id, "Analyse chaque document en détail.", &[])
                .unwrap();
            assert_eq!(run.status, "completed");
            assert!(run.message.expect("surface").contains("prior-a.md"));
            project.id
        };

        // Reopen a fresh process-like state over the same data dir.
        let fresh = staged_state(tmp.path());
        fresh.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        fresh.set_per_item_summarizer(PerItemFake {
            calls: Arc::new(Mutex::new(0)),
        });
        fresh.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let run = fresh
            .send_message(&project_id, "Analyse chaque document en détail.", &[])
            .unwrap();
        assert_eq!(run.status, "completed");
        let surface = run.message.expect("surface");
        assert!(surface.contains("prior-a.md"), "{surface}");
        assert!(surface.contains("prior-b.md"), "{surface}");
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().indexing, 0);
    }

    /// Reads the durable conversation-active material set for a project, in
    /// original attach order.
    fn active_material_ids(
        state: &AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
        project_id: &str,
    ) -> Vec<String> {
        let pid = ProjectId::parse(project_id).unwrap();
        let root = state.base_dir().join("projects").join(project_id);
        let store = KnowledgeStore::open(&root, &pid).unwrap();
        store
            .conversation_active_material_ids()
            .unwrap()
            .into_iter()
            .map(|id| id.as_str().to_owned())
            .collect()
    }

    /// Accepts and indexes a staged material batch (the durable import step)
    /// without running a K6/summary turn, so tests can exercise the active-set
    /// fallback with exactly one summary turn.
    fn import_and_index(
        state: &AppState<RecordingEngine, FakeTunnel, FakeProviderConnector, FakeRestarter>,
        project_id: &str,
        paths: &[String],
    ) -> Vec<String> {
        let accepted = state
            .send_staged_message_persist(project_id, "subí estos archivos", paths, &[])
            .unwrap();
        state.index_accepted_material_batch(
            &accepted.inputs.project_id,
            &accepted.material_ids,
            Some(&accepted.operation_id),
            false,
        );
        accepted.material_ids
    }

    /// The exact live AppImage reproduction (DEEP per-source path): Turn 1
    /// attaches 4 files and asks a deep per-source analysis in the same turn
    /// (the real staged seam, forced PerSourceSummary via explicit deep wording
    /// so the compact depth clamp never applies); Turn 2 (same conversation, no
    /// attachments, deep French) must reuse those exact 4 materials through the
    /// durable conversation-active material set. The live failure was caused by
    /// the no-attachment turn's own empty accepted-import operation shadowing
    /// the earlier 4-material operation in `latest_accepted_import_operation`.
    #[test]
    fn staged_french_no_attachment_per_source_reuses_the_prior_active_set() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let document_sources = Arc::new(Mutex::new(Vec::new()));
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: document_sources.clone(),
        });
        let project = state.create_project("LiveReproFrench").unwrap();

        // Turn 1: the exact human sequence — attach 4 files and ask a per-source
        // summary in the same turn through the packaged staged seam.
        let paths = (0..4)
            .map(|index| {
                write_staged_file(
                    tmp.path(),
                    &format!("doc-{index}.md"),
                    &format!("Contenido del documento {index}.\n"),
                )
            })
            .collect::<Vec<_>>();
        let turn1_calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: turn1_calls.clone(),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "analizá detalladamente cada documento por separado",
                &paths,
                &[],
            )
            .unwrap();
        let turn1_ids = accepted.material_ids.clone();
        assert_eq!(turn1_ids.len(), 4);
        let turn1 = state.run_accepted_staged_turn(accepted).unwrap();
        assert_eq!(turn1.status, "completed");
        assert_eq!(*turn1_calls.lock().unwrap(), 1);
        assert!(turn1.message.is_some(), "assistant answer exists");
        // The current-turn selection is exactly the four ids, and the K6/
        // selected-source recorder saw exactly those four sources.
        assert_eq!(active_material_ids(&state, &project.id), turn1_ids);
        {
            let seen = document_sources.lock().unwrap();
            assert_eq!(
                seen.as_slice(),
                ["doc-0.md", "doc-1.md", "doc-2.md", "doc-3.md"],
                "Turn 1 must summarize exactly the four current-turn sources"
            );
        }
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(
                |entry| entry.message.contains("summary_scope_source=current_turn")
                    && entry.message.contains("selected_materials=4")
            ),
            "Turn 1 must be recorded as current_turn scope"
        );

        // Turn 2: same conversation, no attachments, French semantic classifier
        // resolves PerSourceSummary (exactly like the live AppImage test).
        let turn2_calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: turn2_calls.clone(),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let turn2 = run_staged_turn(
            &state,
            &project.id,
            "Analyse chaque document en détail.",
            &[],
        );
        assert_eq!(turn2.status, "completed");
        assert_eq!(*turn2_calls.lock().unwrap(), 1);
        let surface = turn2.message.expect("per-source surface");
        assert!(
            !surface.contains("No tengo archivos seleccionados para resumir"),
            "Turn 2 must not fall into NoSelection: {surface}"
        );
        for index in 0..4 {
            let name = format!("doc-{index}.md");
            assert!(surface.contains(&name), "missing {name} in {surface}");
        }
        // Turn 2 resolved the exact same four ids: the durable active set is
        // unchanged, so the scope-resolution seam fed K6 the identical set it
        // summarized in Turn 1. (The document summaries are reused by node
        // fingerprint, so the provider is not re-invoked; no new source was
        // handed to the recorder.)
        assert_eq!(active_material_ids(&state, &project.id), turn1_ids);
        assert_eq!(
            document_sources.lock().unwrap().len(),
            4,
            "Turn 2 must not hand K6 any new or different source"
        );
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_scope_source=conversation_active_material_set")
                && entry.message.contains("selected_materials=4")),
            "conversation-active-material-set scope source must be recorded"
        );
    }

    /// A later fresh attachment replaces the active set: [E,F] become both the
    /// current turn and the durable active set, so a following no-attachment
    /// per-source summary uses [E,F], never the earlier [A,B,C,D].
    #[test]
    fn fresh_attachments_replace_the_active_set_for_later_turns() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        let project = state.create_project("FreshReplace").unwrap();

        let old_paths = vec![
            write_staged_file(tmp.path(), "old-a.md", "Contenido A.\n"),
            write_staged_file(tmp.path(), "old-b.md", "Contenido B.\n"),
            write_staged_file(tmp.path(), "old-c.md", "Contenido C.\n"),
            write_staged_file(tmp.path(), "old-d.md", "Contenido D.\n"),
        ];
        let imported = import_and_index(&state, &project.id, &old_paths);
        assert_eq!(imported.len(), 4);
        assert_eq!(active_material_ids(&state, &project.id).len(), 4);

        let new_paths = vec![
            write_staged_file(tmp.path(), "new-e.md", "Contenido E.\n"),
            write_staged_file(tmp.path(), "new-f.md", "Contenido F.\n"),
        ];
        let imported_fresh = import_and_index(&state, &project.id, &new_paths);
        assert_eq!(imported_fresh.len(), 2);
        assert_eq!(
            active_material_ids(&state, &project.id).len(),
            2,
            "the fresh attachment set replaces the active set"
        );

        let calls = Arc::new(Mutex::new(0));
        state.set_test_classifier(CountingClassifier {
            calls: calls.clone(),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let later = run_staged_turn(
            &state,
            &project.id,
            "Analyse chaque document en détail.",
            &[],
        );
        assert_eq!(later.status, "completed");
        let surface = later.message.expect("later surface");
        assert!(surface.contains("new-e.md"), "{surface}");
        assert!(surface.contains("new-f.md"), "{surface}");
        assert!(!surface.contains("old-a.md"), "{surface}");
        assert!(!surface.contains("old-b.md"), "{surface}");
        assert!(!surface.contains("old-c.md"), "{surface}");
        assert!(!surface.contains("old-d.md"), "{surface}");
    }

    /// The durable active set persists across a real close/reopen, so a later
    /// no-attachment per-source summary reuses the same ids with no
    /// re-embedding or re-indexing.
    #[test]
    fn active_set_survives_restart_without_reembedding() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let project_id = {
            let state = staged_state(tmp.path());
            state.set_summarizer(SelectedSourceRecorder {
                calls: Arc::new(Mutex::new(0)),
                document_sources: Arc::new(Mutex::new(Vec::new())),
            });
            let project = state.create_project("ActiveSetRestart").unwrap();
            let paths = (0..4)
                .map(|index| {
                    write_staged_file(
                        tmp.path(),
                        &format!("active-{index}.md"),
                        &format!("Contenido activo {index}.\n"),
                    )
                })
                .collect::<Vec<_>>();
            let turn1 = run_staged_turn(
                &state,
                &project.id,
                "resumime cada archivo por separado",
                &paths,
            );
            assert_eq!(turn1.status, "completed");
            project.id
        };

        let fresh = staged_state(tmp.path());
        fresh.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        fresh.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        let run = fresh
            .send_staged_message_persist(
                &project_id,
                "Analyse chaque document en détail.",
                &[],
                &[],
            )
            .unwrap();
        fresh.run_accepted_staged_turn(run).unwrap();
        // Reopen resolves the active set from SQLite. No real re-embedding or
        // re-indexing of materials happens (`indexing` counts the empty batch
        // attempt; embedding inference/persistence must stay zero).
        assert_eq!(fresh.test_activity().embedding_inference, 0);
        assert_eq!(fresh.test_activity().embedding_persistence, 0);
        assert_eq!(active_material_ids(&fresh, &project_id).len(), 4);
        let logs = crate::session_log::list();
        assert!(
            logs.iter().any(|entry| entry
                .message
                .contains("summary_scope_source=conversation_active_material_set")
                && entry.message.contains("selected_materials=4")),
            "restart must resolve the same active set"
        );
    }

    /// The active set is conversation-scoped: two conversations in the same
    /// project directory do not leak each other's active sets. This exercises
    /// the actual PerSourceSummary dispatch — each conversation's no-attachment
    /// turn reaches the selected-source recorder with only its own two sources.
    #[test]
    fn active_set_is_conversation_scoped() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let document_sources = Arc::new(Mutex::new(Vec::new()));
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: document_sources.clone(),
        });

        let conv1 = state.create_project("ConvOne").unwrap();
        let conv1_paths = vec![
            write_staged_file(tmp.path(), "c1-a.md", "Contenido A.\n"),
            write_staged_file(tmp.path(), "c1-b.md", "Contenido B.\n"),
        ];
        let conv1_import = import_and_index(&state, &conv1.id, &conv1_paths);
        assert_eq!(conv1_import.len(), 2);

        let conv2 = state.create_project("ConvTwo").unwrap();
        let conv2_paths = vec![
            write_staged_file(tmp.path(), "c2-c.md", "Contenido C.\n"),
            write_staged_file(tmp.path(), "c2-d.md", "Contenido D.\n"),
        ];
        let conv2_import = import_and_index(&state, &conv2.id, &conv2_paths);
        assert_eq!(conv2_import.len(), 2);

        let conv1_active = active_material_ids(&state, &conv1.id);
        let conv2_active = active_material_ids(&state, &conv2.id);
        assert_eq!(conv1_active.len(), 2);
        assert_eq!(conv2_active.len(), 2);
        assert_ne!(conv1_active, conv2_active);

        state.set_test_classifier(CountingClassifier {
            calls: Arc::new(Mutex::new(0)),
            intent: crate::intent::Intent::PerSourceSummary,
            reason: crate::intent::ReasonCode::SemanticClassifier,
        });
        // No-attachment deep PerSourceSummary in conversation 1 resolves only [A1,A2].
        let turn1 = run_staged_turn(&state, &conv1.id, "Analyse chaque document en détail.", &[]);
        assert_eq!(turn1.status, "completed");
        let surface1 = turn1.message.expect("surface");
        assert!(surface1.contains("c1-a.md"));
        assert!(surface1.contains("c1-b.md"));

        // No-attachment deep PerSourceSummary in conversation 2 resolves only [B1,B2].
        let turn2 = run_staged_turn(&state, &conv2.id, "Analyse chaque document en détail.", &[]);
        assert_eq!(turn2.status, "completed");
        let surface2 = turn2.message.expect("surface");
        assert!(surface2.contains("c2-c.md"));
        assert!(surface2.contains("c2-d.md"));

        // The recorder observed exactly conversation 1's two sources then
        // conversation 2's two sources — no cross-conversation leakage.
        {
            let seen = document_sources.lock().unwrap();
            assert_eq!(
                seen.as_slice(),
                ["c1-a.md", "c1-b.md", "c2-c.md", "c2-d.md"]
            );
        }
    }

    /// A failed/empty later import must never erase a valid active set.
    #[test]
    fn failed_or_empty_import_does_not_erase_the_active_set() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        let project = state.create_project("FailedImportKeepsActive").unwrap();
        let paths = (0..4)
            .map(|index| {
                write_staged_file(
                    tmp.path(),
                    &format!("keep-{index}.md"),
                    &format!("Contenido {index}.\n"),
                )
            })
            .collect::<Vec<_>>();
        run_staged_turn(
            &state,
            &project.id,
            "resumime cada archivo por separado",
            &paths,
        );
        let before = active_material_ids(&state, &project.id);
        assert_eq!(before.len(), 4);
        // A later import that accepts zero usable materials (a missing path).
        let missing = tmp.path().join("does-not-exist.md");
        let failed = state
            .send_staged_message_persist(
                &project.id,
                "subí este archivo",
                &[missing.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        assert!(failed.material_ids.is_empty());

        // A no-attachment turn must also leave the active set untouched.
        let _empty = state
            .send_staged_message_persist(&project.id, "hola", &[], &[])
            .unwrap();

        let after = active_material_ids(&state, &project.id);
        assert_eq!(
            after, before,
            "failed/empty imports must not erase the active set"
        );
    }

    /// A later import with mixed outcomes replaces the active set with only the
    /// materials it actually accepted: a failed item contributes no material id,
    /// so [A,B,C,D] followed by {successful E, one failed item} yields [E].
    /// This documents the current accepted-material semantics — the active set
    /// is the last *successful* explicit attachment set, replaced wholesale.
    #[test]
    fn partial_import_replaces_active_set_with_only_successful_materials() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        state.set_summarizer(SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        });
        let project = state.create_project("PartialImport").unwrap();

        let old_paths = (0..4)
            .map(|index| {
                write_staged_file(
                    tmp.path(),
                    &format!("old-{index}.md"),
                    &format!("Contenido {index}.\n"),
                )
            })
            .collect::<Vec<_>>();
        let imported = import_and_index(&state, &project.id, &old_paths);
        assert_eq!(imported.len(), 4);
        assert_eq!(active_material_ids(&state, &project.id).len(), 4);

        let good = write_staged_file(tmp.path(), "only-e.md", "Contenido E.\n");
        let missing = tmp.path().join("does-not-exist.md");
        let partial = state
            .send_staged_message_persist(
                &project.id,
                "subí estos archivos",
                &[good, missing.to_string_lossy().to_string()],
                &[],
            )
            .unwrap();
        // Only the successful material is accepted; the failed item adds no id.
        assert_eq!(partial.material_ids.len(), 1);
        assert_eq!(
            active_material_ids(&state, &project.id),
            partial.material_ids,
            "a partial import replaces the active set with only the successful material"
        );
    }

    /// Defense-in-depth: a direct `send_summary_run_with` call with
    /// `SelectedPerSource` and an empty selected set produces the truthful
    /// no-selection answer and never runs K6.
    #[test]
    fn send_summary_run_with_empty_selected_per_source_never_reaches_k6() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        let tmp = tempfile::tempdir().unwrap();
        let state = staged_state(tmp.path());
        let project = state.create_project("EmptyDirect").unwrap();
        let mut inputs = state
            .send_message_persist(&project.id, "hola", &[])
            .unwrap();
        inputs.selected_material_ids = Vec::new();
        let summarizer = SelectedSourceRecorder {
            calls: Arc::new(Mutex::new(0)),
            document_sources: Arc::new(Mutex::new(Vec::new())),
        };
        let k6_before = state.test_activity().k6_calls;
        let run = state
            .send_summary_run_with(
                inputs,
                &summarizer,
                crate::intent::SummaryExecutionKind::SelectedPerSource,
            )
            .unwrap();
        assert_eq!(run.status, "completed");
        assert!(
            run.message
                .expect("no-selection answer")
                .contains("No tengo archivos seleccionados para resumir")
        );
        assert_eq!(
            state.test_activity().k6_calls,
            k6_before,
            "empty selected-per-source must never reach K6"
        );
    }
}

#[cfg(test)]
mod creation_from_material_tests {
    use super::*;
    use project_agent::FakeAgentEngine;
    use project_agent::model::{Artifact, ArtifactKind};
    use project_knowledge::{
        EmbeddingGeneration, EmbeddingProvider, KnowledgeStore, ModelManifest,
    };
    use project_provider::{FakeProviderConnector, FakeRestarter, ModelSummary, ProviderDetail};
    use project_tunnel::FakeTunnel;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

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

    /// Deterministic embedding provider that counts query vs passage calls so
    /// the test can prove embeddings are created only during ingestion and the
    /// creation routing itself never re-embeds.
    struct CountingEmbeddings {
        generation: EmbeddingGeneration,
        query_calls: Arc<Mutex<usize>>,
        passage_calls: Arc<Mutex<usize>>,
    }
    impl EmbeddingProvider for CountingEmbeddings {
        fn generation(&self) -> &EmbeddingGeneration {
            &self.generation
        }
        fn embed_query(&mut self, _query: &str) -> project_knowledge::Result<Vec<f32>> {
            *self.query_calls.lock().unwrap() += 1;
            Ok(vec![0.0; 384])
        }
        fn embed_passages(
            &mut self,
            passages: &[String],
        ) -> project_knowledge::Result<Vec<Vec<f32>>> {
            *self.passage_calls.lock().unwrap() += 1;
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

    /// A README large enough to produce several chunks, with a distinctive
    /// sentinel in the LAST chunk so the test can prove document-wide
    /// representative coverage (a tiny semantic top-K slice would never reach
    /// the end of a large document).
    fn large_readme() -> String {
        let mut body = String::from(
            "# EducAI\n\nLa plataforma permite a una docente generar materiales educativos a partir de sus propios archivos.\n\n",
        );
        for index in 0..60 {
            body.push_str(&format!(
                "# Sección {index}\n\nLa sección {index} describe un tema educativo de ejemplo para que el documento sea extenso y se fragmenten muchos fragmentos de contenido con suficiente longitud textual como para superar el límite de fragmentación.\n\n"
            ));
        }
        body.push_str(
            "SENTINEL_FINAL_DEL_DOCUMENTO: la presentación debe cubrir también el cierre del material.\n",
        );
        body
    }

    fn staged_readme(base: &std::path::Path, name: &str, body: &str) -> Vec<String> {
        let corpus = base.join("corpus");
        std::fs::create_dir_all(&corpus).unwrap();
        let path = corpus.join(name);
        std::fs::write(&path, body).unwrap();
        vec![path.to_string_lossy().to_string()]
    }

    /// Places the artifact the fake engine claims it created so the registrar
    /// can read it back (mirrors the existing integration-test pattern).
    fn write_workspace_artifact(base: &std::path::Path, project_id: &str, rel: &str, bytes: &[u8]) {
        let path = base
            .join("projects")
            .join(project_id)
            .join("workspace")
            .join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }

    /// The critical staged production seam (Test O): attach README.md and ask
    /// "me podes armar una presentacion interactiva para presentar esto?".
    /// The turn must resolve README.md, run the creation path AFTER ingestion
    /// (no second embedding generation), register a real artifact, report
    /// `turn_kind=creation_from_material`, `reason=creation_from_material`,
    /// and `retrieval_mode=None` (never `hybrid`).
    #[test]
    fn staged_creation_from_material_uses_the_production_seam() {
        let _session_log_guard = crate::session_log::test_guard();
        crate::session_log::clear();
        crate::session_log::configure_from_args(["--debug".to_owned()]);
        let tmp = tempfile::tempdir().unwrap();
        let inner = FakeAgentEngine::new();
        inner.set_message(
            "Listo. Creé la presentación interactiva a partir del archivo README.md.".to_owned(),
        );
        inner.set_artifacts(vec![Artifact {
            path: "workspace/index.html".to_owned(),
            kind: ArtifactKind::Web,
            byte_size: 1,
            sha256: None,
        }]);
        let mut state = AppState::with_components(
            tmp.path().to_path_buf(),
            inner.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.summarizer_backend = Some(Arc::new(OpenCodeBackend::new(
            PathBuf::from("/usr/bin/true"),
            tmp.path().join("oc-config"),
            0,
        )));
        let query_calls = Arc::new(Mutex::new(0));
        let passage_calls = Arc::new(Mutex::new(0));
        state.set_local_embedding_provider(CountingEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
            query_calls: query_calls.clone(),
            passage_calls: passage_calls.clone(),
        });
        let project = state.create_project("P").unwrap();

        // Pre-ingestion: no embeddings yet.
        assert_eq!(*query_calls.lock().unwrap(), 0);
        assert_eq!(*passage_calls.lock().unwrap(), 0);

        let paths = staged_readme(tmp.path(), "README.md", &large_readme());
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "me podes armar una presentacion interactiva para presentar esto?",
                &paths,
                &[],
            )
            .unwrap();
        write_workspace_artifact(tmp.path(), &project.id, "index.html", b"<html></html>");
        let run = state.run_accepted_staged_turn(accepted).unwrap();
        assert_eq!(run.status, "completed");
        assert!(
            !run.registered_creation_ids.is_empty(),
            "a creation-from-material turn must register an artifact"
        );

        // README becomes READY.
        let store = KnowledgeStore::open(
            tmp.path().join("projects").join(&project.id),
            &ProjectId::parse(&project.id).unwrap(),
        )
        .unwrap();
        let material_ids = store.ready_source_names().unwrap();
        assert_eq!(material_ids, vec!["README.md".to_owned()]);
        let corpus = store.corpus_stats().unwrap();
        assert!(
            corpus.chunks_total >= 10,
            "large README must produce many chunks"
        );

        // Embeddings were generated only during ingestion; the creation
        // routing itself made zero query-embedding calls and zero new passage
        // embedding rounds beyond ingestion.
        assert_eq!(*query_calls.lock().unwrap(), 0);
        assert!(
            *passage_calls.lock().unwrap() >= 1,
            "ingestion embedded the README chunks"
        );
        let activity = state.test_activity();
        assert_eq!(
            activity.embedding_inference, 1,
            "only the ingestion embedding round"
        );
        assert_eq!(
            activity.retrieval, 0,
            "creation routing must never run top-K retrieval"
        );

        // Durable turn metrics: creation_from_material, retrieval_mode == None.
        let metrics = state.last_turn_metrics(&project.id).unwrap().unwrap();
        assert_eq!(metrics.turn_kind.as_deref(), Some("creation_from_material"));
        assert_eq!(
            metrics.local_mode.as_deref(),
            Some("creation_from_material")
        );
        assert_eq!(
            metrics.retrieval_mode, None,
            "creation action is not a retrieval mode"
        );

        // Usage log reason is creation_from_material.
        let usages = crate::session_log::list();
        assert!(
            usages
                .iter()
                .filter_map(|entry| entry.usage.as_ref())
                .any(|usage| usage.conversation_id == project.id
                    && usage.reason == "creation_from_material"),
            "usage log must record reason=creation_from_material"
        );

        // The serialized prompt grounds the creation on the READY material and
        // neutralizes the empty-filesystem false signal; the raw attachment is
        // not duplicated into the workspace.
        let prompt = inner.last_prompt_text().unwrap_or_default();
        assert!(prompt.contains("<knowledge_evidence"), "{prompt}");
        assert!(
            prompt.contains("material already available in Knowledge"),
            "creation directive must be serialized: {prompt}"
        );
        assert!(
            prompt.contains("empty filesystem does not mean there is no source material"),
            "anti-empty-directory directive must be serialized: {prompt}"
        );
        assert!(
            prompt.contains("README.md"),
            "exact target source name must reach the model: {prompt}"
        );
        assert!(
            !prompt.contains("materials/1-README.md"),
            "indexed target must not be raw-forwarded into workspace materials: {prompt}"
        );
        assert!(
            prompt.contains("SENTINEL_FINAL_DEL_DOCUMENTO"),
            "document-wide representative coverage must reach the model"
        );
    }

    /// Deterministic provider with no counters, used for the context-quality
    /// test that only inspects the serialized prompt.
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

    /// Context-quality proof (Test P): the creation path is grounded on a
    /// document-wide representative (or persisted summary), never on a tiny
    /// semantic top-K slice (~128 chars / 2 chunks).
    #[test]
    fn creation_context_is_document_wide_not_tiny_topk() {
        let tmp = tempfile::tempdir().unwrap();
        let inner = FakeAgentEngine::new();
        inner.set_message("Listo.".to_owned());
        inner.set_artifacts(vec![Artifact {
            path: "workspace/index.html".to_owned(),
            kind: ArtifactKind::Web,
            byte_size: 1,
            sha256: None,
        }]);
        let state = AppState::with_components(
            tmp.path().to_path_buf(),
            inner.clone(),
            FakeTunnel::new(),
            connector(),
            FakeRestarter::new(),
        );
        state.set_local_embedding_provider(DeterministicEmbeddings {
            generation: EmbeddingGeneration::from(ModelManifest::embedded().unwrap().active()),
        });
        let project = state.create_project("P").unwrap();
        let paths = staged_readme(tmp.path(), "README.md", &large_readme());
        let accepted = state
            .send_staged_message_persist(
                &project.id,
                "generame una página web basada en este README",
                &paths,
                &[],
            )
            .unwrap();
        write_workspace_artifact(tmp.path(), &project.id, "index.html", b"<html></html>");
        let run = state.run_accepted_staged_turn(accepted).unwrap();
        assert_eq!(run.status, "completed");
        assert!(!run.registered_creation_ids.is_empty());

        let prompt = inner.last_prompt_text().unwrap_or_default();
        let evidence_start = prompt.find("<knowledge_evidence").unwrap_or(0);
        let evidence_end = prompt.find("</knowledge_evidence>").unwrap_or(prompt.len());
        let evidence_chars = prompt[evidence_start..evidence_end].chars().count();
        assert!(
            evidence_chars > 2_000,
            "creation context must be document-wide, not a tiny top-k slice (~128 chars): {evidence_chars} chars"
        );
        // The end-of-document sentinel proves the coverage spans the document.
        assert!(
            prompt.contains("SENTINEL_FINAL_DEL_DOCUMENTO"),
            "document-wide representative coverage must include the document end"
        );
        // The raw attachment is never duplicated into the workspace.
        assert!(!prompt.contains("materials/1-README.md"), "{prompt}");
    }
}
