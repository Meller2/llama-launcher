//! Каталог Hugging Face (Фаза 4): поиск GGUF-репозиториев, список файлов, скачивание.
//! HTTP делаем на Rust (reqwest): стриминг прямо в файл + события прогресса без фронт-плагинов.
//!
//! Эндпоинты HF:
//!   поиск   GET /api/models?search=&filter=gguf&sort=downloads&direction=-1&limit=40
//!   файлы   GET /api/models/{repo}/tree/main?recursive=true
//!   файл    GET /{repo}/resolve/main/{path}   (LFS-редирект reqwest проходит сам)
//!
//! `.part` именуется с учётом repo+path (короткий hash), чтобы два репо с одним
//! basename GGUF не склеивали докачку.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

const HF: &str = "https://huggingface.co";
const UA: &str = concat!("LlamaLauncher/", env!("CARGO_PKG_VERSION"));
/// Не чаще одного события прогресса на ~2 МБ — чтобы не заваливать фронт.
const EMIT_STEP: u64 = 2_000_000;
/// Запас места на диске сверх размера файла (ФС / meta).
const DISK_MARGIN: u64 = 64 * 1024 * 1024;

// ── Модели данных ─────────────────────────────────────────────────────────────

/// Репозиторий в результатах поиска (для UI).
#[derive(Debug, Clone, Serialize)]
pub struct HfModel {
    pub id: String,
    pub downloads: u64,
    pub likes: u64,
    pub last_modified: Option<String>,
}

/// Сырой ответ HF search (поля в camelCase).
#[derive(Debug, Deserialize)]
struct HfModelRaw {
    id: String,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    likes: u64,
    #[serde(rename = "lastModified", default)]
    last_modified: Option<String>,
}

/// GGUF-файл в репозитории (для UI).
#[derive(Debug, Clone, Serialize)]
pub struct HfFile {
    pub path: String,
    pub size: u64,
}

/// Элемент дерева репо. У LFS-файлов реальный размер лежит в `lfs.size`,
/// а `size` — это размер указателя (мелкий), поэтому предпочитаем lfs.size.
#[derive(Debug, Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<Lfs>,
}

#[derive(Debug, Deserialize)]
struct Lfs {
    #[serde(default)]
    size: u64,
}

/// Событие `download-progress` для фронта.
#[derive(Debug, Clone, Serialize)]
struct Progress {
    file: String,
    downloaded: u64,
    total: u64, // 0 = размер неизвестен
    done: bool,
    error: Option<String>,
    canceled: bool,
}

/// Состояние текущей загрузки — для отмены и защиты от параллельных скачиваний.
#[derive(Default)]
pub struct DownloadState {
    cancel: AtomicBool,
    /// Имя файла активной загрузки (None = свободно).
    active: Mutex<Option<String>>,
}

