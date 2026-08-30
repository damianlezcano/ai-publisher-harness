//! M7 provider domain: `ProviderConnector` port and OpenCode-independent models.
//!
//! This crate owns the provider/model domain and the credential boundary. Domain
//! types are OpenCode-independent; the UI and project-core never see OpenCode
//! concepts. The HTTP adapter is [`OpenCodeProviderConnector`]. Tests also use
//! [`FakeProviderConnector`]. Orchestration lives in [`ProviderService`].

#![forbid(unsafe_code)]

pub mod adapter;
pub mod error;
pub mod fake;
pub mod models;
pub mod port;
pub mod secret;
pub mod service;
pub mod settings;

pub use adapter::OpenCodeProviderConnector;
pub use error::{ProviderError, ProviderResult};
pub use fake::{FakeProviderConnector, ProviderCall, ScriptedOAuth};
pub use models::{
    AuthMethodKind, AuthMethodView, AuthPrompt, AuthPromptKind, ConnectionState, ConnectionTest,
    ConnectionTestOutcome, ConnectionView, ModelSummary, OAuthAttempt, OAuthMode, OAuthStatus,
    OAuthStatusKind, ProviderDetail, ProviderSummary,
};
pub use port::ProviderConnector;
pub use secret::{SecretString, redact_credentials};
pub use service::{BackendRestarter, FakeRestarter, ProviderService, SelectedModel};
pub use settings::{ModelSelection, Settings, SettingsStore};
