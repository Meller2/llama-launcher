//! Managed runtime llama.cpp: portable-установка рядом с exe приложения.
//!
//! Скачивает **закреплённый** релиз llama.cpp (не «latest»), проверяет SHA-256,
//! распаковывает в staging, smoke-check, затем атомарно подменяет рабочую папку.
//!
//! Путь: `{app_dir}/runtime/{tag}/{backend}/`
//!
//! Выбор backend (Windows x64):
//!   NVIDIA → CUDA 12.4 (+ cudart DLLs)
//!   иначе GPU → Vulkan
//!   без GPU → CPU
//!
//! Portable: всё лежит рядом с программой — можно носить на флешке.

use crate::hardware::detect_hardware;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::fs::File as TokioFile;
use tokio::io::AsyncWriteExt;

const GH: &str = "https://github.com/ggml-org/llama.cpp";
const GH_API: &str = "https://api.github.com/repos/ggml-org/llama.cpp";
const UA: &str = concat!("GGFlow/", env!("CARGO_PKG_VERSION"));
const EMIT_STEP: u64 = 2_000_000;
const SERVER_EXE: &str = "llama-server.exe";

/// Закреплённый тег llama.cpp. Обновлять вместе с `PINNED_DIGESTS`.
/// Не используем /releases/latest — смена CLI/имён архивов не должна ломать лаунчер внезапно.
pub const PINNED_TAG: &str = "b9963";

/// Доверенные SHA-256 (hex) zip-ассетов pinned-релиза (Windows x64).
/// Источник: GitHub Releases digests на момент фиксации; сверяем после скачивания.
const PINNED_DIGESTS: &[(&str, &str)] = &[
    (
        "llama-b9963-bin-win-cpu-x64.zip",
        "267e1fc1a043cb9cfbd5dbc452b6bb1f00331108f848b0a3c6fbe0dade52d928",
    ),
    (
        "llama-b9963-bin-win-vulkan-x64.zip",
        "6951dc63f9fb5227ce987c15d6651589eddf1df2671ff57cdc66c582fd63b019",
    ),
    (
        "llama-b9963-bin-win-cuda-12.4-x64.zip",
        "d6a67339715ffa95820be6a0452c9151fb77fb635f86add9ba6cf05777df72d6",
    ),
    (
        "cudart-llama-bin-win-cuda-12.4-x64.zip",
        "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6",
    ),
];

// ── Типы ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeBackend {
    Cpu,
    Vulkan,
    Cuda12,
}

impl RuntimeBackend {
    pub fn id(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Vulkan => "vulkan",
            Self::Cuda12 => "cuda-12.4",
        }
    }

    pub fn label_ru(&self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Vulkan => "Vulkan (GPU)",
            Self::Cuda12 => "CUDA 12.4 (NVIDIA)",
        }
    }

    fn from_id(s: &str) -> Option<Self> {
        match s {
            "cpu" => Some(Self::Cpu),
            "vulkan" => Some(Self::Vulkan),
            "cuda-12.4" | "cuda12" | "cuda" => Some(Self::Cuda12),
            _ => None,
        }
    }
}

/// Статус managed-runtime для UI.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeStatus {
    /// Папка с llama-server.exe (если уже стоит).
    pub llama_dir: Option<String>,
    pub installed: bool,
    pub tag: Option<String>,
    pub backend: Option<String>,
    pub backend_label: Option<String>,
    /// Рекомендуемый backend под текущее железо.
    pub recommended_backend: String,
    pub recommended_label: String,
    /// Корень данных (рядом с exe, если writable; иначе LocalAppData).
    pub app_dir: String,
    /// Дефолтная папка моделей `{app_dir}/models`.
    pub default_models_dir: String,
    /// Корень runtime `{app_dir}/runtime`.
    pub runtime_root: String,
    pub latest_tag: Option<String>,
    pub update_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeUpdate {
    pub current_tag: Option<String>,
    pub latest_tag: String,
    pub available: bool,
}

/// Событие `runtime-progress`.
#[derive(Debug, Clone, Serialize)]
struct Progress {
    /// Человекочитаемый этап: «Скачиваю…», «Распаковываю…».
    stage: String,
    file: String,
    downloaded: u64,
    total: u64,
    done: bool,
    error: Option<String>,
    canceled: bool,
}

#[derive(Default)]
pub struct RuntimeInstallState {
    cancel: AtomicBool,
    active: Mutex<bool>,
}

impl RuntimeInstallState {
    fn lock_active(&self) -> std::sync::MutexGuard<'_, bool> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

enum DlErr {
    Canceled,
    Failed(String),
}

// ── Пути (portable-first, fallback в AppData) ────────────────────────────────

/// Каталог, где лежит exe приложения. В dev — `target/debug`, в release — папка программы.
pub fn exe_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .map_err(|e| format!("Не удалось определить путь к программе: {e}"))?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "Некорректный путь к программе".into())
}

/// Можно ли писать в каталог (portable-проверка для Program Files и т.п.).
fn dir_is_writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        // Попробуем создать — если нельзя, не writable.
        if std::fs::create_dir_all(dir).is_err() {
            return false;
        }
    }
    let probe = dir.join(".ll-write-test");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Имя папки данных — совпадает с `tauri.conf.json` → `identifier`.
