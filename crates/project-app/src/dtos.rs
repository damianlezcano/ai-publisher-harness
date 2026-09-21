//! UI-oriented data transfer objects exposed by the application facade.
//!
//! These are stable, serializable product concepts (Proyecto, Material,
//! Creación, Publicación), never internal infrastructure. IDs are present for
//! command addressing but are presentation-neutral; the frontend decides what
//! to render and must never interpret paths, hashes, or runtime state from
//! these values.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub shared: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialView {
    pub id: String,
    pub display_name: String,
    pub original_file_name: String,
    /// Stable kind code derived from the file name: `pdf`, `image`, `document`,
    /// `spreadsheet`, `presentation`, `text`, or `other`.
    pub kind: String,
    pub byte_size: u64,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreationView {
    pub id: String,
    pub display_name: String,
    /// `web`, `document`, `image`, or `file`.
    pub kind: String,
    /// `public` ("Se compartirá") or `private` ("Privado").
    pub visibility: String,
    pub byte_size: u64,
    pub created_at: String,
    pub revision: u32,
    /// Stable lineage identity of this logical interactive resource. Every
    /// version of the same resource shares this id.
    pub lineage_id: String,
    /// 1-based version number inside the lineage.
    pub version_number: u32,
    /// The version this one derives from, when it is not the first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_version_id: Option<String>,
    /// Whether this version is the authoritative current one of its lineage.
    pub is_current: bool,
    /// Every version id of this lineage, for exact-version addressing.
    pub available_version_ids: Vec<String>,
}

/// Deterministic per-file result for a multi-file import batch (M8 §5).
///
/// One entry per input, in input order; partial failure is explicit and a bad
/// file never aborts the rest of the batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialImportResult {
    /// Sanitized base name only; never a full path.
    pub source_name: String,
    /// `added`, `duplicate`, `unsupported`, or `failed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_id: Option<String>,
    /// Human message for `unsupported`/`failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<MaterialView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialsImportReport {
    pub items: Vec<MaterialImportResult>,
}

/// A frontend-owned, pre-acceptance attachment selection. This is deliberately
/// not a Material: it contains no project-owned copy, Material ID, or Knowledge
/// record. `source_name` is sanitized and `status` is limited to staging
/// validation/duplicate information so the composer can remain truthful before
/// the user commits the turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedAttachmentView {
    pub source_name: String,
    /// `ready`, `duplicate_in_selection`, `unsupported`, or `failed`.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedAttachmentsReport {
    pub items: Vec<StagedAttachmentView>,
}

/// Result of a clipboard image paste. `duplicate` is true when the same bytes
/// were already a project material (M8 §4): the existing material is returned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialAddImageView {
    pub material: MaterialView,
    pub duplicate: bool,
}

/// In-app preview bytes for images and text/Markdown (M8 §10). Never a path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewData {
    pub content_type: String,
    pub data_base64: String,
}

/// Endpoint for the isolated web preview (M8 §11). `url` is a loopback-only,
/// token-guarded URL created backend-side; `token` allows teardown.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebPreview {
    pub url: String,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationView {
    /// `local` or `published`.
    pub state: String,
    /// Authoritative share URL when `state` is `published`. For a shared web
    /// lineage this is the immutable current-version URL
    /// (`/<slug>/<current-version-id>/`); for non-web publications it is the
    /// route root. Copy / Open / QR all consume exactly this value. Always
    /// runtime-only and never persisted.
    pub public_url: Option<String>,
    /// Version-history landing page URL (`/<slug>/`).
    pub root_url: Option<String>,
    /// Current-version alias URL (`/<slug>/latest/`).
    pub latest_url: Option<String>,
    /// The immutable version id the share URL currently targets; `None` when
    /// the share URL is the route root (non-web publications).
    pub current_version_id: Option<String>,
}

