//! M7 provider domain: `ProviderConnector` port and OpenCode-independent models.
//!
//! This crate owns the provider/model domain and the credential boundary. It is
//! OpenCode-independent: the UI and project-core never see OpenCode concepts. The
//! real adapter (`OpenCodeProviderConnector`, driving OpenCode's integration API)
//! and `ProviderService` are added by later M7 tasks; tests use
//! `FakeProviderConnector`.

#![forbid(unsafe_code)]

pub mod error;
pub mod fake;
pub mod models;
pub mod port;
pub mod secret;

pub use error::{ProviderError, ProviderResult};
pub use fake::{FakeProviderConnector, ProviderCall, ScriptedOAuth};
pub use models::{
    AuthMethodKind, AuthMethodView, AuthPrompt, AuthPromptKind, ConnectionState, ConnectionTest,
    ConnectionTestOutcome, ConnectionView, ModelSummary, OAuthAttempt, OAuthMode, OAuthStatus,
    OAuthStatusKind, ProviderDetail, ProviderSummary,
};
pub use port::ProviderConnector;
pub use secret::{SecretString, redact_credentials};