pub const DATA_DIR_NAME: &str = "com.ggflow.app";
/// Прежние identifier: ищем runtime/settings, но больше не пишем сюда.
pub const LEGACY_DATA_DIR_NAMES: &[&str] =
    &["com.llamalauncher.app", "com.ilzat.llama-launcher"];

/// `%LOCALAPPDATA%/<name>` — fallback, когда рядом с exe писать нельзя (Program Files).
fn local_data_dir(name: &str) -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "Не удалось определить LOCALAPPDATA".to_string())?;
    Ok(base.join(name))
}

/// Корень данных приложения:
/// 1) рядом с exe, если туда можно писать (portable / dev);
/// 2) иначе `%LOCALAPPDATA%/com.ggflow.app` (NSIS в Program Files).
pub fn app_dir() -> Result<PathBuf, String> {
    let beside = exe_dir()?;
    if dir_is_writable(&beside) {
        return Ok(beside);
    }
    // Fallback: локальные данные пользователя (не требует админа).
    let dir = local_data_dir(DATA_DIR_NAME)?;
    ensure_dir(&dir)?;
    Ok(dir)
}

pub fn runtime_root() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("runtime"))
}

pub fn default_models_dir() -> Result<PathBuf, String> {
    Ok(app_dir()?.join("models"))
}

/// `{runtime}/{tag}/{backend_id}/`
fn backend_dir(tag: &str, backend: &RuntimeBackend) -> Result<PathBuf, String> {
    Ok(runtime_root()?.join(tag).join(backend.id()))
}

fn ensure_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("Не удалось создать «{}»: {e}", path.display()))
}

/// Найти llama-server.exe в дереве (после распаковки zip может быть вложенная папка).
fn find_server_exe(root: &Path) -> Option<PathBuf> {
    if !root.is_dir() {
        return None;
    }
    let direct = root.join(SERVER_EXE);
    if direct.is_file() {
        return Some(direct);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for ent in entries.flatten() {
            let p = ent.path();
            if p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(SERVER_EXE))
            {
                return Some(p);
            }
            if p.is_dir() {
                stack.push(p);
            }
        }
    }
    None
}

fn is_installed_at(dir: &Path) -> bool {
    dir.join(SERVER_EXE).is_file()
}

// ── Выбор backend ────────────────────────────────────────────────────────────

fn recommend_backend() -> RuntimeBackend {
    let hw = detect_hardware();
    match hw.gpu {
        Some(g) => {
            let name = g.name.to_lowercase();
            if name.contains("nvidia")
                || name.contains("geforce")
                || name.contains("rtx")
                || name.contains("gtx")
                || name.contains("quadro")
                || name.contains("tesla")
            {
                RuntimeBackend::Cuda12
            } else if g.vram_bytes > 0 {
                // AMD / Intel / прочие с VRAM — Vulkan.
                RuntimeBackend::Vulkan
            } else {
                RuntimeBackend::Cpu
            }
        }
        None => RuntimeBackend::Cpu,
    }
}

// ── GitHub release ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    assets: Vec<GhAsset>,
}

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        // GitHub иногда долго отдаёт редиректы на objects.githubusercontent.com.
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600)) // крупные zip (CUDA ~250+ МБ)
        .build()
        .map_err(|e| format!("Не удалось создать HTTP-клиент: {e}"))
}

/// Скачать метаданные **закреплённого** релиза (не latest).
async fn fetch_pinned_release() -> Result<GhRelease, String> {
    let url = format!("{GH_API}/releases/tags/{PINNED_TAG}");
    let resp = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Не удалось связаться с GitHub: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(format!(
            "Закреплённый релиз llama.cpp «{PINNED_TAG}» не найден на GitHub. Обновите GGFlow."
        ));
    }
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub вернул {} при запросе релиза {PINNED_TAG}",
            resp.status()
        ));
    }
    let release: GhRelease = resp
        .json()
        .await
        .map_err(|e| format!("Не удалось разобрать ответ GitHub: {e}"))?;
    if release.tag_name != PINNED_TAG {
        return Err(format!(
            "Ожидался тег {PINNED_TAG}, GitHub вернул «{}».",
            release.tag_name
        ));
    }
    Ok(release)
}

#[tauri::command]
pub fn runtime_check_update() -> Result<RuntimeUpdate, String> {
    let current = find_existing_install().map(|(_, tag, _)| tag);
    // Обновляем только на заранее проверенную версию, для которой в приложении
    // есть SHA-256 всех архивов. Это защищает от внезапного несовместимого
    // релиза llama.cpp и не требует переустановки самого приложения.
    let latest = PINNED_TAG.to_string();
    Ok(RuntimeUpdate {
        available: current.as_deref() != Some(latest.as_str()),
        current_tag: current,
        latest_tag: latest,
    })
}

fn pinned_digest_for(asset_name: &str) -> Option<&'static str> {
    PINNED_DIGESTS
        .iter()
        .find(|(n, _)| *n == asset_name)
        .map(|(_, d)| *d)
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// SHA-256 файла → lowercase hex.
fn file_sha256_hex(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Не удалось открыть «{}» для проверки: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Ошибка чтения при проверке SHA-256: {e}"))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(bytes_to_hex(&hasher.finalize()))
}

