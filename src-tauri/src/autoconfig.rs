//! Авто-настройка запуска под железо: по размеру модели, VRAM и GGUF-мете
//! подбираем ngl / ctx / kv_quant / threads так, чтобы (по возможности) уложить
//! всё в видеопамять, иначе — вынести на GPU максимум слоёв.

use crate::hardware::{detect_hardware, HardwareInfo};
use crate::models::{parse_gguf, GgufMeta};
use serde::Serialize;
use std::path::Path;

/// Резерв VRAM под compute-буферы / оверхед (не под веса и KV).
const VRAM_RESERVE: u64 = 1024 * 1024 * 1024; // 1 GiB
/// Потолок контекста по умолчанию (не раздуваем без нужды).
const CTX_CAP: u32 = 16384;
/// Минимально осмысленный контекст при нехватке памяти.
const CTX_MIN: u32 = 4096;

#[derive(Debug, Clone, Serialize)]
pub struct AutoConfig {
    pub ngl: u32,
    pub ctx: u32,
    pub kv_quant: String,
    pub threads: u32,
    /// Оценка занятой VRAM (веса+KV) в байтах.
    pub est_vram_bytes: u64,
    /// Уходит ли модель целиком на GPU.
    pub full_offload: bool,
    /// Человекочитаемое пояснение для UI (RU).
    pub rationale: String,
}

/// Байт на элемент KV для кванта (приблизительно).
fn quant_bytes(q: &str) -> f64 {
    match q {
        "f16" => 2.0,
        "q8_0" => 1.0,
        "q4_0" => 0.5,
        _ => 2.0,
    }
}

/// KV-кэш (K+V, все слои) в байтах на 1 токен: 2 * n_layers * n_head_kv * head_dim * bytes.
/// None, если не хватает метаданных.
fn kv_bytes_per_token(meta: &GgufMeta, quant: &str) -> Option<u64> {
    let n_layers = meta.n_layers? as f64;
    let n_head_kv = meta.n_head_kv? as f64;
    let n_head = meta.n_head? as f64;
    let n_embd = meta.n_embd? as f64;
    if n_head == 0.0 {
        return None;
    }
    let head_dim = n_embd / n_head;
    let bytes = 2.0 * n_layers * n_head_kv * head_dim * quant_bytes(quant);
    Some(bytes as u64)
}

/// Целевой контекст: не больше тренировочного и не больше потолка.
fn target_ctx(meta: &GgufMeta) -> u32 {
    match meta.ctx_train {
        Some(t) if t > 0 => t.min(CTX_CAP),
        _ => CTX_CAP,
    }
}

/// Основная эвристика.
fn compute(model_size: u64, meta: &GgufMeta, hw: &HardwareInfo) -> AutoConfig {
    let threads = hw.physical_cores.max(1);
    let vram = hw.gpu.as_ref().map(|g| g.vram_bytes).unwrap_or(0);

    // Нет GPU (или не определили VRAM) → чистый CPU.
    if vram == 0 {
        return AutoConfig {
            ngl: 0,
            ctx: target_ctx(meta),
            kv_quant: "f16".into(),
            threads,
            est_vram_bytes: 0,
            full_offload: false,
            rationale: "GPU не обнаружена — запуск на CPU (-ngl 0).".into(),
        };
    }

    let usable = vram.saturating_sub(VRAM_RESERVE);
    let ctx = target_ctx(meta);

    // 1) Полный оффлоад: пробуем кванты KV от лучшего к экономному при целевом ctx.
    for quant in ["q8_0", "q4_0"] {
        if let Some(per_tok) = kv_bytes_per_token(meta, quant) {
            let kv_total = per_tok.saturating_mul(ctx as u64);
            if model_size + kv_total <= usable {
                return AutoConfig {
                    ngl: 99,
                    ctx,
                    kv_quant: quant.into(),
                    threads,
                    est_vram_bytes: model_size + kv_total,
                    full_offload: true,
                    rationale: format!(
                        "Модель ({}) + KV-кэш ({} KV, ctx {}) уходят в VRAM целиком → все слои на GPU.",
                        fmt_gb(model_size),
                        quant,
                        ctx
                    ),
                };
            }
        }
    }

    // 2) Полный оффлоад с урезанным контекстом (q4_0).
    if let Some(per_tok) = kv_bytes_per_token(meta, "q4_0") {
        for &try_ctx in &[ctx, 8192, CTX_MIN] {
            if try_ctx > ctx {
                continue;
            }
            let kv_total = per_tok.saturating_mul(try_ctx as u64);
            if model_size + kv_total <= usable {
                return AutoConfig {
                    ngl: 99,
                    ctx: try_ctx,
                    kv_quant: "q4_0".into(),
                    threads,
                    est_vram_bytes: model_size + kv_total,
                    full_offload: true,
                    rationale: format!(
                        "Для полного оффлоада контекст уменьшен до {} (KV q4_0).",
                        try_ctx
                    ),
                };
            }
        }
    }

    // 3) Частичный оффлоад: сколько слоёв влезет (веса + KV на слой), q4_0, ctx=CTX_MIN.
    let part_ctx = CTX_MIN;
    if let (Some(n_layers), Some(per_tok_all)) =
        (meta.n_layers, kv_bytes_per_token(meta, "q4_0"))
    {
        if n_layers > 0 {
            let per_layer_weight = model_size / n_layers as u64;
            let per_layer_kv =
                (per_tok_all / n_layers as u64).saturating_mul(part_ctx as u64);
            let per_layer = (per_layer_weight + per_layer_kv).max(1);
            let fit = (usable / per_layer) as u32;
            let ngl = fit.min(n_layers);
            let est = (ngl as u64) * per_layer;
            return AutoConfig {
                ngl,
                ctx: part_ctx,
                kv_quant: "q4_0".into(),
                threads,
                est_vram_bytes: est,
                full_offload: false,
                rationale: format!(
                    "Модель ({}) не влезает в VRAM целиком → {} из {} слоёв на GPU, остальное на CPU.",
                    fmt_gb(model_size),
                    ngl,
                    n_layers
                ),
            };
        }
    }

    // 4) Фолбэк без GGUF-меты: грубо по размеру файла.
    let full = model_size + VRAM_RESERVE / 2 <= usable;
    AutoConfig {
        ngl: if full { 99 } else { 0 },
        ctx: CTX_MIN,
        kv_quant: "q4_0".into(),
        threads,
        est_vram_bytes: if full { model_size } else { 0 },
        full_offload: full,
        rationale: if full {
            "Метаданные неполные — по размеру файла модель помещается в VRAM.".into()
        } else {
            "Метаданные неполные и модель крупная — безопасный запуск на CPU.".into()
        },
    }
}

