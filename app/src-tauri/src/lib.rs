mod commands;

use std::sync::Arc;

use project_app::{AppConfig, AppState};
use tauri::Manager;

pub type SharedState = Arc<AppState>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = build_state(app.handle())?;
            app.manage(Arc::new(state));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::project_list,
            commands::project_create,
            commands::project_open,
            commands::project_rename,
            commands::project_delete,
            commands::material_add_from_path,
            commands::creation_set_visibility,
            commands::creation_open,
            commands::open_public_url,
            commands::agent_send,
            commands::agent_cancel,
            commands::agent_status,
            commands::publish,
            commands::unpublish,
            commands::publication_status,
            commands::app_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_state(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let config = AppConfig {
        data_dir,
        opencode_binary: std::path::PathBuf::from("opencode"),
        opencode_config_dir: app.path().app_data_dir()?.join("opencode"),
        opencode_port: ephemeral_port(),
        cloudflared_binary: None,
    };
    Ok(AppState::new(config)?)
}

/// Reserve a loopback ephemeral port for the OpenCode backend. A fixed default
/// (8787) is a fallback only; the bound port is released before the backend
/// starts (a small race window accepted for M6).
fn ephemeral_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr().map(|addr| addr.port()))
        .unwrap_or(8787)
}