/// Сверить zip с доверенным digest (pinned table). Без совпадения — удаляем файл.
fn verify_zip_digest(path: &Path, asset_name: &str) -> Result<(), String> {
    let expected = pinned_digest_for(asset_name).ok_or_else(|| {
        format!(
            "Нет доверенного SHA-256 для «{asset_name}». Обновите GGFlow (pinned digests)."
        )
    })?;
    let got = file_sha256_hex(path)?;
    if !got.eq_ignore_ascii_case(expected) {
        let _ = std::fs::remove_file(path);
        return Err(format!(
            "Проверка целостности «{asset_name}» не пройдена (SHA-256 не совпал). Файл удалён — попробуйте снова."
        ));
    }
    Ok(())
}

/// Имена asset'ов для backend (основной zip + опциональный cudart).
fn asset_names(tag: &str, backend: &RuntimeBackend) -> (String, Option<String>) {
    match backend {
        RuntimeBackend::Cpu => (format!("llama-{tag}-bin-win-cpu-x64.zip"), None),
        RuntimeBackend::Vulkan => (format!("llama-{tag}-bin-win-vulkan-x64.zip"), None),
        RuntimeBackend::Cuda12 => (
            format!("llama-{tag}-bin-win-cuda-12.4-x64.zip"),
            Some("cudart-llama-bin-win-cuda-12.4-x64.zip".into()),
        ),
    }
}

fn find_asset<'a>(release: &'a GhRelease, name: &str) -> Result<&'a GhAsset, String> {
    release
        .assets
        .iter()
        .find(|a| a.name == name)
        .ok_or_else(|| {
            format!(
                "В релизе {} нет файла «{name}». Попробуйте другой backend или позже.",
                release.tag_name
            )
        })
}

// ── Прогресс / скачивание ────────────────────────────────────────────────────

/// Единый набор параметров для одного события прогресса установки —
/// разумнее не выносить в отдельный struct ради одной внутренней функции.
#[allow(clippy::too_many_arguments)]
fn emit(
    app: &AppHandle,
    stage: &str,
    file: &str,
    downloaded: u64,
    total: u64,
    done: bool,
    error: Option<String>,
    canceled: bool,
) {
    let _ = app.emit(
        "runtime-progress",
        Progress {
            stage: stage.to_string(),
            file: file.to_string(),
            downloaded,
            total,
            done,
            error,
            canceled,
        },
    );
}

async fn stream_to_file(
    app: &AppHandle,
    state: &RuntimeInstallState,
    url: &str,
    dest: &Path,
    stage: &str,
    label: &str,
) -> Result<u64, DlErr> {
    if let Some(parent) = dest.parent() {
        ensure_dir(parent).map_err(DlErr::Failed)?;
    }

    let mut resp = client()
        .map_err(DlErr::Failed)?
        .get(url)
        .send()
        .await
        .map_err(|e| DlErr::Failed(format!("Сеть недоступна: {e}")))?;

    if !resp.status().is_success() {
        return Err(DlErr::Failed(format!(
            "Сервер вернул {} при скачивании {label}",
            resp.status()
        )));
    }

    let total = resp.content_length().unwrap_or(0);
    let mut out = TokioFile::create(dest)
        .await
        .map_err(|e| DlErr::Failed(format!("Не удалось создать файл: {e}")))?;

    let mut downloaded = 0u64;
    let mut last_emit = 0u64;
    emit(app, stage, label, 0, total, false, None, false);

    loop {
        if state.cancel.load(Ordering::SeqCst) {
            let _ = out.flush().await;
            let _ = tokio::fs::remove_file(dest).await;
            return Err(DlErr::Canceled);
        }
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка при скачивании: {e}")))?;
        let Some(chunk) = chunk else { break };
        out.write_all(&chunk)
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка записи: {e}")))?;
        downloaded += chunk.len() as u64;
        if downloaded - last_emit >= EMIT_STEP {
            last_emit = downloaded;
            emit(app, stage, label, downloaded, total, false, None, false);
        }
    }
    out.flush()
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка сброса на диск: {e}")))?;

    if total > 0 && downloaded != total {
        let _ = tokio::fs::remove_file(dest).await;
        return Err(DlErr::Failed(format!(
            "Файл скачан не полностью: {downloaded} из {total} байт"
        )));
    }
    emit(
        app,
        stage,
        label,
        downloaded,
        downloaded.max(total),
        false,
        None,
        false,
    );
    Ok(downloaded)
}

// ── Распаковка ───────────────────────────────────────────────────────────────

