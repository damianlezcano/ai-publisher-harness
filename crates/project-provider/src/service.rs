//! Provider orchestration: selection, settings, test-connection, restart-on-mutation.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{ProviderError, ProviderResult};
use crate::models::{
    ConnectionState, ConnectionTest, ConnectionTestOutcome, ConnectionView, ModelSummary,
    OAuthAttempt, OAuthStatus, ProviderDetail, ProviderSummary,
};
use crate::port::ProviderConnector;
use crate::secret::SecretString;
use crate::settings::{ModelSelection, SettingsStore};

const DEFAULT_FEATURED: [&str; 5] = ["openai", "google", "deepseek", "anthropic", "opencode"];
const NOTICE_FALLBACK: &str = "Este modelo ya no está disponible; usamos el recomendado.";
const NOTICE_CHOOSE: &str = "Este modelo ya no está disponible. Elegí otro.";
const NOTICE_NONE: &str = "No encontramos un modelo disponible. Elegí uno.";
const NOTICE_NO_TEST_MODEL: &str = "No encontramos un modelo disponible para este proveedor.";

pub trait BackendRestarter: Send + Sync {
    fn restart(&self) -> ProviderResult<()>;
}

#[derive(Clone)]
pub struct FakeRestarter {
    restarts: Arc<AtomicUsize>,
}

