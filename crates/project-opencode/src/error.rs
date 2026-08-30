use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    NotReady,
    StartFailed(String),
    BinaryNotFound(String),
    IncompatibleVersion { found: String, expected: String },
    Timeout,
    Http(String),
    ShutdownFailed(String),
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BackendError::NotReady => write!(f, "opencode backend is not ready"),
            BackendError::StartFailed(_) => write!(f, "failed to start opencode backend"),
            BackendError::BinaryNotFound(_) => write!(f, "opencode backend binary not found"),
            BackendError::IncompatibleVersion { found, expected } => {
                write!(
                    f,
                    "incompatible opencode backend version (found {found}, expected {expected})"
                )
            }
            BackendError::Timeout => write!(f, "opencode backend operation timed out"),
            BackendError::Http(_) => write!(f, "opencode backend HTTP request failed"),
            BackendError::ShutdownFailed(_) => write!(f, "failed to shut down opencode backend"),
        }
    }
}

impl std::error::Error for BackendError {}

pub type BackendResult<T> = Result<T, BackendError>;