/// Распаковать zip **только** в `dest` (обычно staging). Не трогает рабочую установку.
/// `merge=false` — очищает dest перед записью; `merge=true` — дополняет (cudart).
fn extract_zip(zip_path: &Path, dest: &Path, merge: bool) -> Result<(), String> {
    if !merge && dest.exists() {
        std::fs::remove_dir_all(dest)
            .map_err(|e| format!("Не удалось очистить staging «{}»: {e}", dest.display()))?;
    }
    ensure_dir(dest)?;

    let file = File::open(zip_path).map_err(|e| format!("Не удалось открыть zip: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Повреждённый zip-архив: {e}"))?;

    // Внутренний tmp рядом со staging (не рядом с live dest).
    let tmp = dest.parent().unwrap_or(dest).join(format!(
        ".extracting-{}",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("rt")
    ));
    if tmp.exists() {
        let _ = std::fs::remove_dir_all(&tmp);
    }
    ensure_dir(&tmp)?;

    let extract_result = (|| -> Result<PathBuf, String> {
        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("Ошибка чтения zip: {e}"))?;
            let name = entry
                .enclosed_name()
                .ok_or_else(|| "Небезопасный путь внутри zip".to_string())?
                .to_path_buf();

            let out_path = tmp.join(&name);
            if entry.is_dir() {
                ensure_dir(&out_path)?;
                continue;
            }
            if let Some(parent) = out_path.parent() {
                ensure_dir(parent)?;
            }
            let mut outfile =
                File::create(&out_path).map_err(|e| format!("Не удалось создать файл: {e}"))?;
            io::copy(&mut entry, &mut outfile).map_err(|e| format!("Ошибка распаковки: {e}"))?;
        }

        // Если есть llama-server.exe — берём его каталог; иначе всё содержимое tmp (cudart).
        let source = if let Some(exe) = find_server_exe(&tmp) {
            exe.parent()
                .ok_or_else(|| "Некорректный путь к llama-server.exe".to_string())?
                .to_path_buf()
        } else {
            flatten_single_subdir(&tmp)
        };
        Ok(source)
    })();

    match extract_result {
        Ok(source) => {
            let copy_res = copy_dir_contents(&source, dest);
            let _ = std::fs::remove_dir_all(&tmp);
            copy_res
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&tmp);
            if !merge {
                let _ = std::fs::remove_dir_all(dest);
            }
            Err(e)
        }
    }
}

/// Атомарная (насколько позволяет FS) подмена live ← staging.
/// Старая live уходит в `*.bak`, при ошибке rename — восстанавливается.
fn swap_staging_into_live(staging: &Path, live: &Path) -> Result<(), String> {
    if !staging.is_dir() {
        return Err(format!("Staging-папка не найдена: {}", staging.display()));
    }
    let parent = live
        .parent()
        .ok_or_else(|| "Некорректный путь установки runtime".to_string())?;
    ensure_dir(parent)?;

    let bak_name = format!(
        "{}.bak",
        live.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("backend")
    );
    let bak = parent.join(bak_name);
    if bak.exists() {
        let _ = std::fs::remove_dir_all(&bak);
    }

    if live.exists() {
        std::fs::rename(live, &bak).map_err(|e| {
            format!(
                "Не удалось сдвинуть старую установку в сторону: {e}. Закройте llama-server, если он запущен."
            )
        })?;
    }

    match std::fs::rename(staging, live) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&bak);
            Ok(())
        }
        Err(e) => {
            // Откат: вернуть старую live, если была.
            if bak.exists() {
                let _ = std::fs::rename(&bak, live);
            }
            Err(format!(
                "Не удалось активировать новую установку: {e}. Прежняя (если была) восстановлена."
            ))
        }
    }
}

/// Быстрая проверка, что бинарник запускается (не «пустой»/битый PE).
fn smoke_test_server(dir: &Path) -> Result<(), String> {
    let exe = dir.join(SERVER_EXE);
    if !exe.is_file() {
        return Err("llama-server.exe отсутствует в staging.".into());
    }
    let meta = std::fs::metadata(&exe)
        .map_err(|e| format!("Не удалось прочитать llama-server.exe: {e}"))?;
    // Современные сборки llama.cpp — тонкий exe-загрузчик (~9 КБ) + отдельная
    // llama-server-impl.dll с основной логикой, так что монолитный размер exe
    // ничего не говорит о целостности. Проверяем только «не пустой файл» —
    // реальная проверка целостности ниже: реальный запуск `--version`.
    if meta.len() == 0 {
        return Err(
            "llama-server.exe пустой (0 байт) — архив повреждён или распаковка не завершилась."
                .into(),
        );
    }

    let mut cmd = Command::new(&exe);
    cmd.arg("--version")
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    match cmd.output() {
        Ok(out) => {
            // --version у разных сборок может вернуть 0 или ненулевой код — главное, что процесс стартовал.
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if stdout.trim().is_empty() && stderr.trim().is_empty() && !out.status.success() {
                return Err(format!(
                    "llama-server.exe не отвечает на --version (размер файла: {} байт, код выхода: {}). Установка отклонена.",
                    meta.len(),
                    out.status.code().map_or_else(|| "unknown".to_string(), |c| c.to_string())
                ));
            }
            Ok(())
        }
        Err(e) => Err(format!(
            "Не удалось запустить llama-server.exe для проверки: {e}"
        )),
    }
}

fn flatten_single_subdir(root: &Path) -> PathBuf {
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    // Один подкаталог и ничего больше на верхнем уровне → спуститься.
    if entries.len() == 1 && entries[0].is_dir() {
        return entries.swap_remove(0);
    }
    root.to_path_buf()
}

fn copy_dir_contents(src: &Path, dest: &Path) -> Result<(), String> {
    ensure_dir(dest)?;
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for ent in std::fs::read_dir(&dir).map_err(|e| format!("read_dir: {e}"))? {
            let ent = ent.map_err(|e| format!("read_dir entry: {e}"))?;
            let p = ent.path();
            let rel = p.strip_prefix(src).unwrap_or(&p);
            let target = dest.join(rel);
            if p.is_dir() {
                ensure_dir(&target)?;
                stack.push(p);
            } else {
                if let Some(parent) = target.parent() {
                    ensure_dir(parent)?;
                }
                std::fs::copy(&p, &target)
                    .map_err(|e| format!("copy {} → {}: {e}", p.display(), target.display()))?;
            }
        }
    }
    Ok(())
}

