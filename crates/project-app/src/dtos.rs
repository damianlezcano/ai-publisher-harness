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
