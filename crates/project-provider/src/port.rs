//! The credential boundary: `ProviderConnector`.
//!
//! The single real adapter is `OpenCodeProviderConnector` (OpenCode-owned
//! credential storage); tests use [`crate::FakeProviderConnector`]. The app and
//! project-core only ever see this port, never OpenCode concepts.

use crate::error::ProviderResult;
use crate::models::{
    ConnectionState, ConnectionTest, ModelSummary, OAuthAttempt, OAuthStatus, ProviderDetail,
    ProviderSummary,
};
use crate::secret::SecretString;

pub trait ProviderConnector: Send + Sync {
    fn list_providers(&self) -> ProviderResult<Vec<ProviderSummary>>;

    fn provider_detail(&self, provider_id: &str) -> ProviderResult<ProviderDetail>;

    /// Stores a credential with OpenCode (one-way). The secret is dropped by
    /// the caller immediately after this call; it is never returned.
    fn connect_api_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> ProviderResult<ConnectionState>;

    fn begin_oauth(&self, provider_id: &str, method_id: &str) -> ProviderResult<OAuthAttempt>;

    fn oauth_status(&self, attempt_id: &str) -> ProviderResult<OAuthStatus>;

    fn complete_oauth(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> ProviderResult<ConnectionState>;

    fn cancel_oauth(&self, attempt_id: &str) -> ProviderResult<()>;

    fn disconnect(&self, credential_id: &str) -> ProviderResult<()>;

    fn list_models(&self) -> ProviderResult<Vec<ModelSummary>>;

    fn test_connection(&self, provider_id: &str, model_id: &str) -> ProviderResult<ConnectionTest>;
}