// ── Статус / установка ───────────────────────────────────────────────────────

/// Прочитать managed-метаданные, если есть.
fn read_meta(dir: &Path) -> Option<(String, String)> {
    let meta_path = dir.join("ll-runtime.json");
    let s = std::fs::read_to_string(meta_path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    Some((
        v.get("tag")?.as_str()?.to_string(),
        v.get("backend")?.as_str()?.to_string(),
    ))
}

fn write_meta(dir: &Path, tag: &str, backend: &RuntimeBackend) -> Result<(), String> {
    let meta = serde_json::json!({
        "tag": tag,
        "backend": backend.id(),
        "source": GH,
    });
    std::fs::write(
        dir.join("ll-runtime.json"),
        serde_json::to_string_pretty(&meta).unwrap_or_default(),
    )
    .map_err(|e| format!("Не удалось записать метаданные runtime: {e}"))
}

/// Найти уже установленный managed runtime (обход runtime/*/*).
/// Смотрим portable (рядом с exe), текущий LocalAppData и legacy-папку.
fn find_existing_install() -> Option<(PathBuf, String, String)> {
    let mut roots = Vec::new();
    if let Ok(r) = runtime_root() {
        roots.push(r);
    }
    // Если app_dir ушёл в LocalAppData — всё равно проверим рядом с exe.
    if let Ok(exe) = exe_dir() {
        let beside = exe.join("runtime");
        if !roots.iter().any(|r| r == &beside) {
            roots.push(beside);
        }
    }
    // Старые установки до смены identifier / ребренда.
    for name in LEGACY_DATA_DIR_NAMES {
        if let Ok(legacy) = local_data_dir(name) {
            let legacy_rt = legacy.join("runtime");
            if !roots.iter().any(|r| r == &legacy_rt) {
                roots.push(legacy_rt);
            }
        }
    }
    let mut best: Option<(PathBuf, String, String, std::time::SystemTime)> = None;
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(tags) = std::fs::read_dir(&root) else {
            continue;
        };
        for tag_ent in tags.flatten() {
            if !tag_ent.path().is_dir() {
                continue;
            }
            // Пропускаем служебный .cache
            if tag_ent.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let tag_name = tag_ent.file_name().to_string_lossy().to_string();
            let Ok(backends) = std::fs::read_dir(tag_ent.path()) else {
                continue;
            };
            for be in backends.flatten() {
                let dir = be.path();
                if !is_installed_at(&dir) {
                    continue;
                }
                let backend_id = if let Some((_, b)) = read_meta(&dir) {
                    b
                } else {
                    be.file_name().to_string_lossy().to_string()
                };
                let mtime = dir
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                let cand = (dir, tag_name.clone(), backend_id, mtime);
                best = match best {
                    Some(b) if b.3 >= cand.3 => Some(b),
                    _ => Some(cand),
                };
            }
        }
    }
    best.map(|(p, t, b, _)| (p, t, b))
}

fn build_status() -> Result<RuntimeStatus, String> {
    let app = app_dir()?;
    let models = default_models_dir()?;
    let rt = runtime_root()?;
    let recommended = recommend_backend();

    let (llama_dir, installed, tag, backend, backend_label) =
        if let Some((dir, t, b)) = find_existing_install() {
            let label = RuntimeBackend::from_id(&b)
                .map(|x| x.label_ru().to_string())
                .unwrap_or_else(|| b.clone());
            (
                Some(dir.to_string_lossy().to_string()),
                true,
                Some(t),
                Some(b),
                Some(label),
            )
        } else {
            (None, false, None, None, None)
        };

    Ok(RuntimeStatus {
        llama_dir,
        installed,
        tag,
        backend,
        backend_label,
        recommended_backend: recommended.id().to_string(),
        recommended_label: recommended.label_ru().to_string(),
        app_dir: app.to_string_lossy().to_string(),
        default_models_dir: models.to_string_lossy().to_string(),
        runtime_root: rt.to_string_lossy().to_string(),
        latest_tag: None,
        update_available: false,
    })
}

/// Цепочка отката для авто-установки: от рекомендованного backend'а вниз
/// к менее требовательным. Если CUDA-архив скачался, но smoke-test не прошёл
/// (нет DLL, старый драйвер, битый бинарник) — не оставляем пользователя
/// с голой ошибкой, а пробуем Vulkan, затем CPU.
fn fallback_chain(recommended: RuntimeBackend) -> Vec<RuntimeBackend> {
    match recommended {
        RuntimeBackend::Cuda12 => vec![
            RuntimeBackend::Cuda12,
            RuntimeBackend::Vulkan,
            RuntimeBackend::Cpu,
        ],
        RuntimeBackend::Vulkan => vec![RuntimeBackend::Vulkan, RuntimeBackend::Cpu],
        RuntimeBackend::Cpu => vec![RuntimeBackend::Cpu],
    }
}

