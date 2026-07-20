//! Сброс данных приложения из Настроек: runtime, models, cache, settings.
//! Не удаляет саму программу (NSIS uninstall / portable-папка — отдельно).

use crate::config::{self, Settings};
use crate::runtime::{self, DATA_DIR_NAME, LEGACY_DATA_DIR_NAME};
use crate::server::{self, ServerState};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Deserialize)]
pub struct WipeOptions {
    /// settings.json (+ bak/tmp) → дефолты, снова wizard.
    pub settings: bool,
    /// Managed llama.cpp: `{app_dir}/runtime` (+ portable/legacy, если есть).
    pub runtime: bool,
    /// Только `{app_dir}/models` (не чужие папки из model_folders).
    pub models: bool,
    /// Кэш загрузок runtime: `runtime/.cache`.
    pub cache: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WipeResult {
    /// Что реально снесли (человекочитаемые пути/метки).
    pub removed: Vec<String>,
    /// Ошибки по отдельным шагам (частичный успех допустим).
    pub errors: Vec<String>,
    /// Актуальные настройки после операции (дефолт, если settings сброшены).
    pub settings: Settings,
}

fn push_err(errors: &mut Vec<String>, label: &str, e: impl std::fmt::Display) {
    errors.push(format!("{label}: {e}"));
}

/// Рекурсивно удалить каталог, если есть. Ok(true) = был и удалён.
fn remove_dir_if_exists(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

fn remove_file_if_exists(path: &Path) -> Result<bool, String> {
    if !path.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

fn local_appdata(name: &str) -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join(name))
}

fn roaming_appdata(name: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|b| PathBuf::from(b).join(name))
}

/// Корни, где мог лежать managed runtime / models (текущий + portable + legacy).
fn data_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(d) = runtime::app_dir() {
        roots.push(d);
    }
    if let Ok(exe) = runtime::exe_dir() {
        if !roots.iter().any(|r| r == &exe) {
            roots.push(exe);
        }
    }
    for name in [DATA_DIR_NAME, LEGACY_DATA_DIR_NAME] {
        if let Some(d) = local_appdata(name) {
            if !roots.iter().any(|r| r == &d) {
                roots.push(d);
            }
        }
    }
    roots
}

fn wipe_runtime(removed: &mut Vec<String>, errors: &mut Vec<String>) {
    for root in data_roots() {
        let rt = root.join("runtime");
        match remove_dir_if_exists(&rt) {
            Ok(true) => removed.push(rt.display().to_string()),
            Ok(false) => {}
            Err(e) => push_err(errors, "runtime", e),
        }
    }
}

fn wipe_models(removed: &mut Vec<String>, errors: &mut Vec<String>) {
    // Только дефолтная models рядом с app data — не трогаем произвольные model_folders.
    for root in data_roots() {
        let models = root.join("models");
        match remove_dir_if_exists(&models) {
            Ok(true) => removed.push(models.display().to_string()),
            Ok(false) => {}
            Err(e) => push_err(errors, "models", e),
        }
    }
}

fn wipe_cache(removed: &mut Vec<String>, errors: &mut Vec<String>) {
    for root in data_roots() {
        let cache = root.join("runtime").join(".cache");
        match remove_dir_if_exists(&cache) {
            Ok(true) => removed.push(cache.display().to_string()),
            Ok(false) => {}
            Err(e) => push_err(errors, "cache", e),
        }
    }
}

fn wipe_settings_files(app: &AppHandle, removed: &mut Vec<String>, errors: &mut Vec<String>) {
    // Текущий config dir (Tauri app_config_dir = identifier).
    if let Ok(dir) = app.path().app_config_dir() {
        for name in ["settings.json", "settings.json.bak", "settings.json.tmp"] {
            let p = dir.join(name);
            match remove_file_if_exists(&p) {
                Ok(true) => removed.push(p.display().to_string()),
                Ok(false) => {}
                Err(e) => push_err(errors, "settings", e),
            }
        }
    }
    // Legacy config (старый identifier в Roaming).
    if let Some(dir) = roaming_appdata(LEGACY_DATA_DIR_NAME) {
        for name in ["settings.json", "settings.json.bak", "settings.json.tmp"] {
            let p = dir.join(name);
            match remove_file_if_exists(&p) {
                Ok(true) => removed.push(p.display().to_string()),
                Ok(false) => {}
                Err(e) => push_err(errors, "settings(legacy)", e),
            }
        }
    }
}

/// Сбросить выбранные данные. Перед runtime/models гасим llama-server.
#[tauri::command]
pub fn wipe_app_data(
    app: AppHandle,
    state: State<ServerState>,
    options: WipeOptions,
) -> Result<WipeResult, String> {
    if !options.settings && !options.runtime && !options.models && !options.cache {
        return Err("Не выбрано ни одного пункта для удаления".into());
    }

    // Runtime/models нельзя сносить, пока сервер держит файлы/порт.
    if options.runtime || options.models {
        server::shutdown(&state);
    }

    let mut removed = Vec::new();
    let mut errors = Vec::new();

    if options.cache && !options.runtime {
        // Если runtime сносят целиком — cache уйдёт вместе с ним.
        wipe_cache(&mut removed, &mut errors);
    }
    if options.runtime {
        wipe_runtime(&mut removed, &mut errors);
    }
    if options.models {
        wipe_models(&mut removed, &mut errors);
    }

    let mut settings = config::load(&app);
    if options.settings {
        wipe_settings_files(&app, &mut removed, &mut errors);
        settings = Settings::default();
        // Сохраняем дефолт, чтобы путь config dir существовал и load() был стабилен.
        if let Err(e) = config::save(&app, &settings) {
            push_err(&mut errors, "settings(save)", e);
        } else if let Ok(p) = app.path().app_config_dir() {
            // save создал settings.json — в removed уже могут быть старые; ок.
            let _ = p;
        }
    } else if options.runtime {
        // Движок снесён, а настройки оставили — сбросить managed-поля.
        if settings.runtime_managed {
            settings.runtime_managed = false;
            settings.runtime_tag = None;
            settings.runtime_backend = None;
            settings.llama_dir = None;
            if let Err(e) = config::save(&app, &settings) {
                push_err(&mut errors, "settings(update)", e);
            }
        }
    }

    Ok(WipeResult {
        removed,
        errors,
        settings,
    })
}