fn fmt_gb(bytes: u64) -> String {
    format!("{:.1} ГБ", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

// Разовая проверка железа + авто-конфига на реальной машине (Фаза 3).
// Запуск: cargo test --lib phase3_check -- --nocapture --ignored
#[cfg(test)]
mod phase3 {
    #[test]
    #[ignore]
    fn phase3_check() {
        let hw = crate::hardware::detect_hardware();
        eprintln!("\n=== ЖЕЛЕЗО ===");
        match &hw.gpu {
            Some(g) => eprintln!("GPU: {} · {}", g.name, super::fmt_gb(g.vram_bytes)),
            None => eprintln!("GPU: не обнаружена"),
        }
        eprintln!("RAM: {}", super::fmt_gb(hw.total_ram_bytes));
        eprintln!("CPU: {} физ. / {} лог.", hw.physical_cores, hw.logical_cores);

        let folders = vec!["F:\\programs\\lm studio models".to_string()];
        let models = crate::models::scan_models(folders);
        eprintln!("\n=== МОДЕЛИ ({}) ===", models.len());
        for m in &models {
            let cfg = super::auto_config(m.path.clone());
            match cfg {
                Ok(c) => eprintln!(
                    "{:<48} {:>8}  ->  ngl={} ctx={} kv={} thr={} full={} ~{}\n    {}",
                    truncate(&m.name, 48),
                    format!("{:.1}G", m.size as f64 / 1e9),
                    c.ngl,
                    c.ctx,
                    c.kv_quant,
                    c.threads,
                    c.full_offload,
                    super::fmt_gb(c.est_vram_bytes),
                    c.rationale,
                ),
                Err(e) => eprintln!("{:<48}  ОШИБКА: {}", m.name, e),
            }
        }
        eprintln!();
    }

    fn truncate(s: &str, n: usize) -> String {
        if s.chars().count() <= n {
            s.to_string()
        } else {
            s.chars().take(n - 1).collect::<String>() + "…"
        }
    }
}

// ── Tauri-команда ─────────────────────────────────────────────────────────────

/// Рассчитать рекомендованные параметры запуска для модели под текущее железо.
#[tauri::command]
pub fn auto_config(model_path: String) -> Result<AutoConfig, String> {
    let path = Path::new(&model_path);
    let model_size = std::fs::metadata(path)
        .map_err(|e| format!("Не удалось прочитать размер модели: {e}"))?
        .len();
    // Мета не критична — при ошибке считаем по фолбэку.
    let meta = parse_gguf(path).unwrap_or_default();
    let hw = detect_hardware();
    Ok(compute(model_size, &meta, &hw))
}