/// Установка с авто-откатом backend'а. Откат применяется, только если backend
/// не был явно выбран пользователем (`backend_override == None`) — явный выбор
/// (напр. CUDA в Settings) молча не подменяем, только сообщаем об ошибке.
async fn install_with_fallback(
    app: &AppHandle,
    state: &RuntimeInstallState,
    backend_override: Option<RuntimeBackend>,
) -> Result<RuntimeStatus, DlErr> {
    let Some(explicit) = backend_override else {
        let chain = fallback_chain(recommend_backend());
        let mut last_err: Option<String> = None;
        for (i, backend) in chain.iter().enumerate() {
            if state.cancel.load(Ordering::SeqCst) {
                return Err(DlErr::Canceled);
            }
            if i > 0 {
                emit(
                    app,
                    "Пробую другой backend…",
                    backend.label_ru(),
                    0,
                    0,
                    false,
                    None,
                    false,
                );
            }
            match install_impl(app, state, Some(backend.clone())).await {
                Ok(st) => return Ok(st),
                Err(DlErr::Canceled) => return Err(DlErr::Canceled),
                Err(DlErr::Failed(msg)) => last_err = Some(msg),
            }
        }
        return Err(DlErr::Failed(
            last_err.unwrap_or_else(|| "Не удалось установить движок.".into()),
        ));
    };
    install_impl(app, state, Some(explicit)).await
}

async fn install_impl(
    app: &AppHandle,
    state: &RuntimeInstallState,
    backend_override: Option<RuntimeBackend>,
) -> Result<RuntimeStatus, DlErr> {
    let backend = backend_override.unwrap_or_else(recommend_backend);

    emit(
        app,
        "Готовлю установку",
        &format!("{} · {}", PINNED_TAG, backend.label_ru()),
        0,
        0,
        false,
        None,
        false,
    );

    // Создаём portable-папки.
    let models = default_models_dir().map_err(DlErr::Failed)?;
    ensure_dir(&models).map_err(DlErr::Failed)?;
    let cache = runtime_root().map_err(DlErr::Failed)?.join(".cache");
    ensure_dir(&cache).map_err(DlErr::Failed)?;

    let release = fetch_pinned_release().await.map_err(DlErr::Failed)?;
    let tag = release.tag_name.clone();
    let (main_name, cudart_name) = asset_names(&tag, &backend);
    let main_asset = find_asset(&release, &main_name).map_err(DlErr::Failed)?;
    let cudart_asset = match &cudart_name {
        Some(n) => Some(find_asset(&release, n).map_err(DlErr::Failed)?),
        None => None,
    };

    // Оценка места: zip'ы * 2 (распаковка + staging) + запас.
    let need = main_asset.size + cudart_asset.map(|a| a.size).unwrap_or(0);
    let need = need.saturating_mul(3);
    if let Some(free) = free_space_bytes(&runtime_root().map_err(DlErr::Failed)?) {
        if free < need {
            return Err(DlErr::Failed(format!(
                "Недостаточно места на диске: нужно ~{}, свободно ~{}",
                fmt_mb(need),
                fmt_mb(free)
            )));
        }
    }

    // live = рабочая папка; staging = соседняя, live не трогаем до успешной проверки.
    let live = backend_dir(&tag, &backend).map_err(DlErr::Failed)?;
    let staging = live.with_file_name(format!(
        "{}.staging",
        live.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("backend")
    ));
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }

    // Скачать основной zip + SHA-256.
    let main_zip = cache.join(&main_name);
    stream_to_file(
        app,
        state,
        &main_asset.browser_download_url,
        &main_zip,
        &format!("Скачиваю {}…", backend.label_ru()),
        &main_name,
    )
    .await?;
    emit(
        app,
        "Проверяю целостность…",
        &main_name,
        0,
        0,
        false,
        None,
        false,
    );
    let main_zip_c = main_zip.clone();
    let main_name_c = main_name.clone();
    tokio::task::spawn_blocking(move || verify_zip_digest(&main_zip_c, &main_name_c))
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка проверки SHA-256: {e}")))?
        .map_err(DlErr::Failed)?;

    // Скачать cudart + SHA-256.
    let cudart_zip = if let Some(asset) = cudart_asset {
        let p = cache.join(&asset.name);
        let asset_name = asset.name.clone();
        stream_to_file(
            app,
            state,
            &asset.browser_download_url,
            &p,
            "Скачиваю CUDA Runtime…",
            &asset_name,
        )
        .await?;
        emit(
            app,
            "Проверяю целостность…",
            &asset_name,
            0,
            0,
            false,
            None,
            false,
        );
        let p_c = p.clone();
        let n_c = asset_name.clone();
        tokio::task::spawn_blocking(move || verify_zip_digest(&p_c, &n_c))
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка проверки SHA-256: {e}")))?
            .map_err(DlErr::Failed)?;
        Some(p)
    } else {
        None
    };

    if state.cancel.load(Ordering::SeqCst) {
        return Err(DlErr::Canceled);
    }

    emit(
        app,
        "Распаковываю движок…",
        &main_name,
        0,
        0,
        false,
        None,
        false,
    );

    // Распаковка только в staging — live (старая установка) цела.
    let staging_c = staging.clone();
    let main_zip_c = main_zip.clone();
    tokio::task::spawn_blocking(move || extract_zip(&main_zip_c, &staging_c, false))
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка задачи распаковки: {e}")))?
        .map_err(DlErr::Failed)?;

    if let Some(cz) = cudart_zip {
        emit(
            app,
            "Распаковываю CUDA Runtime…",
            cz.file_name().and_then(|n| n.to_str()).unwrap_or("cudart"),
            0,
            0,
            false,
            None,
            false,
        );
        let staging_c = staging.clone();
        tokio::task::spawn_blocking(move || extract_zip(&cz, &staging_c, true))
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка задачи распаковки: {e}")))?
            .map_err(DlErr::Failed)?;
    }

    if !is_installed_at(&staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(DlErr::Failed(
            "После распаковки llama-server.exe не найден. Архив релиза изменился? Обновите GGFlow.".into(),
        ));
    }

    write_meta(&staging, &tag, &backend).map_err(DlErr::Failed)?;

    emit(
        app,
        "Проверяю запуск…",
        SERVER_EXE,
        0,
        0,
        false,
        None,
        false,
    );
    let staging_c = staging.clone();
    tokio::task::spawn_blocking(move || smoke_test_server(&staging_c))
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка smoke-теста: {e}")))?
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            DlErr::Failed(e)
        })?;

    emit(app, "Активирую установку…", "", 0, 0, false, None, false);
    let staging_c = staging.clone();
    let live_c = live.clone();
    tokio::task::spawn_blocking(move || swap_staging_into_live(&staging_c, &live_c))
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка активации: {e}")))?
        .map_err(DlErr::Failed)?;

    // Чистим zip-кэш.
    let _ = std::fs::remove_file(&main_zip);
    if let Some(n) = cudart_name {
        let _ = std::fs::remove_file(cache.join(n));
    }
    // На всякий случай убрать брошенный staging.
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }

    emit(
        app,
        "Готово",
        &format!("{} · {}", tag, backend.label_ru()),
        1,
        1,
        true,
        None,
        false,
    );

    build_status().map_err(DlErr::Failed)
}

