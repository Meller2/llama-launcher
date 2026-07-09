// LlamaLauncher — Tauri backend.
// Модули доменной логики: настройки, скан моделей, жизненный цикл сервера.

mod autoconfig;
mod config;
mod hardware;
mod hf;
mod models;
mod server;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(server::ServerState::default())
        .manage(hf::DownloadState::default())
        .invoke_handler(tauri::generate_handler![
            config::load_settings,
            config::save_settings,
            config::validate_llama_dir,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
