//! Скан локальных .gguf моделей + минимальный парсер GGUF-заголовка.
//!
//! GGUF-метаданные (n_layers, n_head_kv, n_head, n_embd, ctx_train, arch) нужны
//! для точного расчёта размера KV-кэша в авто-настройке (Фаза 3). Здесь мы читаем
//! только заголовок: массивы (словарь токенизатора и т.п.) пропускаем через seek,
//! не загружая в память.

use serde::Serialize;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

// ── Типы, уходящие во фронтенд ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    /// Полный путь к .gguf.
    pub path: String,
    /// Имя файла (например, "Qwen3-8B-Q4_K_M.gguf").
    pub name: String,
    /// Размер файла в байтах.
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GgufMeta {
    pub architecture: Option<String>,
    /// Кол-во слоёв ({arch}.block_count).
    pub n_layers: Option<u32>,
    /// KV-heads ({arch}.attention.head_count_kv) — с учётом GQA.
    pub n_head_kv: Option<u32>,
    /// Attention heads ({arch}.attention.head_count).
    pub n_head: Option<u32>,
    /// Размерность эмбеддинга ({arch}.embedding_length).
    pub n_embd: Option<u32>,
    /// Тренировочный контекст ({arch}.context_length).
    pub ctx_train: Option<u32>,
}

// ── GGUF value types ─────────────────────────────────────────────────────────

const GGUF_MAGIC: u32 = 0x4655_4747; // "GGUF" little-endian

const T_UINT8: u32 = 0;
const T_INT8: u32 = 1;
const T_UINT16: u32 = 2;
const T_INT16: u32 = 3;
const T_UINT32: u32 = 4;
const T_INT32: u32 = 5;
const T_FLOAT32: u32 = 6;
const T_BOOL: u32 = 7;
const T_STRING: u32 = 8;
const T_ARRAY: u32 = 9;
const T_UINT64: u32 = 10;
const T_INT64: u32 = 11;
const T_FLOAT64: u32 = 12;

/// Размер скалярного типа в байтах (для пропуска массивов). None для string/array.
fn scalar_size(t: u32) -> Option<u64> {
    match t {
        T_UINT8 | T_INT8 | T_BOOL => Some(1),
        T_UINT16 | T_INT16 => Some(2),
        T_UINT32 | T_INT32 | T_FLOAT32 => Some(4),
        T_UINT64 | T_INT64 | T_FLOAT64 => Some(8),
        _ => None,
    }
}

// ── Низкоуровневые ридеры (всё little-endian) ────────────────────────────────

fn read_u32<R: Read>(r: &mut R) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64<R: Read>(r: &mut R) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

/// GGUF-строка: u64-длина + UTF-8 байты.
fn read_gguf_string<R: Read>(r: &mut R) -> io::Result<String> {
    let len = read_u64(r)?;
    // Защита от мусора/битого файла: не аллоцируем гигабайты.
    if len > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "GGUF string too long",
        ));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Прочитать скалярное значение как u32 (для числовых метаданных).
/// Возвращает None для типов, которые нам не нужны как число.
fn read_scalar_as_u32<R: Read>(r: &mut R, t: u32) -> io::Result<Option<u32>> {
    let v = match t {
        T_UINT8 | T_INT8 | T_BOOL => {
            let mut b = [0u8; 1];
            r.read_exact(&mut b)?;
            Some(b[0] as u32)
        }
        T_UINT16 | T_INT16 => {
            let mut b = [0u8; 2];
            r.read_exact(&mut b)?;
            Some(u16::from_le_bytes(b) as u32)
        }
        T_UINT32 | T_INT32 | T_FLOAT32 => Some(read_u32(r)?),
        T_UINT64 | T_INT64 | T_FLOAT64 => Some(read_u64(r)? as u32),
        _ => None,
    };
    Ok(v)
}

