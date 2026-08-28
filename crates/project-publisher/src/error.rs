use std::fmt;

use crate::model::PublicationRoute;

/// Errors produced by local publisher operations and route management.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublisherError {
    AlreadyRunning,
    NotRunning,
    RouteConflict(PublicationRoute),
    InvalidRoute(String),
    NotRegistered(PublicationRoute),
    InvalidPublishRoot(String),
    InvalidEndpoint(String),
    BindFailed(String),
    RegistrationFailed(String),
    ShutdownFailed(String),
}

impl fmt::Display for PublisherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("local publisher is already running"),
            Self::NotRunning => f.write_str("local publisher is not running"),
            Self::RouteConflict(route) => write!(f, "publication route conflict: {route}"),
            Self::InvalidRoute(reason) => write!(f, "invalid publication route: {reason}"),
            Self::NotRegistered(route) => write!(f, "publication route not registered: {route}"),
            Self::InvalidPublishRoot(reason) => write!(f, "invalid publish root: {reason}"),
            Self::InvalidEndpoint(reason) => write!(f, "invalid publisher endpoint: {reason}"),
            Self::BindFailed(reason) => write!(f, "publisher bind failed: {reason}"),
            Self::RegistrationFailed(reason) => {
                write!(f, "publisher registration failed: {reason}")
            }
            Self::ShutdownFailed(reason) => write!(f, "publisher shutdown failed: {reason}"),
        }
    }
}

impl std::error::Error for PublisherError {}

pub type PublisherResult<T> = Result<T, PublisherError>;
