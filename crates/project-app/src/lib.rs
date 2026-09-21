//! Application core facade for the desktop app (M6).
//!
//! Tauri-free: this crate wires M1-M5 into a UI-oriented API over serializable
//! DTOs and human-facing errors. The Tauri shell (`app/src-tauri`) is a thin
//! adapter over [`AppState`].

#![forbid(unsafe_code)]

pub mod app;
pub mod classifier;
pub mod conversation_context;
pub mod creation;
pub mod dtos;
pub mod error;
pub mod eval;
pub mod intent;
pub mod per_item;
pub mod referent;
pub mod retrieval_intent;
pub mod session_contract;
pub mod session_log;
pub mod sidecar;
pub mod summarize;

pub use app::{
    APP_VERSION, AcceptedStagedTurn, AppConfig, AppState, SharedBackendRestarter, StagedImage,
};
pub use classifier::opencode::{ClassifierObservation, OpenCodeIntentClassifier};
pub use classifier::{
    ClassifierFallbackReason, ClassifierInput, ClassifierProvenance, DEFAULT_MIN_CONFIDENCE,
    DETERMINISTIC_ADAPTER_NAME, DeterministicAdapter, IntentClassificationError, IntentClassifier,
    SemanticIntentClassifier, knowledge_available, knowledge_cue_for_turn, knowledge_may_apply,
    should_classify,
};
pub use creation::{
    CREATION_LOCAL_MODE, CREATION_REASON, CreationRequest, CreationResolution, CreationRunMeta,
    CreationTargetCue, detect_creation_intent, resolve_creation_targets,
};
pub use dtos::{
    AcceptedImportProgressView, AgentRunView, AppStatusView, ConversationUsageTotalsView,
    CreationView, MaterialAddImageView, MaterialImportResult, MaterialView, MaterialsImportReport,
    MessageView, PreviewData, ProjectSummary, ProjectSummaryAnswerView, ProjectView,
    PublicationView, SelectedModelView, StagedAttachmentView, StagedAttachmentsReport,
    SummarizationReportView, SummaryContentView, SummaryItemView, SummaryNodeView, TurnMetricsView,
    WebPreview,
};
pub use error::{AppError, AppResult, ErrorCode};
pub use intent::{
    BoundRoute, ClassifierDecision, Intent, IntentModifier, KnowledgeRoutingContext,
    PerSourceScope, PriorReferentKind, ReasonCode, SummaryExecutionKind, from_retrieval_intent,
    from_summary_intent, resolve_intent, resolve_intent_with, resolve_per_source_scope,
    routing_telemetry,
};
pub use referent::{ContextualFollowUp, FollowUpAction, PriorReferent, resolve_followup};
pub use retrieval_intent::{
    InventoryAction, InventoryRequest, InventorySort, RetrievalIntent, contains_presence_cue,
    detect_retrieval_intent, extract_inventory_membership_target, extract_presence_terms,
    has_open_question_lead, inventory_request, inventory_request_or_list, retrieval_mode_for,
};
pub use session_log::SessionLogEntry;
pub use summarize::OpenCodeRemoteSummarizer;
pub use summarize::{SummaryIntent, detect_summarize_intent, detect_summary_intent};
