//! Настройки приложения: структура Settings + load/save.
//!
//! Путь к settings.json = `runtime::app_dir()` (как runtime/models):
//!   1) рядом с exe, если папка writable → **настоящий portable**;
//!   2) иначе `%LOCALAPPDATA%/com.ggflow.app` (NSIS / Program Files).
//!
//! Старые пути (Tauri Roaming / legacy identifier) читаются один раз при миграции.

use crate::runtime::{self, DATA_DIR_NAME, LEGACY_DATA_DIR_NAMES};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::AppHandle;

/// Дефолты запуска модели (маппинг флагов из llama.bat).
/// Пользователь может переопределить в UI; auto_config переопределит под железо.
///
/// Все поля с `#[serde(default)]`: если старый settings.json не содержит поля,
/// подставится дефолт этого поля, а не потеряется весь конфиг.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LaunchDefaults {
    /// Контекст в токенах.
    pub ctx: u32,
    /// Квант KV-кэша: "f16" | "q8_0" | "q4_0".
    pub kv_quant: String,
    /// Число потоков CPU (-t).
    pub threads: u32,
    /// Слои на GPU (-ngl). 99 = всё.
    pub ngl: u32,
    /// HTTP-порт llama-server.
    pub port: u16,
    /// Включить нативные инструменты (--tools all --ui-mcp-proxy).
    pub tools: bool,
}

impl Default for LaunchDefaults {
    fn default() -> Self {
        // Рабочая связка из llama.bat: 16k ctx, q4_0 KV, 6 потоков, всё на GPU.
        Self {
            ctx: 16384,
            kv_quant: "q4_0".to_string(),
            threads: 6,
            ngl: 99,
            port: 8080,
            tools: false,
        }
    }
}

/// Версия wizard'а первоначальной настройки.
/// Старые settings.json без поля → 0 → wizard покажется снова (даже если onboarded=true).
pub const SETUP_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Папка с llama-server.exe (managed runtime или ручной путь).
    pub llama_dir: Option<String>,
    /// Папки, где искать .gguf модели.
    pub model_folders: Vec<String>,
    /// Дефолты запуска.
    pub defaults: LaunchDefaults,
    /// Пройден ли онбординг (legacy-флаг; смотри ещё setup_version).
    pub onboarded: bool,
    /// Какую версию wizard'а пользователь завершил. 0 = ещё не проходил актуальный.
    pub setup_version: u32,
    /// Runtime поставлен лаунчером (portable, рядом с exe).
    pub runtime_managed: bool,
    /// Тег релиза llama.cpp, напр. "b9952".
    pub runtime_tag: Option<String>,
    /// Backend: "cpu" | "vulkan" | "cuda-12.4".
    pub runtime_backend: Option<String>,
    /// Язык UI: "ru" | "en".
    pub locale: String,
    /// Уровень: "beginner" | "intermediate" | "expert".
    /// Для старых конфигов без поля — "expert", чтобы не урезать уже настроенный UI.
    #[serde(default = "default_expertise")]
    pub expertise: String,
    /// Открывать Web-UI llama-server, когда сервер стал ready.
    pub open_ui_on_ready: bool,
}

fn default_expertise() -> String {
    // Старые settings.json без поля: сохраняем полный UI.
    "expert".into()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            llama_dir: None,
            model_folders: Vec::new(),
            defaults: LaunchDefaults::default(),
            onboarded: false,
            setup_version: 0,
            runtime_managed: false,
            runtime_tag: None,
            runtime_backend: None,
            locale: "ru".into(),
            expertise: "beginner".into(), // новый пользователь — с wizard'а
            open_ui_on_ready: true,
        }
    }
}

/// Каталог конфигурации = data root (portable-first).
pub fn config_dir() -> Result<PathBuf, String> {
    runtime::app_dir()
}

/// Путь к файлу настроек: `{app_dir}/settings.json`.
pub fn settings_path() -> Result<PathBuf, String> {
    Ok(config_dir()?.join("settings.json"))
}

fn roaming_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|b| PathBuf::from(b).join(name))
}

fn local_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|b| PathBuf::from(b).join(name))
}

