use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelError {
    InvalidOrigin(String),
    InvalidBaseUrl(String),
    AlreadyRunning,
    NotRunning,
    StartFailed(String),
    StartupTimeout,
    UrlNotDetected,
    ProcessExited { code: Option<i32> },
    StopFailed(String),
    BinaryNotFound(String),
}

impl fmt::Display for TunnelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TunnelError::InvalidOrigin(origin) => write!(f, "invalid local origin: {origin}"),
            TunnelError::InvalidBaseUrl(url) => write!(f, "invalid public base URL: {url}"),
            TunnelError::AlreadyRunning => write!(f, "tunnel is already running"),
            TunnelError::NotRunning => write!(f, "tunnel is not running"),
            TunnelError::StartFailed(_) => write!(f, "failed to start tunnel"),
            TunnelError::StartupTimeout => write!(f, "timed out waiting for tunnel to start"),
            TunnelError::UrlNotDetected => write!(f, "tunnel URL was not detected"),
            TunnelError::ProcessExited { code } => match code {
                Some(code) => write!(f, "tunnel process exited unexpectedly (code {code})"),
                None => write!(f, "tunnel process exited unexpectedly"),
            },
            TunnelError::StopFailed(_) => write!(f, "failed to stop tunnel"),
            TunnelError::BinaryNotFound(_) => write!(f, "cloudflared binary not found"),
        }
    }
}

impl std::error::Error for TunnelError {}

pub type TunnelResult<T> = Result<T, TunnelError>;
