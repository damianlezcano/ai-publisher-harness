use std::fmt;

use project_core::{ProjectCoreError, ProjectId};
use project_publisher::PublisherError;

/// Typed publication errors. Messages do not leak filesystem paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicationError {
    NotFound(ProjectId),
    Conflict { project_id: ProjectId },
    Metadata,
    Preparation,
    RouteAllocation,
    PublisherStart,
    PublisherRegister,
    PublisherReplace,
    PublisherUnregister,
    PublisherStop,
    Recovery,
}

impl fmt::Display for PublicationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(_) => f.write_str("project not found"),
            Self::Conflict { .. } => f.write_str("project was changed elsewhere"),
            Self::Metadata => f.write_str("project metadata could not be updated"),
            Self::Preparation => f.write_str("publication snapshot could not be prepared"),
            Self::RouteAllocation => f.write_str("publication route could not be allocated"),
            Self::PublisherStart => f.write_str("local publisher could not start"),
            Self::PublisherRegister => f.write_str("publication could not be registered"),
            Self::PublisherReplace => f.write_str("publication could not be updated"),
            Self::PublisherUnregister => f.write_str("publication could not be unregistered"),
            Self::PublisherStop => f.write_str("local publisher could not stop"),
            Self::Recovery => f.write_str("publication recovery failed"),
        }
    }
}

impl std::error::Error for PublicationError {}

pub type PublicationResult<T> = Result<T, PublicationError>;

pub(crate) fn from_core(error: ProjectCoreError) -> PublicationError {
    match error {
        ProjectCoreError::NotFound(id) => PublicationError::NotFound(id),
        ProjectCoreError::Conflict { project_id } => PublicationError::Conflict { project_id },
        ProjectCoreError::OperationFailed {
            operation: "recover",
        } => PublicationError::Recovery,
        ProjectCoreError::OperationFailed {
            operation: "prepare",
        }
        | ProjectCoreError::InvalidCreation(_)
        | ProjectCoreError::SourceUnreadable
        | ProjectCoreError::PathEscape
        | ProjectCoreError::SymlinkRejected
        | ProjectCoreError::WriteFailed
        | ProjectCoreError::InvalidName(_)
        | ProjectCoreError::InvalidPath(_) => PublicationError::Preparation,
        _ => PublicationError::Metadata,
    }
}

pub(crate) fn from_start(error: PublisherError) -> PublicationError {
    let _ = error;
    PublicationError::PublisherStart
}

pub(crate) fn from_register(error: PublisherError) -> PublicationError {
    let _ = error;
    PublicationError::PublisherRegister
}

pub(crate) fn from_replace(error: PublisherError) -> PublicationError {
    let _ = error;
    PublicationError::PublisherReplace
}

pub(crate) fn from_unregister(error: PublisherError) -> PublicationError {
    let _ = error;
    PublicationError::PublisherUnregister
}

pub(crate) fn from_stop(error: PublisherError) -> PublicationError {
    let _ = error;
    PublicationError::PublisherStop
}
