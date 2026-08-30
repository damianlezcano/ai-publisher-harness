//! Thin Tauri command layer over the `project-app` facade.
//!
//! Each command is narrow and case-oriented; there is no generic shell,
//! filesystem, process, or open command. Blocking facade calls run on the async
//! runtime via `spawn_blocking`; the long-running agent task is dispatched to a
//! detached thread and reported back through `agent://task` events.

use std::sync::Arc;

use project_app::{
    AppError, AppState, AppStatusView, CreationView, MaterialView, ProjectSummary, ProjectView,
    PublicationView,
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
