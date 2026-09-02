//! Application core facade for the desktop app (M6).
//!
//! Tauri-free: this crate wires M1-M5 into a UI-oriented API over serializable
//! DTOs and human-facing errors. The Tauri shell (`app/src-tauri`) is a thin
//! adapter over [`AppState`].

#![forbid(unsafe_code)]

pub mod app;
pub mod dtos;
pub mod error;
pub mod session_log;
pub mod sidecar;

pub use app::{APP_VERSION, AppConfig, AppState, SharedBackendRestarter};
pub use dtos::{
    AgentRunView, AppStatusView, CreationView, MaterialAddImageView, MaterialImportResult,
    MaterialView, MaterialsImportReport, MessageView, PreviewData, ProjectSummary, ProjectView,
    PublicationView, SelectedModelView, WebPreview,
};
pub use error::{AppError, AppResult, ErrorCode};
pub use session_log::SessionLogEntry;
