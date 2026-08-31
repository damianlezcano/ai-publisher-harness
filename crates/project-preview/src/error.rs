use std::fmt;
use std::net::SocketAddr;

/// Errors produced by preview-server construction, bind, and teardown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewError {
    AlreadyRunning,
    NotRunning,
    NonLoopbackBind(String),
    InvalidCopyDir(String),
    InvalidToken(String),
    BindFailed(String),
    ShutdownFailed(String),
    Entropy(String),
}

impl PreviewError {
    pub(crate) fn non_loopback(addr: SocketAddr) -> Self {
        Self::NonLoopbackBind(addr.to_string())
    }
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("preview server is already running"),
            Self::NotRunning => f.write_str("preview server is not running"),
            Self::NonLoopbackBind(addr) => {
                write!(f, "preview server bind must be 127.0.0.1, got {addr}")
            }
            Self::InvalidCopyDir(reason) => write!(f, "invalid preview copy directory: {reason}"),
            Self::InvalidToken(reason) => write!(f, "invalid preview token: {reason}"),
            Self::BindFailed(reason) => write!(f, "preview server bind failed: {reason}"),
            Self::ShutdownFailed(reason) => write!(f, "preview server shutdown failed: {reason}"),
            Self::Entropy(reason) => write!(f, "preview token entropy failed: {reason}"),
        }
    }
}

impl std::error::Error for PreviewError {}

pub type PreviewResult<T> = Result<T, PreviewError>;
