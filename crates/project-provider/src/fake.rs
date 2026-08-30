//! In-memory `ProviderConnector` for deterministic offline tests.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::error::{ProviderError, ProviderResult};
use crate::models::{
    ConnectionState, ConnectionTest, ConnectionTestOutcome, ConnectionView, ModelSummary,
    OAuthAttempt, OAuthMode, OAuthStatus, OAuthStatusKind, ProviderDetail, ProviderSummary,
};
use crate::port::ProviderConnector;
use crate::secret::SecretString;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderCall {
    ListProviders,
    ProviderDetail,
    ConnectApiKey,
    BeginOauth,
    OauthStatus,
    CompleteOauth,
    CancelOauth,
    Disconnect,
    ListModels,
    TestConnection,
}

/// A scripted OAuth/device flow in the fake.
#[derive(Clone, Debug)]
pub struct ScriptedOAuth {
    pub attempt_id: String,
    pub provider_id: String,
    pub url: String,
    pub instructions: Option<String>,
    pub mode: OAuthMode,
    pub status: OAuthStatusKind,
    pub connection: Option<ConnectionView>,
}

#[derive(Clone)]
pub struct FakeProviderConnector {
    inner: Arc<Mutex<FakeProviderState>>,
}

struct FakeProviderState {
    calls: Vec<ProviderCall>,
    providers: Vec<ProviderDetail>,
    featured: Vec<String>,
    models: Vec<ModelSummary>,
    oauth: HashMap<String, ScriptedOAuth>,
    next_attempt: u64,
    next_connection: u64,
    test_outcome: ConnectionTestOutcome,
    test_message: String,
    connect_error: Option<ProviderError>,
    disconnect_error: Option<ProviderError>,
    begin_oauth_error: Option<ProviderError>,
    last_connect_key: Option<String>,
    last_connect_label: Option<String>,
}

impl fmt::Debug for FakeProviderState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeProviderState")
            .field("calls", &self.calls)
            .field("providers", &self.providers)
            .field("featured", &self.featured)
            .field("models", &self.models)
            .field("oauth", &self.oauth)
            .field("test_outcome", &self.test_outcome)
            .field("connect_error", &self.connect_error)
            .field("disconnect_error", &self.disconnect_error)
            .field("begin_oauth_error", &self.begin_oauth_error)
            .field("last_connect_label", &self.last_connect_label)
            .field("last_connect_key", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for FakeProviderConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeProviderConnector")
            .field(
                "inner",
                &*self.inner.lock().unwrap_or_else(|e| e.into_inner()),
            )
            .finish()
    }
}

impl Default for FakeProviderConnector {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeProviderConnector {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(FakeProviderState {
                calls: Vec::new(),
                providers: Vec::new(),
                featured: Vec::new(),
                models: Vec::new(),
                oauth: HashMap::new(),
                next_attempt: 1,
                next_connection: 1,
                test_outcome: ConnectionTestOutcome::Connected,
                test_message: "Conectado.".into(),
                connect_error: None,
                disconnect_error: None,
                begin_oauth_error: None,
                last_connect_key: None,
                last_connect_label: None,
            })),
        }
    }

    pub fn with_provider(self, detail: ProviderDetail) -> Self {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .providers
            .push(detail);
        self
    }

    pub fn with_featured(self, id: impl Into<String>) -> Self {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .featured
            .push(id.into());
        self
    }

    pub fn with_model(self, model: ModelSummary) -> Self {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .models
            .push(model);
        self
    }

    /// Replaces the scripted model catalog (simulates a model disappearing
    /// from the catalog after an update/refresh).
    pub fn set_models(&self, models: Vec<ModelSummary>) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).models = models;
    }

    pub fn set_test_outcome(&self, outcome: ConnectionTestOutcome, message: &str) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.test_outcome = outcome;
        state.test_message = message.to_owned();
    }

    pub fn set_connect_error(&self, error: ProviderError) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .connect_error = Some(error);
    }

    pub fn set_disconnect_error(&self, error: ProviderError) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .disconnect_error = Some(error);
    }

    pub fn set_begin_oauth_error(&self, error: ProviderError) {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .begin_oauth_error = Some(error);
    }

    /// Seeds an OAuth attempt so `oauth_status`/`complete_oauth` can be driven
    /// through `failed`/`expired` paths deterministically.
    pub fn seed_oauth(&self, attempt: ScriptedOAuth) {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.oauth.insert(attempt.attempt_id.clone(), attempt);
    }

    pub fn calls(&self) -> Vec<ProviderCall> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .calls
            .clone()
    }

    pub fn last_connect_key(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_connect_key
            .clone()
    }

    pub fn last_connect_label(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_connect_label
            .clone()
    }

    /// Whether the provider currently holds at least one connection.
    pub fn is_connected(&self, provider_id: &str) -> bool {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| !p.connections.is_empty())
            .unwrap_or(false)
    }

    /// The opaque connections currently stored for a provider.
    pub fn connections(&self, provider_id: &str) -> Vec<ConnectionView> {
        let state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state
            .providers
            .iter()
            .find(|p| p.id == provider_id)
            .map(|p| p.connections.clone())
            .unwrap_or_default()
    }

    fn provider_index(&self, state: &FakeProviderState, id: &str) -> ProviderResult<usize> {
        state
            .providers
            .iter()
            .position(|p| p.id == id)
            .ok_or_else(|| ProviderError::NotFound(id.to_owned()))
    }

    fn push_connection(
        state: &mut FakeProviderState,
        provider_id: &str,
        connection: ConnectionView,
    ) {
        if let Some(provider) = state.providers.iter_mut().find(|p| p.id == provider_id) {
            provider.connections.push(connection);
        }
    }
}