impl FakeRestarter {
    pub fn new() -> Self {
        Self {
            restarts: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn restart_count(&self) -> usize {
        self.restarts.load(Ordering::SeqCst)
    }
}

impl Default for FakeRestarter {
    fn default() -> Self {
        Self::new()
    }
}

impl BackendRestarter for FakeRestarter {
    fn restart(&self) -> ProviderResult<()> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedModel {
    pub model: ModelSummary,
    pub notice: Option<String>,
    pub requires_choice: bool,
}

pub struct ProviderService<C: ProviderConnector, R: BackendRestarter> {
    connector: C,
    restarter: R,
    settings: SettingsStore,
}

impl<C: ProviderConnector, R: BackendRestarter> ProviderService<C, R> {
    pub fn new(connector: C, restarter: R, settings_path: PathBuf) -> Self {
        Self {
            connector,
            restarter,
            settings: SettingsStore::new(settings_path),
        }
    }

    pub fn list_providers(&self) -> ProviderResult<Vec<ProviderSummary>> {
        let mut list = self.connector.list_providers()?;
        let stored = self.settings.load();
        let order: Vec<String> = match stored.featured_order {
            Some(ref ids) if !ids.is_empty() => ids.clone(),
            _ => DEFAULT_FEATURED.iter().map(|id| (*id).to_owned()).collect(),
        };
        let live: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
        let featured: Vec<&str> = order
            .iter()
            .map(String::as_str)
            .filter(|id| live.contains(id))
            .collect();
        for provider in &mut list {
            provider.highlighted = featured.contains(&provider.id.as_str());
        }
        Ok(list)
    }

    pub fn provider_detail(&self, provider_id: &str) -> ProviderResult<ProviderDetail> {
        self.connector.provider_detail(provider_id)
    }

    pub fn list_models(&self) -> ProviderResult<Vec<ModelSummary>> {
        self.connector.list_models()
    }

    pub fn connect_api_key(
        &self,
        provider_id: &str,
        key: &SecretString,
        label: Option<&str>,
    ) -> ProviderResult<ConnectionView> {
        let state = self.connector.connect_api_key(provider_id, key, label)?;
        self.restarter.restart()?;
        Ok(connection_view(state))
    }

    pub fn complete_oauth(
        &self,
        attempt_id: &str,
        code: Option<&str>,
    ) -> ProviderResult<ConnectionView> {
        let state = self.connector.complete_oauth(attempt_id, code)?;
        self.restarter.restart()?;
        Ok(connection_view(state))
    }

    pub fn disconnect(&self, credential_id: &str) -> ProviderResult<()> {
        self.connector.disconnect(credential_id)?;
        self.restarter.restart()
    }

    pub fn cancel_oauth(&self, attempt_id: &str) -> ProviderResult<()> {
        self.connector.cancel_oauth(attempt_id)
    }

    pub fn oauth_status(&self, attempt_id: &str) -> ProviderResult<OAuthStatus> {
        self.connector.oauth_status(attempt_id)
    }

    pub fn begin_oauth(&self, provider_id: &str, method_id: &str) -> ProviderResult<OAuthAttempt> {
        self.connector.begin_oauth(provider_id, method_id)
    }

    pub fn get_selected_model(&self) -> ProviderResult<SelectedModel> {
        let models = self.connector.list_models()?;
        let stored = self.settings.load();
        match stored.selected_model {
            Some(sel) => {
                if let Some(model) = find_model(&models, &sel.provider_id, &sel.model_id) {
                    return Ok(SelectedModel {
                        model: model.clone(),
                        notice: None,
                        requires_choice: false,
                    });
                }
                let same_provider: Vec<&ModelSummary> = models
                    .iter()
                    .filter(|m| m.provider_id == sel.provider_id)
                    .collect();
                if let Some(chosen) = pick_free(&same_provider) {
                    self.persist_selection(&chosen.provider_id, &chosen.model_id)?;
                    return Ok(SelectedModel {
                        model: chosen.clone(),
                        notice: Some(NOTICE_FALLBACK.into()),
                        requires_choice: false,
                    });
                }
                Ok(SelectedModel {
                    model: ghost_model(&sel),
                    notice: Some(NOTICE_CHOOSE.into()),
                    requires_choice: true,
                })
            }
            None => {
                if let Some(model) = default_free_model(&models) {
                    return Ok(SelectedModel {
                        model: model.clone(),
                        notice: None,
                        requires_choice: false,
                    });
                }
                Ok(SelectedModel {
                    model: ghost_model(&ModelSelection {
                        provider_id: String::new(),
                        model_id: String::new(),
                    }),
                    notice: Some(NOTICE_NONE.into()),
                    requires_choice: true,
                })
            }
        }
    }

    pub fn select_model(&self, provider_id: &str, model_id: &str) -> ProviderResult<ModelSummary> {
        let models = self.connector.list_models()?;
        let model = find_model(&models, provider_id, model_id)
            .cloned()
            .ok_or(ProviderError::ModelUnavailable)?;
        self.persist_selection(&model.provider_id, &model.model_id)?;
        Ok(model)
    }

    pub fn test_connection(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
    ) -> ProviderResult<ConnectionTest> {
        let resolved = match model_id {
            Some(id) => id.to_owned(),
            None => {
                let models = self.connector.list_models()?;
                let for_provider: Vec<&ModelSummary> = models
                    .iter()
                    .filter(|m| m.provider_id == provider_id)
                    .collect();
                match pick_free(&for_provider) {
                    Some(model) => model.model_id.clone(),
                    None => {
                        return Ok(ConnectionTest {
                            outcome: ConnectionTestOutcome::NoCompatibleModel,
                            message: NOTICE_NO_TEST_MODEL.into(),
                        });
                    }
                }
            }
        };
        self.connector.test_connection(provider_id, &resolved)
    }

    fn persist_selection(&self, provider_id: &str, model_id: &str) -> ProviderResult<()> {
        let mut settings = self.settings.load();
        settings.selected_model = Some(ModelSelection {
            provider_id: provider_id.to_owned(),
            model_id: model_id.to_owned(),
        });
        self.settings.save(&settings)
    }
}

fn connection_view(state: ConnectionState) -> ConnectionView {
    state.connection.unwrap_or(ConnectionView {
        id: String::new(),
        label: None,
    })
}

fn find_model<'a>(
    models: &'a [ModelSummary],
    provider_id: &str,
    model_id: &str,
) -> Option<&'a ModelSummary> {
    models
        .iter()
        .find(|m| m.provider_id == provider_id && m.model_id == model_id)
}

fn rank_free_model(model: &ModelSummary) -> u8 {
    match (model.provider_id.as_str() == "opencode", model.recommended) {
        (true, true) => 3,
        (true, false) => 2,
        (false, true) => 1,
        (false, false) => 0,
    }
}

fn pick_free<'a>(models: &[&'a ModelSummary]) -> Option<&'a ModelSummary> {
    let mut candidates: Vec<&'a ModelSummary> = models
        .iter()
        .copied()
        .filter(|m| !m.deprecated && m.free)
        .collect();
    candidates.sort_by_key(|m| {
        (
            std::cmp::Reverse(rank_free_model(m)),
            &m.provider_id,
            &m.model_id,
        )
    });
    candidates.into_iter().next()
}

fn default_free_model(models: &[ModelSummary]) -> Option<&ModelSummary> {
    let mut candidates: Vec<&ModelSummary> =
        models.iter().filter(|m| !m.deprecated && m.free).collect();
    candidates.sort_by_key(|m| {
        (
            std::cmp::Reverse(rank_free_model(m)),
            &m.provider_id,
            &m.model_id,
        )
    });
    candidates.into_iter().next()
}

fn ghost_model(sel: &ModelSelection) -> ModelSummary {
    ModelSummary {
        provider_id: sel.provider_id.clone(),
        model_id: sel.model_id.clone(),
        name: sel.model_id.clone(),
        free: false,
        recommended: false,
        deprecated: false,
    }
}
