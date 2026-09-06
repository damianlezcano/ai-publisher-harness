use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentError {
    BackendNotReady,
    BackendAlreadyReady,
    BackendStartFailed(String),
    BinaryNotFound(String),
    IncompatibleVersion { found: String, expected: String },
    SessionCreationFailed(String),
    SessionNotFound(String),
    TaskFailed(String),
    Cancelled,
    Timeout,
    Http(String),
    ShutdownFailed(String),
    RegistrationFailed(String),
}

/// Sanitized cause of a terminal agent task failure. Unit variants only; the
/// classifier maps the observable `AgentError` state to a stable code without
/// exposing provider response bodies, prompts, secrets, or raw HTTP payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskFailureKind {
    /// The bounded polling deadline elapsed before a terminal assistant reply.
    Timeout,
    /// The sidecar reported the session failed/errored (agent-execution
    /// failure, which may conceal a provider error OpenCode does not surface).
    AgentExecution,
    /// A provider/sidecar HTTP transport error during the run.
    ProviderTransport,
    /// The user cancelled the run.
    Cancelled,
    /// Any other classifyable failure not covered above.
    Unknown,
}

impl TaskFailureKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::AgentExecution => "agent_execution",
            Self::ProviderTransport => "provider_transport",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

impl AgentError {
    /// Classifies a terminal task error into a sanitized [`TaskFailureKind`]
    /// without inspecting or logging any provider/transport payload.
    pub fn task_failure_kind(&self) -> TaskFailureKind {
        match self {
            Self::Timeout => TaskFailureKind::Timeout,
            Self::Cancelled => TaskFailureKind::Cancelled,
            // OpenCode collapses provider rate-limit/context/auth/internal
            // failures into an opaque "task failed"; we record agent_execution
            // and do not invent a more specific provider cause we cannot prove.
            Self::TaskFailed(_) => TaskFailureKind::AgentExecution,
            Self::Http(_) => TaskFailureKind::ProviderTransport,
            Self::SessionCreationFailed(_)
            | Self::SessionNotFound(_)
            | Self::BackendNotReady
            | Self::BackendAlreadyReady
            | Self::BackendStartFailed(_)
            | Self::BinaryNotFound(_)
            | Self::IncompatibleVersion { .. }
            | Self::ShutdownFailed(_)
            | Self::RegistrationFailed(_) => TaskFailureKind::Unknown,
        }
    }
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::BackendNotReady => write!(f, "agent backend is not ready"),
            AgentError::BackendAlreadyReady => write!(f, "agent backend is already ready"),
            AgentError::BackendStartFailed(_) => write!(f, "failed to start agent backend"),
            AgentError::BinaryNotFound(_) => write!(f, "agent backend binary not found"),
            AgentError::IncompatibleVersion { found, expected } => {
                write!(
                    f,
                    "incompatible agent backend version (found {found}, expected {expected})"
                )
            }
            AgentError::SessionCreationFailed(_) => write!(f, "failed to create agent session"),
            AgentError::SessionNotFound(_) => write!(f, "agent session not found"),
            AgentError::TaskFailed(_) => write!(f, "agent task failed"),
            AgentError::Cancelled => write!(f, "agent task was cancelled"),
            AgentError::Timeout => write!(f, "agent operation timed out"),
            AgentError::Http(_) => write!(f, "agent HTTP request failed"),
            AgentError::ShutdownFailed(_) => write!(f, "failed to shut down agent backend"),
            AgentError::RegistrationFailed(_) => write!(f, "could not register creation"),
        }
    }
}

impl std::error::Error for AgentError {}

pub type AgentResult<T> = Result<T, AgentError>;
