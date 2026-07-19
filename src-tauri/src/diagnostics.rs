//! Диагностический отчёт: снимок состояния приложения для отладки/багрепортов
//! (железо, managed runtime, статус сервера, свободное место).

use crate::hardware::detect_hardware;
use crate::runtime;
use crate::server::{self, ServerState};
use serde::Serialize;
use std::path::Path;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub gpu_name: Option<String>,
    pub vram_bytes: u64,
    pub total_ram_bytes: u64,
    pub logical_cores: u32,
    pub runtime_installed: bool,
    pub runtime_tag: Option<String>,
    pub runtime_backend: Option<String>,
    pub runtime_dir: Option<String>,
    pub app_dir: String,
    pub default_models_dir: String,
    pub free_disk_bytes: Option<u64>,
    pub server_running: bool,
    pub server_ready: bool,
    pub server_port: Option<u16>,
    pub server_model: Option<String>,
    pub last_exit_code: Option<i32>,
}

#[tauri::command]
pub fn diagnostic_report(state: State<ServerState>) -> DiagnosticReport {
    let hw = detect_hardware();
    let rt = runtime::runtime_status().ok();
    let srv = server::status(&state);

    let app_dir = rt.as_ref().map(|r| r.app_dir.clone()).unwrap_or_default();
    let free_disk_bytes = if app_dir.is_empty() {
        None
    } else {
        runtime::free_space_bytes(Path::new(&app_dir))
    };

    DiagnosticReport {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        gpu_name: hw.gpu.as_ref().map(|g| g.name.clone()),
        vram_bytes: hw.gpu.as_ref().map(|g| g.vram_bytes).unwrap_or(0),
        total_ram_bytes: hw.total_ram_bytes,
        logical_cores: hw.logical_cores,
        runtime_installed: rt.as_ref().map(|r| r.installed).unwrap_or(false),
        runtime_tag: rt.as_ref().and_then(|r| r.tag.clone()),
        runtime_backend: rt.as_ref().and_then(|r| r.backend.clone()),
        runtime_dir: rt.as_ref().and_then(|r| r.llama_dir.clone()),
        app_dir: app_dir.clone(),
        default_models_dir: rt
            .as_ref()
            .map(|r| r.default_models_dir.clone())
            .unwrap_or_default(),
        free_disk_bytes,
        server_running: srv.running,
        server_ready: srv.ready,
        server_port: srv.port,
        server_model: srv.model_name,
        last_exit_code: state.last_exit_code(),
    }
}
