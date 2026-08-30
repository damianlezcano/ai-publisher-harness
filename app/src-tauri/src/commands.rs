//! Thin Tauri command layer over the `project-app` facade.
//!
//! Each command is narrow and case-oriented; there is no generic shell,
//! filesystem, process, or open command. Blocking facade calls run on the async
//! runtime via `spawn_blocking`; the long-running agent task is dispatched to a
//! detached thread and reported back through `agent://task` events.

use std::sync::Arc;

use project_app::{
    AppError, AppState, AppStatusView, CreationView, MaterialView, ProjectSummary, ProjectView,
    PublicationView, SelectedModelView,
};
use project_provider::{
    ConnectionTest, ConnectionView, ModelSummary, OAuthAttempt, OAuthStatus, ProviderDetail,
    ProviderSummary, SecretString,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::SharedState;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTaskEvent {
    pub project_id: String,
    /// `working`, `completed`, `failed`, or `cancelled`.
    pub status: String,
    pub message: Option<String>,
    pub registered_creation_ids: Vec<String>,
}

async fn blocking<F, T>(app: Arc<AppState>, f: F) -> Result<T, AppError>
where
    F: FnOnce(&AppState) -> Result<T, AppError> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || f(&app))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
}

#[tauri::command]
pub async fn project_list(state: State<'_, SharedState>) -> Result<Vec<ProjectSummary>, AppError> {
    blocking(state.inner().clone(), |app| app.list_projects()).await
}

#[tauri::command]
pub async fn project_create(
    state: State<'_, SharedState>,
    name: String,
) -> Result<ProjectSummary, AppError> {
    blocking(state.inner().clone(), move |app| app.create_project(&name)).await
}

#[tauri::command]
pub async fn project_open(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<ProjectView, AppError> {
    blocking(state.inner().clone(), move |app| app.open_project(&project_id)).await
}

#[tauri::command]
pub async fn project_rename(
    state: State<'_, SharedState>,
    project_id: String,
    name: String,
) -> Result<ProjectSummary, AppError> {
    blocking(state.inner().clone(), move |app| app.rename_project(&project_id, &name)).await
}

#[tauri::command]
pub async fn project_delete(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| app.delete_project(&project_id)).await
}

#[tauri::command]
pub async fn material_add_from_path(
    state: State<'_, SharedState>,
    project_id: String,
    path: String,
) -> Result<MaterialView, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.add_material_from_path(&project_id, &path)
    })
    .await
}

#[tauri::command]
pub async fn creation_set_visibility(
    state: State<'_, SharedState>,
    project_id: String,
    creation_id: String,
    public: bool,
) -> Result<CreationView, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.set_creation_visibility(&project_id, &creation_id, public)
    })
    .await
}

#[tauri::command]
pub async fn creation_open(
    state: State<'_, SharedState>,
    project_id: String,
    creation_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.open_creation(&project_id, &creation_id)
    })
    .await
}

#[tauri::command]
pub async fn open_public_url(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| app.open_public_url(&project_id)).await
}

#[tauri::command]
pub async fn agent_send(
    app: AppHandle,
    state: State<'_, SharedState>,
    project_id: String,
    prompt: String,
) -> Result<(), AppError> {
    let shared = state.inner().clone();
    let _ = app.emit(
        "agent://task",
        AgentTaskEvent {
            project_id: project_id.clone(),
            status: "working".to_owned(),
            message: None,
            registered_creation_ids: Vec::new(),
        },
    );
    std::thread::spawn(move || {
        let event = match shared.run_agent(&project_id, &prompt) {
            Ok(run) => AgentTaskEvent {
                project_id,
                status: run.status,
                message: run.message,
                registered_creation_ids: run.registered_creation_ids,
            },
            Err(err) => AgentTaskEvent {
                project_id,
                status: "failed".to_owned(),
                message: Some(err.message),
                registered_creation_ids: Vec::new(),
            },
        };
        let _ = app.emit("agent://task", event);
    });
    Ok(())
}

