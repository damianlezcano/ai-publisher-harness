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
    /// Public URL when `state` is `published`; always runtime-only and never
    /// persisted.
    pub public_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageView {
    pub id: String,
    pub role: String,
    pub text: String,
    pub status: String,
    pub created_at: String,
    pub material_ids: Vec<String>,
    pub creation_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptedImportProgressView {
    pub state: String,
    pub total: usize,
    pub copied: usize,
    pub lexical_completed: usize,
    pub embedding_completed: usize,
    pub failed: usize,
    pub embeddings_created: usize,
    pub embeddings_reused: usize,
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
