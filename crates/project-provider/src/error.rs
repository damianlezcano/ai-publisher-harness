use std::fmt;

use crate::models::ConnectionTestOutcome;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderError {
    /// Unknown provider/model/credential id (never echoed back to the user).
    NotFound(String),
    ConnectFailed(String),
    CredentialInvalid,
    CredentialRevoked,
    ProviderUnavailable,
    ModelUnavailable,
    NoCompatibleModel,
    NetworkError,
    OAuthFailed(String),
    DisconnectFailed(String),
    BackendNotReady,
    Internal(String),
}

impl ProviderError {
    /// The outcome a connection test would report for this error, when
    /// meaningful. Used to keep the app's error mapping consistent.
    pub fn test_outcome(&self) -> Option<ConnectionTestOutcome> {
        match self {
            ProviderError::CredentialInvalid => Some(ConnectionTestOutcome::CredentialInvalid),
            ProviderError::ProviderUnavailable => Some(ConnectionTestOutcome::ProviderUnavailable),
            ProviderError::NoCompatibleModel | ProviderError::ModelUnavailable => {
                Some(ConnectionTestOutcome::NoCompatibleModel)
            }
            ProviderError::NetworkError => Some(ConnectionTestOutcome::NetworkError),
            _ => None,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderError::NotFound(_) => write!(f, "provider resource not found"),
            ProviderError::ConnectFailed(_) => write!(f, "failed to connect provider"),
            ProviderError::CredentialInvalid => write!(f, "credential is invalid"),
            ProviderError::CredentialRevoked => write!(f, "credential was revoked"),
            ProviderError::ProviderUnavailable => write!(f, "provider is unavailable"),
            ProviderError::ModelUnavailable => write!(f, "model is unavailable"),
            ProviderError::NoCompatibleModel => write!(f, "no compatible model"),
            ProviderError::NetworkError => write!(f, "provider network error"),
            ProviderError::OAuthFailed(_) => write!(f, "OAuth flow failed"),
            ProviderError::DisconnectFailed(_) => write!(f, "failed to disconnect provider"),
            ProviderError::BackendNotReady => write!(f, "provider backend is not ready"),
            ProviderError::Internal(_) => write!(f, "internal provider error"),
        }
    }
}

impl std::error::Error for ProviderError {}

pub type ProviderResult<T> = Result<T, ProviderError>;
