//! Provider/model domain models (OpenCode-independent, camelCase-serializable).

use serde::{Deserialize, Serialize};

/// Lightweight provider list entry for the default UX.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    /// `api_key`/`account` views; `env` is intentionally not offered.
    pub auth_methods: Vec<AuthMethodView>,
    pub connected: bool,
    /// First connection's label when connected, else `None`.
    pub connection_label: Option<String>,
    pub highlighted: bool,
}

/// Full provider detail: auth methods plus every stored connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    pub id: String,
    pub name: String,
    pub auth_methods: Vec<AuthMethodView>,
    /// Opaque credential references; never a secret.
    pub connections: Vec<ConnectionView>,
}

/// A user-facing auth method: an API-key paste (`api_key`) or an account
/// connection (`account`, backed by an OpenCode OAuth method id).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthMethodView {
    pub kind: AuthMethodKind,
    /// Present for `account` methods (the OAuth method id); `None` for `api_key`.
    pub method_id: Option<String>,
    pub label: String,
    /// Optional labeled inputs an OAuth flow may require (e.g. enterprise URL).
    pub prompts: Vec<AuthPrompt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethodKind {
    ApiKey,
    Account,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub key: String,
    pub message: String,
    pub kind: AuthPromptKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPromptKind {
    Text,
    Select,
}

/// An opaque credential reference. The frontend only ever sees this, never the
/// secret itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub id: String,
    pub label: Option<String>,
}

/// The provider connection state after a credential mutation. Contains no
/// secret: `connection` is the opaque credential reference when connected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionState {
    pub connected: bool,
    pub connection: Option<ConnectionView>,
}

/// A model the user may select. Classifications are grounded only on data the
/// catalog reliably provides (`cost == 0`, provider default, `status`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    pub provider_id: String,
    pub model_id: String,
    pub name: String,
    pub free: bool,
    pub recommended: bool,
    pub deprecated: bool,
}

/// A started OAuth/device flow. The app shows `url`/`instructions` and polls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthAttempt {
    pub attempt_id: String,
    pub url: String,
    pub instructions: Option<String>,
    pub mode: OAuthMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthMode {
    Auto,
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStatus {
    pub status: OAuthStatusKind,
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthStatusKind {
    Pending,
    Complete,
    Failed,
    Expired,
}

/// Result of a minimal real model call (`test_connection`). Raw provider
/// payloads are never surfaced; the outcome maps to a human message elsewhere.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub outcome: ConnectionTestOutcome,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionTestOutcome {
    Connected,
    CredentialInvalid,
    ProviderUnavailable,
    NoCompatibleModel,
    NetworkError,
}