/// Turn metrics exposed to the UI. All numeric fields are Option so that
/// absent provider data serializes as `null` (rendered "No disponible")
/// rather than an invented zero.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnMetricsView {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub turn_duration_ms: Option<u64>,
    pub source: Option<String>,
    pub remote_calls: Option<usize>,
    // Knowledge structural metrics
    pub material_count: Option<usize>,
    pub corpus_bytes: Option<u64>,
    pub corpus_utf8_chars: Option<usize>,
    pub corpus_est_tokens: Option<usize>,
    pub retrieval_candidate_count: Option<usize>,
    pub selected_evidence_count: Option<usize>,
    pub selected_evidence_bytes: Option<usize>,
    pub selected_evidence_utf8_chars: Option<usize>,
    pub evidence_est_tokens: Option<usize>,
    pub context_reduction_pct: Option<usize>,
    pub semantic_provider_state: Option<String>,
    pub request_preparation_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible_materials: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials_inspected: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chunks_inspected: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exhaustive_coverage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lexical_hits: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_hits: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contextual_followup: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referent_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub referent_count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_kind: Option<String>,
    /// Exact grounded source display names for this turn. Never paths.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_names: Vec<String>,
}

/// Durable additive provider telemetry across completed turns in one
/// conversation. Provider/model are the latest reported identity; numeric
/// fields are sums only when every provider-using turn reported that field.
/// It intentionally contains no retrieval/corpus fields because those are
/// turn-context snapshots, not additive quantities.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationUsageTotalsView {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub turn_duration_ms: Option<u64>,
    pub source: Option<String>,
    pub remote_calls: Option<usize>,
}