/// Пропустить одно значение заданного типа (используется для ненужных ключей).
fn skip_value<R: Read + Seek>(r: &mut R, t: u32) -> io::Result<()> {
    if let Some(sz) = scalar_size(t) {
        r.seek(SeekFrom::Current(sz as i64))?;
        return Ok(());
    }
    match t {
        T_STRING => {
            let len = read_u64(r)?;
            r.seek(SeekFrom::Current(len as i64))?;
        }
        T_ARRAY => {
            let elem_type = read_u32(r)?;
            let count = read_u64(r)?;
            if let Some(sz) = scalar_size(elem_type) {
                r.seek(SeekFrom::Current((sz * count) as i64))?;
            } else if elem_type == T_STRING {
                // Массив строк (напр. словарь токенизатора) — пропускаем по одной.
                for _ in 0..count {
                    let len = read_u64(r)?;
                    r.seek(SeekFrom::Current(len as i64))?;
                }
            } else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported nested array type",
                ));
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unknown GGUF value type",
            ));
        }
    }
    Ok(())
}

/// Разобрать GGUF-заголовок и вытащить нужные метаданные.
pub(crate) fn parse_gguf(path: &Path) -> io::Result<GgufMeta> {
    let file = File::open(path)?;
    let mut r = BufReader::new(file);

    let magic = read_u32(&mut r)?;
    if magic != GGUF_MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not a GGUF file",
        ));
    }
    let _version = read_u32(&mut r)?;
    let _tensor_count = read_u64(&mut r)?;
    let kv_count = read_u64(&mut r)?;

    let mut meta = GgufMeta::default();
    // Архитектуру узнаём из general.architecture; ключи вроде "{arch}.block_count"
    // проверяем по суффиксу, т.к. arch может встретиться позже ключа — но на практике
    // general.architecture идёт первым. Для надёжности сверяем по суффиксу.
    for _ in 0..kv_count {
        let key = read_gguf_string(&mut r)?;
        let vtype = read_u32(&mut r)?;

        // Разбираем только интересные ключи, остальное пропускаем.
        if key == "general.architecture" && vtype == T_STRING {
            meta.architecture = Some(read_gguf_string(&mut r)?);
            continue;
        }

        let matched = if key.ends_with(".block_count") {
            Some(&mut meta.n_layers)
        } else if key.ends_with(".attention.head_count_kv") {
            Some(&mut meta.n_head_kv)
        } else if key.ends_with(".attention.head_count") {
            Some(&mut meta.n_head)
        } else if key.ends_with(".embedding_length") {
            Some(&mut meta.n_embd)
        } else if key.ends_with(".context_length") {
            Some(&mut meta.ctx_train)
        } else {
            None
        };

        match matched {
            Some(slot) => {
                if let Some(v) = read_scalar_as_u32(&mut r, vtype)? {
                    *slot = Some(v);
                } else {
                    // Тип оказался не скаляром — корректно пропустим.
                    // (read_scalar_as_u32 не двигал позицию для не-скаляров? Двигал:
                    //  для string/array — нет. Так что здесь только строки/массивы.)
                    skip_value(&mut r, vtype)?;
                }
            }
            None => skip_value(&mut r, vtype)?,
        }
    }

    Ok(meta)
}

// ── Скан папок ───────────────────────────────────────────────────────────────

/// Рекурсивно собрать .gguf из папки (без внешних зависимостей).
fn scan_dir(dir: &Path, out: &mut Vec<ModelInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("gguf"))
            .unwrap_or(false)
        {
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            out.push(ModelInfo {
                path: path.to_string_lossy().into_owned(),
                name,
                size,
            });
        }
    }
}

// ── Tauri-команды ────────────────────────────────────────────────────────────

/// Скан всех указанных папок → список моделей (дедуп по пути, сортировка по имени).
#[tauri::command]
pub fn scan_models(folders: Vec<String>) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    for folder in &folders {
        let p = PathBuf::from(folder);
        if p.is_dir() {
            scan_dir(&p, &mut out);
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Прочитать GGUF-метаданные одной модели. Ошибка → строка для UI.
#[tauri::command]
pub fn read_gguf_meta(path: String) -> Result<GgufMeta, String> {
    parse_gguf(Path::new(&path)).map_err(|e| format!("Не удалось прочитать GGUF: {e}"))
}
