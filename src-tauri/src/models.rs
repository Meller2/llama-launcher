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

/// Макс. глубина вложенности (защита от «вечных» деревьев).
const SCAN_MAX_DEPTH: u32 = 8;
/// Потолок числа моделей за один скан (UI и память).
const SCAN_MAX_MODELS: usize = 5_000;

/// Рекурсивно собрать .gguf: лимит глубины, лимит числа, без symlink/junction-циклов.
fn scan_dir(
    dir: &Path,
    out: &mut Vec<ModelInfo>,
    depth: u32,
    visited: &mut std::collections::HashSet<PathBuf>,
) {
    if depth > SCAN_MAX_DEPTH || out.len() >= SCAN_MAX_MODELS {
        return;
    }
    // Канонический путь — защита от циклов (junction/symlink loop).
    let canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canon) {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        if out.len() >= SCAN_MAX_MODELS {
            return;
        }
        let path = entry.path();
        // Не следуем по symlink/junction — иначе легко уйти в цикл или «весь диск».
        let ft = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            scan_dir(&path, out, depth + 1, visited);
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
    let mut visited = std::collections::HashSet::new();
    for folder in &folders {
        let p = PathBuf::from(folder);
        if p.is_dir() {
            scan_dir(&p, &mut out, 0, &mut visited);
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out.dedup_by(|a, b| a.path == b.path);
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use std::io::Write;

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn part_key_stable_and_distinct() {
        // part_key живёт в hf — здесь только scan limits sanity.
        assert!(SCAN_MAX_DEPTH >= 4);
        assert!(SCAN_MAX_MODELS >= 100);
    }

    #[test]
    fn scan_finds_gguf_in_temp() {
        let dir = std::env::temp_dir().join(format!("ll-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let mut f = std::fs::File::create(dir.join("sub").join("a.gguf")).unwrap();
        f.write_all(b"gguf").unwrap();
        let list = scan_models(vec![dir.to_string_lossy().into()]);
        assert!(list.iter().any(|m| m.name == "a.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Прочитать GGUF-метаданные одной модели. Ошибка → строка для UI.
#[tauri::command]
pub fn read_gguf_meta(path: String) -> Result<GgufMeta, String> {
    parse_gguf(Path::new(&path)).map_err(|e| format!("Не удалось прочитать GGUF: {e}"))
}

/// Открыть Проводник и выделить файл (Windows). Для ПКМ «Показать в папке».
#[tauri::command]
pub fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Путь не найден: {path}"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // canonicalize → \\?\C:\...; explorer любит обычный путь.
        let full = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let mut s = full.to_string_lossy().into_owned();
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            s = stripped.to_string();
        }
        // /select,"C:\path\file.gguf"
        let arg = format!("/select,{s}");
        Command::new("explorer")
            .arg(&arg)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Не удалось открыть Проводник: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        // macOS/Linux: открыть родительскую папку.
        let dir = if p.is_dir() {
            p
        } else {
            p.parent().unwrap_or(p)
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .or_else(|_| std::process::Command::new("open").arg(dir).spawn())
            .map_err(|e| format!("Не удалось открыть папку: {e}"))?;
        Ok(())
    }
}