/// Все известные места, где когда-либо мог лежать settings.json
/// (текущий + миграции). Для wipe и one-shot migrate.
pub fn settings_search_paths() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.iter().any(|x| x == &p) {
            out.push(p);
        }
    };

    if let Ok(p) = settings_path() {
        push(p);
    }
    // Рядом с exe — даже если app_dir ушёл в LocalAppData (на всякий).
    if let Ok(exe) = runtime::exe_dir() {
        push(exe.join("settings.json"));
    }
    // Текущий + legacy id (Roaming / LocalAppData).
    let mut names: Vec<&str> = vec![DATA_DIR_NAME];
    names.extend_from_slice(LEGACY_DATA_DIR_NAMES);
    for name in names {
        if let Some(d) = roaming_dir(name) {
            push(d.join("settings.json"));
        }
        if let Some(d) = local_dir(name) {
            push(d.join("settings.json"));
        }
    }
    out
}

/// Одноразовая миграция: если канонического settings.json нет —
/// копируем из первого найденного legacy-пути.
fn migrate_legacy_settings() {
    let Ok(canonical) = settings_path() else {
        return;
    };
    if canonical.is_file() {
        return;
    }

    for candidate in settings_search_paths() {
        if candidate == canonical || !candidate.is_file() {
            continue;
        }
        if let Some(parent) = canonical.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if std::fs::copy(&candidate, &canonical).is_ok() {
            return;
        }
    }
}

/// Чтение настроек. Если файла нет или он битый — возвращаем дефолт (не онбордед).
pub fn load(_app: &AppHandle) -> Settings {
    migrate_legacy_settings();
    let path = match settings_path() {
        Ok(p) => p,
        Err(_) => return Settings::default(),
    };
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

/// Атомарная (насколько позволяет ОС) запись: tmp → replace target.
///
/// На Windows `std::fs::rename` **не** заменяет существующий файл (в отличие от
/// POSIX). Поэтому: пишем `.json.tmp`, сдвигаем старый `settings.json` в
/// `.json.bak`, затем переименовываем tmp → target. При сбое rename
/// восстанавливаем bak.
pub fn save(_app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Не удалось создать папку конфигурации: {e}"))?;
    }
    let json = serde_json::to_string_pretty(settings)
        .map_err(|e| format!("Ошибка сериализации настроек: {e}"))?;

    let tmp = path.with_extension("json.tmp");
    let bak = path.with_extension("json.bak");
    std::fs::write(&tmp, json.as_bytes())
        .map_err(|e| format!("Не удалось записать настройки: {e}"))?;

    // Путь свободен (первый save) — обычный rename.
    if !path.exists() {
        std::fs::rename(&tmp, &path).map_err(|e| format!("Не удалось сохранить настройки: {e}"))?;
        return Ok(());
    }

    // Target уже есть: убрать старый bak, сдвинуть target → bak, tmp → target.
    let _ = std::fs::remove_file(&bak);
    std::fs::rename(&path, &bak).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Не удалось сохранить настройки: {e}")
    })?;
    match std::fs::rename(&tmp, &path) {
        Ok(()) => {
            let _ = std::fs::remove_file(&bak);
            Ok(())
        }
        Err(e) => {
            // Откат: вернуть предыдущий settings.json, если возможно.
            let _ = std::fs::rename(&bak, &path);
            let _ = std::fs::remove_file(&tmp);
            Err(format!("Не удалось сохранить настройки: {e}"))
        }
    }
}

/// Проверка, что в папке есть llama-server.exe.
pub fn validate_llama_dir_impl(dir: &str) -> bool {
    let exe = Path::new(dir).join("llama-server.exe");
    exe.is_file()
}

// ── Tauri-команды ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn load_settings(app: AppHandle) -> Settings {
    load(&app)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    save(&app, &settings)
}

/// Есть ли llama-server.exe в указанной папке (валидация онбординга).
#[tauri::command]
pub fn validate_llama_dir(dir: String) -> bool {
    validate_llama_dir_impl(&dir)
}

/// Актуальная версия wizard'а (для фронта).
#[tauri::command]
pub fn setup_version() -> u32 {
    SETUP_VERSION
}