impl From<project_core::TurnMetrics> for TurnMetricsView {
    fn from(m: project_core::TurnMetrics) -> Self {
        Self {
            provider: m.provider,
            model: m.model,
            input_tokens: m.input_tokens,
            output_tokens: m.output_tokens,
            cache_read_tokens: m.cache_read_tokens,
            cache_write_tokens: m.cache_write_tokens,
            total_tokens: m.total_tokens,
            cost_usd: m.cost_usd,
            turn_duration_ms: m.turn_duration_ms,
            source: m.source,
            remote_calls: m.remote_calls,
            material_count: m.material_count,
            corpus_bytes: m.corpus_bytes,
            corpus_utf8_chars: m.corpus_utf8_chars,
            corpus_est_tokens: m.corpus_est_tokens,
            retrieval_candidate_count: m.retrieval_candidate_count,
            selected_evidence_count: m.selected_evidence_count,
            selected_evidence_bytes: m.selected_evidence_bytes,
            selected_evidence_utf8_chars: m.selected_evidence_utf8_chars,
            evidence_est_tokens: m.evidence_est_tokens,
            context_reduction_pct: m.context_reduction_pct,
            semantic_provider_state: m.semantic_provider_state,
            request_preparation_ms: m.request_preparation_ms,
            retrieval_mode: m.retrieval_mode,
            eligible_materials: m.eligible_materials,
            materials_inspected: m.materials_inspected,
            chunks_inspected: m.chunks_inspected,
            exhaustive_coverage: m.exhaustive_coverage,
            lexical_hits: m.lexical_hits,
            semantic_hits: m.semantic_hits,
            local_mode: m.local_mode,
            contextual_followup: m.contextual_followup,
            referent_type: m.referent_type,
            referent_count: m.referent_count,
            origin_turn_id: m.origin_turn_id,
            base_intent: m.base_intent,
            turn_kind: m.turn_kind,
            source_names: m.source_names,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: String,
    pub role: String,
    pub text: String,
    pub status: String,
    pub created_at: String,
    pub material_ids: Vec<String>,
    pub creation_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_metrics: Option<TurnMetricsView>,
    /// Durable id of the owning user turn (the user message id), present on
    /// assistant messages. Legacy records lack this field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: String,
    pub name: String,
    pub materials: Vec<MaterialView>,
    pub creations: Vec<CreationView>,
    pub messages: Vec<MessageView>,
    pub publication: PublicationView,
    pub model: Option<ConversationModelView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accepted_import: Option<AcceptedImportProgressView>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedImportProgressView {
    /// Opaque identity of the durable accepted-import operation. This is how
    /// the frontend addresses an explicit recovery/resume command; it never
    /// encodes a path, source, or storage detail.
    pub operation_id: String,
    pub state: String,
    /// The remote agent boundary, distinct from local progress:
    /// `not_started`, `started_outcome_unknown`, `completed`,
    /// `failed_retryable`, or `failed_terminal`.
    pub agent_state: String,
    /// True when a durable K6 operation for this turn is `retry_required` or
    /// retryable `failed`. The UI uses this for an explicit retry action, never
    /// for ordinary resume.
    pub summary_retryable: bool,
    pub total: usize,
    pub copied: usize,
    pub lexical_completed: usize,
    pub embedding_completed: usize,
    pub failed: usize,
    pub embeddings_created: usize,
    pub embeddings_reused: usize,
    /// Fully usable materials of this accepted operation for the currently
    /// active Knowledge embedding generation: lexically ready AND every
    /// reachable chunk has a ready embedding for the active generation (zero
    /// chunk materials count once lexically ready). Never a lexical-only count.
    pub materials_ready: usize,
    /// Corpus chunks reachable from this operation (0 until the embedding
    /// phase resolves them). Counts only, never content.
    pub chunks_total: usize,
    /// Chunks that still need an embedding for the active generation (0 until
    /// the embedding phase resolves them). The embedding progress denominator.
    pub embeddings_total: usize,
    /// Wall-clock milliseconds since the operation was created. Structural,
    /// never a misleading ETA.
    pub elapsed_ms: u64,
    /// Smoothed embeddings-per-second over the embedding phase, once a stable
    /// sample exists. `None` while there is no trustworthy rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput_embeddings_per_sec: Option<f64>,
    /// True while the turn's post-embedding compact/generic per-item summary
    /// synthesis is running. The frontend uses this to replace the misleading
    /// "99% · N de N archivos listos" import line with a truthful synthesis
    /// phase. Never set for the K6 deep route.
    pub synthesizing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationModelView {
    pub provider_id: String,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunView {
    /// `completed`, `failed`, or `cancelled`.
    pub status: String,
    /// The persisted user message that owns this execution, when the run was
    /// initiated through the durable chat-send path.
    pub turn_id: Option<String>,
    pub registered_creation_ids: Vec<String>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatusView {
    pub version: String,
    /// `stopped`, `starting`, `ready`, or `failed`.
    pub agent: String,
}

/// The resolved global model selection (design §12/§13). `requiresChoice` is
/// true when the stored model disappeared and only paid/unavailable models
/// remain: the frontend must show `notice` and require an explicit choice
/// (never an automatic paid/provider switch).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedModelView {
    pub model: project_provider::models::ModelSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    pub requires_choice: bool,
}

/// One structured summary finding, grounded in evidence labels (E1.. or P1..).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryItemView {
    pub text: String,
    pub evidence: Vec<String>,
}

/// The structured summary content (the same validated contract K6 persists).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryContentView {
    pub summary: String,
    pub topics: Vec<SummaryItemView>,
    pub decisions: Vec<SummaryItemView>,
    pub action_items: Vec<SummaryItemView>,
    pub questions: Vec<SummaryItemView>,
}

/// One durable summary node exposed to the UI. No chunk text, prompt, or path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryNodeView {
    pub summary_id: String,
    /// `document`, `batch`, or `global`.
    pub level: String,
    /// `pending`, `ready`, `failed`, or `stale`.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<SummaryContentView>,
    pub source_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_summary_id: Option<String>,
}

/// Result of one summarization run: accounting only, never summary content.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummarizationReportView {
    pub remote_calls: usize,
    pub estimated_input_units: usize,
    pub cache_hits: usize,
    pub reused: usize,
    pub regenerated: usize,
    pub source_count: usize,
    pub hierarchy_depth: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub provider_usage_actual: bool,
}

/// Whole-project summary answer: K6 accounting plus the user-facing global
/// summary text (or `None` when the project has no indexed sources).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummaryAnswerView {
    pub report: SummarizationReportView,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_summary: Option<String>,
    /// Conservative estimate of the whole corpus size (see K4 estimator shape),
    /// used only to report an estimated reduction against naive full-corpus
    /// context. Never a provider tokenizer claim.
    pub naive_corpus_est_tokens: usize,
}

impl ProjectSummaryAnswerView {
    /// The text to surface to the user, or `None` when there is nothing to say.
    pub fn summarize_surface_text(&self) -> Option<String> {
        self.global_summary
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    }

    /// Estimated context reduction versus naive full-corpus context (percent,
    /// 0..=100). A comparison estimate, not a claim of exact tokens saved.
    pub fn reduction_pct_vs_naive_full_corpus(&self) -> usize {
        if self.naive_corpus_est_tokens == 0 {
            return 100;
        }
        let saved = self
            .naive_corpus_est_tokens
            .saturating_sub(self.report.estimated_input_units);
        saved * 100 / self.naive_corpus_est_tokens
    }
}
