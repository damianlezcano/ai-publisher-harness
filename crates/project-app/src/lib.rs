//! Application core facade for the desktop app (M6).
//!
//! Tauri-free: this crate wires M1-M5 into a UI-oriented API over serializable
//! DTOs and human-facing errors. The Tauri shell (`app/src-tauri`) is a thin
//! adapter over [`AppState`].

#![forbid(unsafe_code)]

pub mod app;
pub mod dtos;
pub mod error;
pub mod retrieval_intent;
pub mod session_log;
pub mod sidecar;
pub mod summarize;

pub use app::{
    APP_VERSION, AcceptedStagedTurn, AppConfig, AppState, SharedBackendRestarter, StagedImage,
};
pub use dtos::{
    AcceptedImportProgressView, AgentRunView, AppStatusView, CreationView, MaterialAddImageView,
    MaterialImportResult, MaterialView, MaterialsImportReport, MessageView, PreviewData,
    ProjectSummary, ProjectSummaryAnswerView, ProjectView, PublicationView, SelectedModelView,
    StagedAttachmentView, StagedAttachmentsReport, SummarizationReportView, SummaryContentView,
    SummaryItemView, SummaryNodeView, TurnMetricsView, WebPreview,
};
pub use error::{AppError, AppResult, ErrorCode};
pub use retrieval_intent::{
    RetrievalIntent, detect_retrieval_intent, extract_presence_terms, retrieval_mode_for,
};
pub use session_log::SessionLogEntry;
pub use summarize::OpenCodeRemoteSummarizer;
pub use summarize::{SummaryIntent, detect_summarize_intent, detect_summary_intent};
