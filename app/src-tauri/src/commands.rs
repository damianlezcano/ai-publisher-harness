//! Thin Tauri command layer over the `project-app` facade.
//!
//! Each command is narrow and case-oriented; there is no generic shell,
//! filesystem, process, or open command. Blocking facade calls run on the async
//! runtime via `spawn_blocking`; the long-running agent task is dispatched to a
//! detached thread and reported back through `agent://task` events.

use std::sync::Arc;

use project_app::{
    AppError, AppState, AppStatusView, CreationView, MaterialAddImageView, MaterialView,
    MaterialsImportReport, PreviewData, ProjectSummary, ProjectView, PublicationView,
    SelectedModelView,
};
use project_provider::{
    ConnectionTest, ConnectionView, ModelSummary, OAuthAttempt, OAuthStatus, ProviderDetail,
    ProviderSummary, SecretString,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State, WebviewUrl, WebviewWindowBuilder};

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
    blocking(state.inner().clone(), move |app| {
        app.open_project(&project_id)
    })
    .await
}

#[tauri::command]
pub async fn project_rename(
    state: State<'_, SharedState>,
    project_id: String,
    name: String,
) -> Result<ProjectSummary, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.rename_project(&project_id, &name)
    })
    .await
}

#[tauri::command]
pub async fn project_delete(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.delete_project(&project_id)
    })
    .await
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
pub async fn material_add_image(
    state: State<'_, SharedState>,
    project_id: String,
    file_name: String,
    content_type: String,
    data: Vec<u8>,
) -> Result<MaterialAddImageView, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.add_material_image(&project_id, &file_name, &content_type, data)
    })
    .await
}

#[tauri::command]
pub async fn materials_add_from_paths(
    state: State<'_, SharedState>,
    project_id: String,
    paths: Vec<String>,
) -> Result<MaterialsImportReport, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.import_materials(&project_id, paths)
    })
    .await
}

#[tauri::command]
pub async fn material_remove(
    state: State<'_, SharedState>,
    project_id: String,
    material_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.remove_material(&project_id, &material_id)
    })
    .await
}

#[tauri::command]
pub async fn material_open(
    state: State<'_, SharedState>,
    project_id: String,
    material_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.open_material(&project_id, &material_id)
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
pub async fn preview_data(
    state: State<'_, SharedState>,
    project_id: String,
    resource_kind: String,
    resource_id: String,
) -> Result<PreviewData, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.preview_data(&project_id, &resource_kind, &resource_id)
    })
    .await
}

/// Opens the isolated web preview (ADR-0010). The facade copies the creation
/// tree and starts a loopback token server; this command creates the dedicated
/// `preview` WebviewWindow (zero-capability `preview.json`) pointing at the
/// backend-created URL, and tears the server down when that window closes. The
/// frontend never chooses the URL or capabilities.
#[tauri::command]
pub async fn preview_open_web(
    app: AppHandle,
    state: State<'_, SharedState>,
    project_id: String,
    creation_id: String,
) -> Result<(), AppError> {
    let shared = state.inner().clone();
    let web = blocking(shared.clone(), move |app| {
        app.preview_open_web(&project_id, &creation_id)
    })
    .await?;
    let token = web.token.clone();
    let preview_origin = format!("http://127.0.0.1:{}/preview/{}/", origin_port(&web.url), token);
    // Navigate to the creation entrypoint. The preview server also maps the
    // token root to index.html; this URL is the same artifact Abrir/Compartir use.
    let url = preview_entrypoint_url(&web.url);

    let window = WebviewWindowBuilder::new(
        &app,
        "preview",
        WebviewUrl::External(
            url.parse()
                .map_err(|_| AppError::internal("La vista previa no es válida."))?,
        ),
    )
    .title("Vista previa")
    .inner_size(1100.0, 720.0)
    .on_navigation(move |request_url| {
        // Pin navigation to the preview server's own origin and token path.
        // Generated content must never navigate the preview webview to the app
        // origin (tauri://localhost / dev server) where other IPC could apply.
        request_url.to_string().starts_with(&preview_origin)
    })
    .build()
    .map_err(|_| {
        let _ = shared.preview_close(&token);
        AppError::new(
            project_app::ErrorCode::PreviewUnavailable,
            "No pudimos mostrar la vista previa.",
        )
    })?;

    let state_for_close = shared.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            let _ = state_for_close.preview_close(&token);
        }
    });
    Ok(())
}

/// Extracts the port from a `http://127.0.0.1:<port>/…` preview URL.
fn origin_port(url: &str) -> String {
    url.split('/')
        .nth(2)
        .and_then(|host_port| host_port.rsplit_once(':'))
        .map(|(_, port)| port.to_owned())
        .unwrap_or_default()
}

fn preview_entrypoint_url(base: &str) -> String {
    if base.ends_with("index.html") {
        base.to_owned()
    } else if base.ends_with('/') {
        format!("{base}index.html")
    } else {
        format!("{base}/index.html")
    }
}

/// Closes the isolated web preview by its single-use token (belt-and-suspenders
/// alongside the window-closed teardown).
#[tauri::command]
pub async fn preview_close(state: State<'_, SharedState>, token: String) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| app.preview_close(&token)).await
}

#[tauri::command]
pub async fn open_public_url(
    state: State<'_, SharedState>,
    project_id: String,
) -> Result<(), AppError> {
    blocking(state.inner().clone(), move |app| {
        app.open_public_url(&project_id)
    })
    .await
}

#[tauri::command]
pub async fn agent_send(
    app: AppHandle,
    state: State<'_, SharedState>,
    project_id: String,
    prompt: String,
    attachment_ids: Vec<String>,
) -> Result<(), AppError> {
    let shared = state.inner().clone();
    // Persist the user message before this command returns so a second send
    // cannot race ahead of the first turn, and so the UI can treat the
    // request as in-flight as soon as `agent_send` resolves.
    let persist_id = project_id.clone();
    let persist_prompt = prompt.clone();
    let persist_attachments = attachment_ids.clone();
    let inputs = blocking(shared.clone(), move |app| {
        app.send_message_persist(&persist_id, &persist_prompt, &persist_attachments)
    })
    .await?;
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
        let event = match shared.send_message_run(inputs) {
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
    blocking(state.inner().clone(), move |app| {
        app.cancel_agent(&project_id)
    })
    .await
}

#[tauri::command]
pub async fn agent_status(state: State<'_, SharedState>) -> Result<String, AppError> {
    Ok(state.agent_status().to_owned())
}

#[tauri::command]
pub async fn publish(
    state: State<'_, SharedState>,
    project_id: String,
    creation_id: Option<String>,
) -> Result<PublicationView, AppError> {
    blocking(state.inner().clone(), move |app| {
        app.publish_creation(&project_id, creation_id.as_deref())
    })
    .await
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
    blocking(state.inner().clone(), move |app| {
        app.publication_status(&project_id)
    })
    .await
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
pub async fn provider_list(
    state: State<'_, SharedState>,
) -> Result<Vec<ProviderSummary>, AppError> {
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