fn fmt_mb(bytes: u64) -> String {
    format!("{:.0} МБ", bytes as f64 / (1024.0 * 1024.0))
}

/// Свободное место на томе, где лежит path (Windows: GetDiskFreeSpaceEx через std нестандартно —
/// используем простой fallback: None если не смогли).
/// Свободное место на томе path (для проверок перед скачиванием/установкой).
pub(crate) fn free_space_bytes(path: &Path) -> Option<u64> {
    // На Windows берём корень диска path.
    #[cfg(windows)]
    {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        // Пробуем через `fs2`-подобный хак: GetDiskFreeSpaceExW через windows crate
        // не подключали — используем PowerShell-free подход через kernel32.
        // Минимально: если path не существует, create parent.
        let probe = if path.exists() {
            path.to_path_buf()
        } else {
            path.parent()?.to_path_buf()
        };
        let wide: Vec<u16> = OsStr::new(&probe)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free_bytes: u64 = 0;
        let mut total: u64 = 0;
        let mut total_free: u64 = 0;
        // windows crate feature Win32_Storage_FileSystem — может не быть. Используем raw.
        #[link(name = "kernel32")]
        extern "system" {
            fn GetDiskFreeSpaceExW(
                lpDirectoryName: *const u16,
                lpFreeBytesAvailableToCaller: *mut u64,
                lpTotalNumberOfBytes: *mut u64,
                lpTotalNumberOfFreeBytes: *mut u64,
            ) -> i32;
        }
        let ok = unsafe {
            GetDiskFreeSpaceExW(wide.as_ptr(), &mut free_bytes, &mut total, &mut total_free)
        };
        if ok != 0 {
            return Some(free_bytes);
        }
        None
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

// ── Tauri-команды ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn runtime_status() -> Result<RuntimeStatus, String> {
    build_status()
}

/// Установить (или переустановить) managed runtime.
/// `backend`: None / "auto" → по железу; иначе "cpu" | "vulkan" | "cuda-12.4".
#[tauri::command]
pub async fn runtime_install(
    app: AppHandle,
    state: State<'_, RuntimeInstallState>,
    backend: Option<String>,
) -> Result<RuntimeStatus, String> {
    {
        let mut active = state.lock_active();
        if *active {
            return Err("Установка движка уже идёт.".into());
        }
        *active = true;
    }
    state.cancel.store(false, Ordering::SeqCst);

    let backend = match backend
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "auto")
    {
        Some(id) => {
            Some(RuntimeBackend::from_id(id).ok_or_else(|| format!("Неизвестный backend: {id}"))?)
        }
        None => None,
    };

    let result = install_with_fallback(&app, &state, backend).await;
    *state.lock_active() = false;

    match result {
        Ok(st) => Ok(st),
        Err(DlErr::Canceled) => {
            emit(&app, "Отменено", "", 0, 0, false, None, true);
            Err("Установка отменена.".into())
        }
        Err(DlErr::Failed(msg)) => {
            emit(&app, "Ошибка", "", 0, 0, false, Some(msg.clone()), false);
            Err(msg)
        }
    }
}

