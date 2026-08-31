//! Application error model: typed codes plus human-facing messages.
//!
//! The frontend receives only `code` + `message`; raw backend errors, debug
//! strings, stack traces, HTTP codes, and filesystem paths are never surfaced.

use std::fmt;

use project_agent::AgentError;
use project_core::ProjectCoreError;
use project_provider::ProviderError;
use project_publication::PublicationError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    NotFound,
    InvalidInput,
    Conflict,
    AiUnavailable,
    AiTaskFailed,
    PublishFailed,
    MaterialFailed,
    MaterialUnsupported,
    MaterialTooLarge,
    MaterialImageInvalid,
    PreviewUnavailable,
    PreviewTooLarge,
    AttachmentInvalid,
    OpenFailed,
    StorageUnavailable,
    ProviderNotFound,
    ProviderConnectFailed,
    CredentialInvalid,
    CredentialRevoked,
    ProviderUnavailable,
    ModelUnavailable,
    NoCompatibleModel,
    NetworkError,
    Internal,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::NotFound => "not_found",
            ErrorCode::InvalidInput => "invalid_input",
            ErrorCode::Conflict => "conflict",
            ErrorCode::AiUnavailable => "ai_unavailable",
            ErrorCode::AiTaskFailed => "ai_task_failed",
            ErrorCode::PublishFailed => "publish_failed",
            ErrorCode::MaterialFailed => "material_failed",
            ErrorCode::MaterialUnsupported => "material_unsupported",
            ErrorCode::MaterialTooLarge => "material_too_large",
            ErrorCode::MaterialImageInvalid => "material_image_invalid",
            ErrorCode::PreviewUnavailable => "preview_unavailable",
            ErrorCode::PreviewTooLarge => "preview_too_large",
            ErrorCode::AttachmentInvalid => "attachment_invalid",
            ErrorCode::OpenFailed => "open_failed",
            ErrorCode::StorageUnavailable => "storage_unavailable",
            ErrorCode::ProviderNotFound => "provider_not_found",
            ErrorCode::ProviderConnectFailed => "provider_connect_failed",
            ErrorCode::CredentialInvalid => "credential_invalid",
            ErrorCode::CredentialRevoked => "credential_revoked",
            ErrorCode::ProviderUnavailable => "provider_unavailable",
            ErrorCode::ModelUnavailable => "model_unavailable",
            ErrorCode::NoCompatibleModel => "no_compatible_model",
            ErrorCode::NetworkError => "network_error",
            ErrorCode::Internal => "internal",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// Contextual mapping for project CRUD operations.
    pub fn from_core(error: ProjectCoreError) -> Self {
        match error {
            ProjectCoreError::NotFound(_) => {
                Self::new(ErrorCode::NotFound, "No se encontró el proyecto.")
            }
            ProjectCoreError::AlreadyExists(_) => {
                Self::new(ErrorCode::Conflict, "Ya existe un proyecto con ese nombre.")
            }
            ProjectCoreError::InvalidName(_)
            | ProjectCoreError::InvalidId { .. }
            | ProjectCoreError::InvalidPath(_)
            | ProjectCoreError::InvalidTimestamp(_)
            | ProjectCoreError::InvalidContentType(_)
            | ProjectCoreError::InvalidDigest(_)
            | ProjectCoreError::InvalidCreation(_)
            | ProjectCoreError::InvalidPublicationRoute(_) => {
                Self::new(ErrorCode::InvalidInput, "Ese valor no es válido.")
            }
            ProjectCoreError::Conflict { .. } => Self::new(
                ErrorCode::Conflict,
                "El proyecto se modificó en otro lugar. Inténtalo de nuevo.",
            ),
            ProjectCoreError::CorruptMetadata(_) | ProjectCoreError::UnsupportedSchema(_) => {
                Self::new(
                    ErrorCode::StorageUnavailable,
                    "Los datos del proyecto no se pudieron leer.",
                )
            }
            _ => Self::new(ErrorCode::Internal, "No se pudo completar la operación."),
        }
    }

    /// Contextual mapping for material ingestion (drop/picker).
    pub fn from_material(error: ProjectCoreError) -> Self {
        match error {
            ProjectCoreError::NotFound(_) => {
                Self::new(ErrorCode::NotFound, "No se encontró el proyecto.")
            }
            ProjectCoreError::InvalidName(_)
            | ProjectCoreError::InvalidId { .. }
            | ProjectCoreError::InvalidPath(_) => {
                Self::new(ErrorCode::InvalidInput, "Ese archivo no es válido.")
            }
            ProjectCoreError::MissingMaterial(_) => {
                Self::new(ErrorCode::NotFound, "No se encontró ese material.")
            }
            _ => Self::new(ErrorCode::MaterialFailed, "No pudimos agregar ese archivo."),
        }
    }

    pub fn from_publication(error: PublicationError) -> Self {
        match error {
            PublicationError::NotFound(_) => {
                Self::new(ErrorCode::NotFound, "No se encontró el proyecto.")
            }
            PublicationError::Conflict { .. } => Self::new(
                ErrorCode::Conflict,
                "El proyecto se modificó en otro lugar. Inténtalo de nuevo.",
            ),
            _ => Self::new(ErrorCode::PublishFailed, "No se pudo publicar en Internet."),
        }
    }

    pub fn from_agent(error: AgentError) -> Self {
        match error {
            AgentError::Cancelled => Self::new(ErrorCode::AiTaskFailed, "La creación se canceló."),
            AgentError::BackendNotReady
            | AgentError::BackendStartFailed(_)
            | AgentError::BinaryNotFound(_)
            | AgentError::IncompatibleVersion { .. }
            | AgentError::SessionCreationFailed(_)
            | AgentError::SessionNotFound(_)
            | AgentError::Timeout
            | AgentError::Http(_)
            | AgentError::ShutdownFailed(_) => Self::new(
                ErrorCode::AiUnavailable,
                "No se pudo iniciar el asistente de IA.",
            ),
            AgentError::BackendAlreadyReady
            | AgentError::TaskFailed(_)
            | AgentError::RegistrationFailed(_) => {
                Self::new(ErrorCode::AiTaskFailed, "No se pudo completar la creación.")
            }
        }
    }

    /// Contextual mapping for the provider/model surface. Only the human-facing
    /// code + message reach the frontend; no raw provider payloads or ids.
    pub fn from_provider(error: ProviderError) -> Self {
        match error {
            ProviderError::NotFound(_) => Self::new(
                ErrorCode::ProviderNotFound,
                "Ese proveedor no está disponible.",
            ),
            ProviderError::ConnectFailed(_) | ProviderError::OAuthFailed(_) => Self::new(
                ErrorCode::ProviderConnectFailed,
                "No pudimos conectar tu cuenta.",
            ),
            ProviderError::DisconnectFailed(_) => Self::new(
                ErrorCode::ProviderConnectFailed,
                "No pudimos desconectar tu cuenta.",
            ),
            ProviderError::CredentialInvalid => {
                Self::new(ErrorCode::CredentialInvalid, "Esta clave no es válida.")
            }
            ProviderError::CredentialRevoked => Self::new(
                ErrorCode::CredentialRevoked,
                "Necesitás volver a conectar tu cuenta.",
            ),
            ProviderError::ProviderUnavailable | ProviderError::BackendNotReady => Self::new(
                ErrorCode::ProviderUnavailable,
                "No pudimos conectarnos con el proveedor.",
            ),
            ProviderError::ModelUnavailable => Self::new(
                ErrorCode::ModelUnavailable,
                "Este modelo ya no está disponible.",
            ),
            ProviderError::NoCompatibleModel => Self::new(
                ErrorCode::NoCompatibleModel,
                "No encontramos un modelo disponible para este proveedor.",
            ),
            ProviderError::NetworkError => Self::new(
                ErrorCode::NetworkError,
                "No hay conexión con el proveedor. Revisá tu conexión.",
            ),
            ProviderError::Internal(_) => {
                Self::new(ErrorCode::Internal, "No se pudo completar la operación.")
            }
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for AppError {}

pub type AppResult<T> = Result<T, AppError>;
