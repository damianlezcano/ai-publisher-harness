mod commands;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use project_app::{AppConfig, AppState};
use tauri::Manager;

pub type SharedState = Arc<AppState>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    project_app::session_log::configure_from_args(std::env::args());
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let state = build_state(app.handle())?;
            project_app::session_log::record(
                "INFO",
                format!("startup version={}", project_app::APP_VERSION),
            );
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
            commands::material_add_image,
            commands::materials_add_from_paths,
            commands::material_remove,
            commands::material_open,
            commands::material_open_folder,
            commands::materials_open_folder,
            commands::creation_set_visibility,
            commands::creation_open,
            commands::creation_open_folder,
            commands::creations_open_folder,
            commands::preview_data,
            commands::preview_open_web,
            commands::preview_close,
            commands::open_public_url,
            commands::agent_send,
            commands::agent_cancel,
            commands::agent_status,
            commands::publish,
            commands::unpublish,
            commands::publication_status,
            commands::app_status,
            commands::provider_list,
            commands::provider_detail,
            commands::provider_connect_key,
            commands::provider_oauth_begin,
            commands::provider_oauth_status,
            commands::provider_oauth_complete,
            commands::provider_oauth_cancel,
            commands::provider_oauth_open,
            commands::provider_disconnect,
            commands::provider_test_connection,
            commands::model_list,
            commands::model_select,
            commands::conversation_model_select,
            commands::conversation_model_clear,
            commands::model_get_selected,
            commands::session_logs,
            commands::session_logs_clear,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Graceful termination on external signals: when the app is terminated
    // with SIGTERM/SIGINT (logout, task manager, `kill <pid>`), the process
    // would otherwise die without running any destructor and orphan its owned
    // `opencode serve` / `cloudflared` children. `signal-hook`'s flag handler
    // is async-signal-safe (it only sets an atomic); a dedicated thread reacts
    // and runs the same owned-child shutdown before exiting.
    let terminate = Arc::new(AtomicBool::new(false));
    if let Err(err) = signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&terminate))
        .and_then(|_| signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&terminate)))
    {
        eprintln!("[EducAI][WARN] failed to register termination signal handlers: {err}");
    }
    let terminate_watcher = Arc::clone(&terminate);
    let signal_app = app.handle().clone();
    std::thread::spawn(move || {
        while !terminate_watcher.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
        }
        project_app::session_log::record("INFO", "app shutdown requested (signal)");
        if let Some(state) = signal_app.try_state::<SharedState>() {
            state.shutdown();
        }
        std::process::exit(0);
    });

    app.run(|app_handle, event| {
        // The app is exiting: deterministically stop every EducAI-owned child
        // (bundled `opencode serve`, bundled `cloudflared`, local publisher,
        // preview servers) before the process goes away. Relying only on Drop
        // is unsafe here because the managed `AppState` may never be dropped
        // on some exit paths, which previously leaked sidecar processes.
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            if let Some(state) = app_handle.try_state::<SharedState>() {
                state.shutdown();
            }
        }
    });
}

fn build_state(app: &tauri::AppHandle) -> Result<AppState, Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    std::fs::create_dir_all(&data_dir)?;
    let install_dir = std::env::current_exe()?
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    let path_env = std::env::var("PATH").unwrap_or_default();
    let (opencode_binary, cloudflared_binary) = project_app::app::apply_sidecar_locations(
        project_app::sidecar::resolve_sidecar_from_env("opencode", &install_dir, &path_env),
        project_app::sidecar::resolve_sidecar_from_env("cloudflared", &install_dir, &path_env),
    );
    let config = AppConfig {
        data_dir,
        opencode_binary,
        opencode_config_dir: app.path().app_data_dir()?.join("opencode"),
        opencode_port: ephemeral_port(),
        cloudflared_binary,
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