#[tauri::command]
pub fn runtime_cancel_install(state: State<RuntimeInstallState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// Гарантировать, что `{app_dir}/models` существует; вернуть путь.
#[tauri::command]
pub fn ensure_default_models_dir() -> Result<String, String> {
    let dir = default_models_dir()?;
    ensure_dir(&dir)?;
    Ok(dir.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn pinned_digests_cover_all_backends() {
        let tag = PINNED_TAG;
        for backend in [
            RuntimeBackend::Cpu,
            RuntimeBackend::Vulkan,
            RuntimeBackend::Cuda12,
        ] {
            let (main, cudart) = asset_names(tag, &backend);
            assert!(
                pinned_digest_for(&main).is_some(),
                "missing digest for {main}"
            );
            if let Some(c) = cudart {
                assert!(pinned_digest_for(&c).is_some(), "missing digest for {c}");
            }
        }
    }

    #[test]
    fn verify_zip_digest_rejects_mismatch() {
        let dir = std::env::temp_dir().join(format!("ll-sha-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("llama-b9963-bin-win-cpu-x64.zip");
        let mut f = File::create(&path).unwrap();
        f.write_all(b"not-a-real-zip").unwrap();
        drop(f);
        let err = verify_zip_digest(&path, "llama-b9963-bin-win-cpu-x64.zip");
        assert!(err.is_err());
        // Файл с неверным hash должен быть удалён.
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn swap_staging_preserves_old_on_empty_staging_fail() {
        // swap requires staging to be a dir — empty path fails clearly.
        let base = std::env::temp_dir().join(format!("ll-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let live = base.join("cpu");
        let staging = base.join("cpu.staging");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("marker.txt"), b"old").unwrap();
        // staging missing → error, live intact
        assert!(swap_staging_into_live(&staging, &live).is_err());
        assert!(live.join("marker.txt").is_file());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn swap_staging_replaces_live() {
        let base = std::env::temp_dir().join(format!("ll-swap2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let live = base.join("cpu");
        let staging = base.join("cpu.staging");
        std::fs::create_dir_all(&live).unwrap();
        std::fs::write(live.join("marker.txt"), b"old").unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("marker.txt"), b"new").unwrap();
        swap_staging_into_live(&staging, &live).unwrap();
        let content = std::fs::read_to_string(live.join("marker.txt")).unwrap();
        assert_eq!(content, "new");
        assert!(!staging.exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    // ── Реальный pipeline на pinned-архивах llama.cpp ───────────────────────
    //
    // Не гоняются по умолчанию (сеть, десятки-сотни МБ на кейс): `cargo test -- --ignored`.
    // Проверяют честный путь release metadata → download → sha256 → extract → smoke test,
    // на настоящих файлах, а не моках. CUDA-кейс подтверждает, что архив скачивается,
    // хэш совпадает, `llama-server.exe` находится и отвечает на `--version` — но НЕ
    // подтверждает загрузку CUDA-ядер (для этого нужен реальный NVIDIA GPU, которого
    // на CI-раннере нет).

    async fn download_to(url: &str, dest: &Path) {
        let resp = client()
            .expect("http client")
            .get(url)
            .send()
            .await
            .expect("download request failed");
        let bytes = resp.bytes().await.expect("download body failed");
        std::fs::write(dest, &bytes).expect("write downloaded zip");
    }

    fn run_real_pipeline(backend: RuntimeBackend) {
        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            let tag = PINNED_TAG;
            let release = fetch_pinned_release()
                .await
                .expect("fetch pinned release metadata");
            let (main_name, cudart_name) = asset_names(tag, &backend);

            let dir = std::env::temp_dir().join(format!(
                "ll-pipeline-test-{}-{}",
                backend.id(),
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();

            let main_asset =
                find_asset(&release, &main_name).expect("main asset present in pinned release");
            let main_zip = dir.join(&main_name);
            download_to(&main_asset.browser_download_url, &main_zip).await;
            verify_zip_digest(&main_zip, &main_name)
                .expect("main zip sha256 matches pinned digest");

            let dest = dir.join("install");
            extract_zip(&main_zip, &dest, false).expect("extract main zip");

            if let Some(cudart_name) = cudart_name {
                let cudart_asset = find_asset(&release, &cudart_name)
                    .expect("cudart asset present in pinned release");
                let cudart_zip = dir.join(&cudart_name);
                download_to(&cudart_asset.browser_download_url, &cudart_zip).await;
                verify_zip_digest(&cudart_zip, &cudart_name)
                    .expect("cudart zip sha256 matches pinned digest");
                extract_zip(&cudart_zip, &dest, true).expect("merge cudart into install dir");
            }

            assert!(
                is_installed_at(&dest),
                "llama-server.exe not found after extraction"
            );
            smoke_test_server(&dest).expect("llama-server.exe --version smoke test failed");

            let _ = std::fs::remove_dir_all(&dir);
        });
    }

    #[test]
    #[ignore = "network: downloads real pinned CPU archive"]
    fn real_pipeline_cpu() {
        run_real_pipeline(RuntimeBackend::Cpu);
    }

    #[test]
    #[ignore = "network: downloads real pinned Vulkan archive"]
    fn real_pipeline_vulkan() {
        run_real_pipeline(RuntimeBackend::Vulkan);
    }

    #[test]
    #[ignore = "network: downloads real pinned CUDA + cudart archives (~300MB+)"]
    fn real_pipeline_cuda() {
        run_real_pipeline(RuntimeBackend::Cuda12);
    }
}