impl DownloadState {
    /// Poison-устойчивый lock (паника чужого потока не должна класть загрузки).
    fn active(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Внутренняя ошибка скачивания: отмена vs реальный сбой.
enum DlErr {
    Canceled,
    Failed(String),
}

// ── Вспомогательное ───────────────────────────────────────────────────────────

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(UA)
        // Только connect: полный timeout на request убил бы многогигабайтные GGUF.
        .connect_timeout(Duration::from_secs(20))
        .build()
        .map_err(|e| format!("Не удалось создать HTTP-клиент: {e}"))
}

/// Короткий hex SHA-256 (12 символов) от repo+path — ключ partial-файла.
fn part_key_hex(repo: &str, file: &str) -> String {
    let mut h = Sha256::new();
    h.update(repo.as_bytes());
    h.update([0u8]);
    h.update(file.as_bytes());
    let dig = h.finalize();
    dig.iter().take(6).map(|b| format!("{b:02x}")).collect()
}

/// `{basename}.{hash12}.part` — уникально на (repo, path), итог rename → basename.
fn part_path_for(dir: &Path, repo: &str, file: &str, basename: &str) -> PathBuf {
    let key = part_key_hex(repo, file);
    dir.join(format!("{basename}.{key}.part"))
}

/// Разбор `Content-Range: bytes START-END/TOTAL` → START.
fn content_range_start(header: &str) -> Option<u64> {
    // "bytes 1000-1999/5000" или "bytes 1000-1999/*"
    let s = header.trim();
    let rest = s.strip_prefix("bytes ")?.trim_start();
    let start = rest.split('-').next()?.trim();
    start.parse().ok()
}

/// Минимальное percent-кодирование строки поиска (RFC 3986 unreserved остаётся как есть).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

// ── Tauri-команды ─────────────────────────────────────────────────────────────

/// Поиск GGUF-репозиториев по подстроке, отсортированных по числу загрузок.
/// `limit` (None → 40) даёт фронту «показать ещё»: перезапрос с бо́льшим лимитом.
#[tauri::command]
pub async fn hf_search(query: String, limit: Option<u32>) -> Result<Vec<HfModel>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    // HF отдаёт максимум ~100 за запрос; ограничим разумным диапазоном.
    let limit = limit.unwrap_or(40).clamp(1, 100);
    let url = format!(
        "{HF}/api/models?search={}&filter=gguf&sort=downloads&direction=-1&limit={limit}",
        urlencode(q)
    );
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Сеть недоступна: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Hugging Face вернул {}", resp.status()));
    }
    let raw: Vec<HfModelRaw> = resp
        .json()
        .await
        .map_err(|e| format!("Не удалось разобрать ответ поиска: {e}"))?;
    Ok(raw
        .into_iter()
        .map(|m| HfModel {
            id: m.id,
            downloads: m.downloads,
            likes: m.likes,
            last_modified: m.last_modified,
        })
        .collect())
}