#[tauri::command]
pub async fn agent_cancel(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| app.cancel_agent(&project_id)).await
}

#[tauri::command]
pub async fn agent_status(state: State<'_, SharedState>) -> Result<String, AppError> {
    Ok(state.agent_status().to_owned())
}

#[tauri::command]
pub async fn publish(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<PublicationView, AppError> {
    blocking(state.inner().clone(), move |app| app.publish(&project_id)).await
}

#[tauri::command]
pub async fn unpublish(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<PublicationView, AppError> {
    blocking(state.inner().clone(), move |app| app.unpublish(&project_id)).await
}

#[tauri::command]
pub async fn publication_status(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<PublicationView, AppError> {
    blocking(state.inner().clone(), move |app| app.publication_status(&project_id)).await
}

#[tauri::command]
pub async fn app_status(state: State<'_, SharedState>) -> Result<AppStatusView, AppError> {
    Ok(state.app_status())
}

// -- M7 provider/model surface ------------------------------------------------
//
// Narrow, capability-scoped commands (design §17). No generic shell/fs/process
// command exists; the OAuth URL is opened backend-side via `provider_oauth_open`
// and the credential flows exactly once through `provider_connect_key`.

#[tauri::command]
pub async fn provider_list(state: State<'_, SharedState>) -> Result<Vec<ProviderSummary>, AppError> {
    blocking(state.inner().clone(), |app| app.provider_list()).await
}

#[tauri::command]
pub async fn provider_detail(
    state: State<'_, SharedState>,
    provider_id: String,
) -> Result<ProviderDetail, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_detail(&provider_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_connect_key(
    state: State<'_, SharedState>,
    provider_id: String,
    key: String,
    label: Option<String>,
) -> Result<ConnectionView, AppError> {
    // The key enters exactly once, wrapped in a redaction-safe SecretString and
    // dropped by the facade after the loopback request; it is never returned.
    blocking(state.inner().clone(), move |app| {
        let secret = SecretString::new(key);
        app.provider_connect_key(&provider_id, &secret, label.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn provider_oauth_begin(
    state: State<'_, SharedState>,
    provider_id: String,
    method_id: String,
) -> Result<OAuthAttempt, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_oauth_begin(&provider_id, &method_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_oauth_status(
    state: State<'_, SharedState>,
    attempt_id: String,
) -> Result<OAuthStatus, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_oauth_status(&attempt_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_oauth_complete(
    state: State<'_, SharedState>,
    attempt_id: String,
    code: Option<String>,
) -> Result<ConnectionView, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_oauth_complete(&attempt_id, code.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn provider_oauth_cancel(
    state: State<'_, SharedState>,
    attempt_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_oauth_cancel(&attempt_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_oauth_open(
    state: State<'_, SharedState>,
    url: String,
) -> Result<(), AppError> {
    // The frontend never opens an arbitrary browser URL itself; the backend
    // validates (https-only) and opens.
    blocking(state.inner().clone(), move |app| {
        app.provider_oauth_open(&url)
    })
    .await
}

#[tauri::command]
pub async fn provider_disconnect(
    state: State<'_, SharedState>,
    credential_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_disconnect(&credential_id)
    })
    .await
}

#[tauri::command]
pub async fn provider_test_connection(
    state: State<'_, SharedState>,
    provider_id: String,
    model_id: Option<String>,
) -> Result<ConnectionTest, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.provider_test_connection(&provider_id, model_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn model_list(state: State<'_, SharedState>) -> Result<Vec<ModelSummary>, AppError> {
    blocking(state.inner().clone(), |app| app.model_list()).await
}

#[tauri::command]
pub async fn model_select(
    state: State<'_, SharedState>,
    provider_id: String,
    model_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.model_select(&provider_id, &model_id).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn model_get_selected(
    state: State<'_, SharedState>,
) -> Result<SelectedModelView, AppError> {
    blocking(state.inner().clone(), |app| app.model_get_selected()).await
}
