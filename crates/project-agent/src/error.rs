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
