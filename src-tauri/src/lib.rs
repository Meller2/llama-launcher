// LlamaLauncher — Tauri backend.
// Модули доменной логики: настройки, скан моделей, жизненный цикл сервера.

mod autoconfig;
mod config;
mod hardware;
mod hf;
mod models;
mod runtime;
mod server;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(server::ServerState::default())
        .manage(hf::DownloadState::default())
        .manage(runtime::RuntimeInstallState::default())
        // При закрытии окна убиваем запущенный llama-server, чтобы он не остался
        // осиротевшим процессом, держащим порт и VRAM.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<server::ServerState>();
                server::shutdown(&state);
            }
        })
        .invoke_handler(tauri::generate_handler![
            config::load_settings,
            config::save_settings,
            config::validate_llama_dir,
            config::setup_version,
            models::scan_models,
            models::read_gguf_meta,
            server::start_server,
            server::stop_server,
            server::server_status,
            hardware::detect_hardware,
            autoconfig::auto_config,
            hf::hf_search,
            hf::hf_list_files,
            hf::hf_download,
            hf::hf_cancel_download,
            runtime::runtime_status,
            runtime::runtime_install,
            runtime::runtime_cancel_install,
            runtime::ensure_default_models_dir,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
