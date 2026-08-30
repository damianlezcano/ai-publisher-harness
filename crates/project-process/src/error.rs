use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    BinaryNotFound(String),
    StartFailed(String),
    Timeout,
    StopFailed(String),
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProcessError::BinaryNotFound(name) => write!(f, "binary not found: {name}"),
            ProcessError::StartFailed(reason) => write!(f, "failed to start process: {reason}"),
            ProcessError::Timeout => write!(f, "timed out waiting for process"),
            ProcessError::StopFailed(reason) => write!(f, "failed to stop process: {reason}"),
        }
    }
}

impl std::error::Error for ProcessError {}

pub type ProcessResult<T> = Result<T, ProcessError>;