/// Список .gguf-файлов репозитория с размерами (учитывая LFS).
#[tauri::command]
pub async fn hf_list_files(repo: String) -> Result<Vec<HfFile>, String> {
    let url = format!("{HF}/api/models/{repo}/tree/main?recursive=true");
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Сеть недоступна: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Hugging Face вернул {} для {repo}", resp.status()));
    }
    let entries: Vec<TreeEntry> = resp
        .json()
        .await
        .map_err(|e| format!("Не удалось разобрать список файлов: {e}"))?;

    let mut files: Vec<HfFile> = entries
        .into_iter()
        .filter(|e| e.kind == "file" && e.path.to_lowercase().ends_with(".gguf"))
        .map(|e| {
            let size = e
                .lfs
                .as_ref()
                .map(|l| l.size)
                .filter(|&s| s > 0)
                .unwrap_or(e.size);
            HfFile { path: e.path, size }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Скачать файл репо в папку назначения. Стримит в keyed `.part`, затем rename.
/// `expected_size` — размер с UI (для проверки места); None = только после ответа сервера.
/// Шлёт `download-progress` и при успехе `models-changed`.
#[tauri::command]
pub async fn hf_download(
    app: AppHandle,
    state: State<'_, DownloadState>,
    repo: String,
    file: String,
    dest_dir: String,
    expected_size: Option<u64>,
) -> Result<String, String> {
    // В репо путь может быть с подпапками — на диск кладём по базовому имени.
    let filename = Path::new(&file)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Некорректное имя файла".to_string())?
        .to_string();

    let dir = PathBuf::from(&dest_dir);
    if !dir.is_dir() {
        return Err(format!("Папка назначения не найдена: {dest_dir}"));
    }
    let final_path = dir.join(&filename);
    if final_path.exists() {
        return Err(format!("Файл «{filename}» уже есть в папке."));
    }
    // Ключ зависит от repo+path — два репо с одним basename не делят .part.
    let part_path = part_path_for(&dir, &repo, &file, &filename);

    // Докачка: если keyed .part уже есть, продолжим с его размера (HTTP Range).
    let resume_from = tokio::fs::metadata(&part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    // Место на диске (остаток = expected − already + margin).
    if let Some(total) = expected_size {
        let need = total
            .saturating_sub(resume_from)
            .saturating_add(DISK_MARGIN);
        if let Some(free) = crate::runtime::free_space_bytes(&dir) {
            if free < need {
                return Err(format!(
                    "Недостаточно места на диске: нужно ещё ~{:.0} МБ, свободно ~{:.0} МБ.",
                    need as f64 / (1024.0 * 1024.0),
                    free as f64 / (1024.0 * 1024.0)
                ));
            }
        }
    }

    // Занять единственный слот загрузки.
    {
        let mut active = state.active();
        if active.is_some() {
            return Err("Уже идёт другая загрузка — дождитесь её завершения.".into());
        }
        *active = Some(filename.clone());
    }
    state.cancel.store(false, Ordering::SeqCst);

    let encoded_file: String = file.split('/').map(urlencode).collect::<Vec<_>>().join("/");
    let url = format!(
        "{HF}/{}/resolve/main/{encoded_file}",
        urlencode_path_repo(&repo)
    );

    let result = stream_to_file(&app, &state, &url, &part_path, &filename, resume_from).await;

    // Освободить слот в любом исходе.
    *state.active() = None;

    match result {
        Ok(total) => {
            // Windows: rename поверх не работает — final уже проверен на отсутствие.
            std::fs::rename(&part_path, &final_path)
                .map_err(|e| format!("Не удалось сохранить файл: {e}"))?;
            emit(&app, &filename, total, total, true, None, false);
            // Список локальных моделей может обновиться без ручного refresh.
            let _ = app.emit("models-changed", final_path.to_string_lossy().to_string());
            Ok(final_path.to_string_lossy().to_string())
        }
        Err(DlErr::Canceled) => {
            emit(&app, &filename, 0, 0, false, None, true);
            Err("Загрузка отменена.".into())
        }
        Err(DlErr::Failed(msg)) => {
            emit(&app, &filename, 0, 0, false, Some(msg.clone()), false);
            Err(msg)
        }
    }
}

/// Кодирует owner/name репо (слэш оставляем).
fn urlencode_path_repo(repo: &str) -> String {
    repo.split('/').map(urlencode).collect::<Vec<_>>().join("/")
}

/// Отменить текущую загрузку (флаг подхватится в цикле стриминга).
#[tauri::command]
pub fn hf_cancel_download(state: State<DownloadState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

// ── Реализация стриминга ──────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn emit(
    app: &AppHandle,
    file: &str,
    downloaded: u64,
    total: u64,
    done: bool,
    error: Option<String>,
    canceled: bool,
) {
    let _ = app.emit(
        "download-progress",
        Progress {
            file: file.to_string(),
            downloaded,
            total,
            done,
            error,
            canceled,
        },
    );
}

/// Качает `url` в `part_path` кусками, эмитит прогресс, проверяет флаг отмены.
/// При `resume_from > 0` продолжает существующий `.part` через HTTP Range.
/// Возвращает итоговый размер при успехе.
async fn stream_to_file(
    app: &AppHandle,
    state: &State<'_, DownloadState>,
    url: &str,
    part_path: &Path,
    filename: &str,
    resume_from: u64,
) -> Result<u64, DlErr> {
    let mut req = client().map_err(DlErr::Failed)?.get(url);
    if resume_from > 0 {
        // Просим сервер отдать хвост файла начиная с уже скачанного смещения.
        req = req.header("Range", format!("bytes={resume_from}-"));
    }
    let mut resp = req
        .send()
        .await
        .map_err(|e| DlErr::Failed(format!("Сеть недоступна: {e}")))?;

    // 206 Partial Content → докачка принята. 200 → сервер отдаёт файл целиком
    // (Range проигнорирован), поэтому начинаем с нуля и перезаписываем .part.
    let status = resp.status();
    let resuming = status == reqwest::StatusCode::PARTIAL_CONTENT && resume_from > 0;
    if !status.is_success() {
        return Err(DlErr::Failed(format!(
            "Hugging Face вернул {status} при скачивании"
        )));
    }

    // Content-Range должен начинаться с resume_from — иначе склейка мусора.
    if resuming {
        if let Some(cr) = resp.headers().get(reqwest::header::CONTENT_RANGE) {
            if let Ok(s) = cr.to_str() {
                if let Some(start) = content_range_start(s) {
                    if start != resume_from {
                        return Err(DlErr::Failed(format!(
                            "Сервер отдал Range с позиции {start}, ожидали {resume_from}. Удалите .part и скачайте заново."
                        )));
                    }
                }
            }
        }
    }

    let already: u64 = if resuming { resume_from } else { 0 };
    // content_length — это длина ТЕЛА ответа; при докачке прибавляем уже скачанное.
    let body_len = resp.content_length().unwrap_or(0);
    let total = if body_len > 0 { already + body_len } else { 0 };

    // Открываем .part на дозапись (докачка) или создаём заново (с нуля).
    let mut out = if resuming {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part_path)
            .await
    } else {
        File::create(part_path).await
    }
    .map_err(|e| DlErr::Failed(format!("Не удалось открыть файл: {e}")))?;

    let mut downloaded = already;
    let mut last_emit = already;
    // Начальное событие, чтобы UI сразу показал полосу (и точку старта при докачке).
    emit(app, filename, downloaded, total, false, None, false);

    loop {
        if state.cancel.load(Ordering::SeqCst) {
            let _ = out.flush().await;
            return Err(DlErr::Canceled);
        }
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка при скачивании: {e}")))?;
        let chunk = match chunk {
            Some(c) => c,
            None => break, // конец потока
        };
        out.write_all(&chunk)
            .await
            .map_err(|e| DlErr::Failed(format!("Ошибка записи на диск: {e}")))?;
        downloaded += chunk.len() as u64;

        if downloaded - last_emit >= EMIT_STEP {
            last_emit = downloaded;
            emit(app, filename, downloaded, total, false, None, false);
        }
    }

    out.flush()
        .await
        .map_err(|e| DlErr::Failed(format!("Ошибка сброса на диск: {e}")))?;

    // Проверка целостности: если сервер сообщил размер — он должен совпасть.
    // Иначе оборванный поток с кодом 200 молча сохранился бы как «валидный» файл.
    if total > 0 && downloaded != total {
        return Err(DlErr::Failed(format!(
            "Файл скачан не полностью: {downloaded} из {total} байт. Попробуйте докачать."
        )));
    }

    Ok(if total > 0 { total } else { downloaded })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn part_key_differs_by_repo() {
        let a = part_key_hex("org/model-a", "q4.gguf");
        let b = part_key_hex("org/model-b", "q4.gguf");
        assert_ne!(a, b);
        assert_eq!(a.len(), 12);
        assert_eq!(
            part_key_hex("org/model-a", "q4.gguf"),
            a,
            "stable for same inputs"
        );
    }

    #[test]
    fn content_range_start_parses() {
        assert_eq!(content_range_start("bytes 1000-1999/5000"), Some(1000));
        assert_eq!(content_range_start("bytes 0-99/*"), Some(0));
        assert_eq!(content_range_start("invalid"), None);
    }

    #[test]
    fn part_path_includes_hash_and_basename() {
        let p = part_path_for(
            Path::new("C:\\models"),
            "owner/repo",
            "sub/model.gguf",
            "model.gguf",
        );
        let name = p.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("model.gguf."));
        assert!(name.ends_with(".part"));
        assert!(name.contains(&part_key_hex("owner/repo", "sub/model.gguf")));
    }
}