impl ProviderConnector for FakeProviderConnector {
    fn list_providers(&self) -> ProviderResult<Vec<ProviderSummary>> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::ListProviders);
        Ok(state
            .providers
            .iter()
            .map(|p| ProviderSummary {
                id: p.id.clone(),
                name: p.name.clone(),
                auth_methods: p.auth_methods.clone(),
                connected: !p.connections.is_empty(),
                connection_label: p.connections.first().and_then(|c| c.label.clone()),
                highlighted: state.featured.contains(&p.id),
            })
            .collect())
    }

    fn provider_detail(&self, provider_id: &str) -> ProviderResult<ProviderDetail> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::ProviderDetail);
        let index = self.provider_index(&state, provider_id)?;
        Ok(state.providers[index].clone())
    }

    fn connect_api_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> ProviderResult<ConnectionState> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::ConnectApiKey);
        if let Some(error) = state.connect_error.take() {
            return Err(error);
        }
        let _ = self.provider_index(&state, provider_id)?;
        state.last_connect_key = Some(key.expose().to_owned());
        state.last_connect_label = label.map(str::to_owned);
        let connection = ConnectionView {
            id: format!("cred-{}", state.next_connection),
            label: label.map(str::to_owned),
        };
        state.next_connection += 1;
        Self::push_connection(&mut state, provider_id, connection.clone());
        Ok(ConnectionState {
            connected: true,
            connection: Some(connection),
        })
    }

    fn begin_oauth(&self, provider_id: &str, method_id: &str) -> ProviderResult<OAuthAttempt> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::BeginOauth);
        if let Some(error) = state.begin_oauth_error.take() {
            return Err(error);
        }
        let _ = self.provider_index(&state, provider_id)?;
        let attempt_id = format!("oauth-{}", state.next_attempt);
        state.next_attempt += 1;
        let scripted = ScriptedOAuth {
            attempt_id: attempt_id.clone(),
            provider_id: provider_id.to_owned(),
            url: format!("https://example.test/oauth/{method_id}"),
            instructions: Some("Abrí el enlace y aprobá el acceso.".into()),
            mode: OAuthMode::Auto,
            status: OAuthStatusKind::Pending,
            connection: None,
        };
        state.oauth.insert(attempt_id.clone(), scripted.clone());
        Ok(OAuthAttempt {
            attempt_id,
            url: scripted.url,
            instructions: scripted.instructions,
            mode: scripted.mode,
        })
    }

    fn oauth_status(&self, attempt_id: &str) -> ProviderResult<OAuthStatus> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::OauthStatus);
        let attempt = state
            .oauth
            .get(attempt_id)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound(attempt_id.to_owned()))?;
        Ok(OAuthStatus {
            status: attempt.status,
            message: None,
        })
    }

    fn complete_oauth(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> ProviderResult<ConnectionState> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::CompleteOauth);
        let mut attempt = state
            .oauth
            .remove(attempt_id)
            .ok_or_else(|| ProviderError::NotFound(attempt_id.to_owned()))?;
        if attempt.status == OAuthStatusKind::Failed || attempt.status == OAuthStatusKind::Expired {
            return Err(ProviderError::OAuthFailed("flow already failed".into()));
        }
        let _ = code;
        let connection = attempt
            .connection
            .clone()
            .unwrap_or_else(|| ConnectionView {
                id: format!("cred-{}", state.next_connection),
                label: Some("Cuenta conectada".into()),
            });
        state.next_connection += 1;
        attempt.status = OAuthStatusKind::Complete;
        state.oauth.insert(attempt_id.to_owned(), attempt.clone());
        Self::push_connection(&mut state, &attempt.provider_id, connection.clone());
        Ok(ConnectionState {
            connected: true,
            connection: Some(connection),
        })
    }

    fn cancel_oauth(&self, attempt_id: &str) -> ProviderResult<()> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::CancelOauth);
        if state.oauth.remove(attempt_id).is_none() {
            return Err(ProviderError::NotFound(attempt_id.to_owned()));
        }
        Ok(())
    }

    fn disconnect(&self, credential_id: &str) -> ProviderResult<()> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::Disconnect);
        if let Some(error) = state.disconnect_error.take() {
            return Err(error);
        }
        let mut removed = false;
        for provider in &mut state.providers {
            let before = provider.connections.len();
            provider.connections.retain(|c| c.id != credential_id);
            if provider.connections.len() < before {
                removed = true;
            }
        }
        if !removed {
            return Err(ProviderError::NotFound(credential_id.to_owned()));
        }
        Ok(())
    }

    fn list_models(&self) -> ProviderResult<Vec<ModelSummary>> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::ListModels);
        Ok(state.models.clone())
    }

    fn test_connection(
        &self,
        _provider_id: &str,
        _model_id: &str,
    ) -> ProviderResult<ConnectionTest> {
        let mut state = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        state.calls.push(ProviderCall::TestConnection);
        Ok(ConnectionTest {
            outcome: state.test_outcome,
            message: state.test_message.clone(),
        })
    }
}
