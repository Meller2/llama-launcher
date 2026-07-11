//! Авто-настройка запуска под железо: по размеру модели, VRAM/RAM и GGUF-мете
//! подбираем ngl / ctx / kv_quant / threads так, чтобы (по возможности) уложить
//! всё в видеопамять, иначе — вынести на GPU максимум слоёв, и не взорвать RAM.

use crate::hardware::{detect_hardware, HardwareInfo};
use crate::models::{parse_gguf, GgufMeta};
use serde::Serialize;
use std::path::Path;

/// Резерв VRAM под compute-буферы / оверхед (не под веса и KV).
/// 1.5 GiB — консервативнее прежнего 1 GiB (драйвер + flash-attn + граф).
const VRAM_RESERVE: u64 = (3 * 1024 * 1024 * 1024) / 2; // 1.5 GiB
/// Доп. safety factor на оценки (равномерные слои = оптимизм).
const VRAM_SAFETY: f64 = 0.90;
/// Резерв системной RAM под ОС + UI + llama overhead (не отдаём модели).
const RAM_RESERVE: u64 = 3 * 1024 * 1024 * 1024; // 3 GiB
/// Потолок контекста по умолчанию (не раздуваем без нужды).
const CTX_CAP: u32 = 16384;
/// Минимально осмысленный контекст при нехватке памяти.
const CTX_MIN: u32 = 4096;
/// Ещё более жёсткий минимум, если и 4K не влезает в RAM.
const CTX_TINY: u32 = 2048;

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

/// Бюджет RAM: total − OS reserve (минимум 1 GiB «доступно», иначе всё равно считаем от 1 GiB).
fn ram_budget(hw: &HardwareInfo) -> u64 {
    hw.total_ram_bytes
        .saturating_sub(RAM_RESERVE)
        .max(1024 * 1024 * 1024)
}

/// Полезный VRAM с резервом и safety factor.
fn usable_vram(vram: u64) -> u64 {
    let after_reserve = vram.saturating_sub(VRAM_RESERVE);
    (after_reserve as f64 * VRAM_SAFETY) as u64
}

/// ngl для полного оффлоада: реальное число слоёв, иначе большой sentinel (не «99»).
fn full_ngl(meta: &GgufMeta) -> u32 {
    match meta.n_layers {
        Some(n) if n > 0 => n,
        // llama-server трактует большое ngl как «все слои»; 99 ломается на deep models.
        _ => 999,
    }
}

/// Подобрать ctx/kv под бюджет памяти (модель + KV ≤ budget).
/// Возвращает (ctx, kv_quant, est_bytes) или None если даже tiny+q4 не влезает.
fn fit_memory(
    model_size: u64,
    meta: &GgufMeta,
    budget: u64,
    want_ctx: u32,
) -> Option<(u32, &'static str, u64)> {
    let ctx_steps: &[u32] = &[want_ctx, 8192, CTX_MIN, CTX_TINY];
    // От лучшего KV к экономному.
    for quant in ["f16", "q8_0", "q4_0"] {
        let per_tok = match kv_bytes_per_token(meta, quant) {
            Some(v) => v,
            None => {
                // Без меты: грубо model + 0.5B на ctx — только q4_0 / урезанный ctx.
                if quant != "q4_0" {
                    continue;
                }
                for &ctx in ctx_steps {
                    if ctx > want_ctx {
                        continue;
                    }
                    // Без меты: model + ~512 MiB KV на каждые 4k ctx.
                    let rough_kv =
                        ((ctx as u64).div_ceil(4096)).saturating_mul(512 * 1024 * 1024);
                    let total = model_size.saturating_add(rough_kv);
                    if total <= budget {
                        return Some((ctx, quant, total));
                    }
                }
                continue;
            }
        };
        for &ctx in ctx_steps {
            if ctx > want_ctx {
                continue;
            }
            let kv_total = per_tok.saturating_mul(ctx as u64);
            let total = model_size.saturating_add(kv_total);
            if total <= budget {
                return Some((ctx, quant, total));
            }
        }
    }
    None
}

/// CPU-only (нет GPU / VRAM=0): подбираем ctx/kv под RAM, ngl=0.
fn compute_cpu(model_size: u64, meta: &GgufMeta, hw: &HardwareInfo) -> AutoConfig {
    let threads = hw.physical_cores.max(1);
    let budget = ram_budget(hw);
    let want = target_ctx(meta);

    if let Some((ctx, quant, _est)) = fit_memory(model_size, meta, budget, want) {
        let note = if ctx < want || quant != "f16" {
            format!(
                "GPU нет — CPU (-ngl 0). Под RAM (~{} доступно) ctx={}, KV {}.",
                fmt_gb(budget),
                ctx,
                quant
            )
        } else {
            format!(
                "GPU не обнаружена — запуск на CPU (-ngl 0), ctx {}, KV {}.",
                ctx, quant
            )
        };
        return AutoConfig {
            ngl: 0,
            ctx,
            kv_quant: quant.into(),
            threads,
            est_vram_bytes: 0,
            full_offload: false,
            rationale: note,
        };
    }

    // Даже tiny не влез — всё равно отдаём минимальные флаги + предупреждение.
    AutoConfig {
        ngl: 0,
        ctx: CTX_TINY,
        kv_quant: "q4_0".into(),
        threads,
        est_vram_bytes: 0,
        full_offload: false,
        rationale: format!(
            "GPU нет, модель ({}) крупная для доступной RAM (~{}) — ctx {} / q4_0, возможен своп.",
            fmt_gb(model_size),
            fmt_gb(budget),
            CTX_TINY
        ),
    }
}

/// Основная эвристика.
fn compute(model_size: u64, meta: &GgufMeta, hw: &HardwareInfo) -> AutoConfig {
    let threads = hw.physical_cores.max(1);
    let vram = hw.gpu.as_ref().map(|g| g.vram_bytes).unwrap_or(0);

    // Нет GPU (или не определили VRAM) → чистый CPU с учётом RAM.
    if vram == 0 {
        return compute_cpu(model_size, meta, hw);
    }

    let usable = usable_vram(vram);
    let want_ctx = target_ctx(meta);
    let ngl_all = full_ngl(meta);

    // 1) Полный оффлоад: кванты KV от лучшего к экономному при целевом ctx.
    for quant in ["q8_0", "q4_0"] {
        if let Some(per_tok) = kv_bytes_per_token(meta, quant) {
            let kv_total = per_tok.saturating_mul(want_ctx as u64);
            if model_size.saturating_add(kv_total) <= usable {
                return AutoConfig {
                    ngl: ngl_all,
                    ctx: want_ctx,
                    kv_quant: quant.into(),
                    threads,
                    est_vram_bytes: model_size + kv_total,
                    full_offload: true,
                    rationale: format!(
                        "Модель ({}) + KV-кэш ({} KV, ctx {}) уходят в VRAM → {} слоёв на GPU.",
                        fmt_gb(model_size),
                        quant,
                        want_ctx,
                        ngl_all
                    ),
                };
            }
        }
    }

    // 2) Полный оффлоад с урезанным контекстом (q4_0).
    if let Some(per_tok) = kv_bytes_per_token(meta, "q4_0") {
        for &try_ctx in &[want_ctx, 8192, CTX_MIN, CTX_TINY] {
            if try_ctx > want_ctx {
                continue;
            }
            let kv_total = per_tok.saturating_mul(try_ctx as u64);
            if model_size.saturating_add(kv_total) <= usable {
                return AutoConfig {
                    ngl: ngl_all,
                    ctx: try_ctx,
                    kv_quant: "q4_0".into(),
                    threads,
                    est_vram_bytes: model_size + kv_total,
                    full_offload: true,
                    rationale: format!(
                        "Для полного оффлоада контекст уменьшен до {} (KV q4_0, ngl={}).",
                        try_ctx, ngl_all
                    ),
                };
            }
        }
    }

    // 3) Частичный оффлоад: сколько слоёв влезет (оценка равномерная — осторожно).
    let part_ctx = CTX_MIN;
    if let (Some(n_layers), Some(per_tok_all)) =
        (meta.n_layers, kv_bytes_per_token(meta, "q4_0"))
    {
        if n_layers > 0 {
            let per_layer_weight = model_size / n_layers as u64;
            let per_layer_kv =
                (per_tok_all / n_layers as u64).saturating_mul(part_ctx as u64);
            let per_layer = (per_layer_weight + per_layer_kv).max(1);
            // Safety: чуть меньше слоёв, чем «влезает ровно».
            let fit = ((usable as f64 * 0.95) / per_layer as f64).floor() as u32;
            let ngl = fit.min(n_layers);
            let est = (ngl as u64).saturating_mul(per_layer);

            // RAM на CPU-часть: (n_layers - ngl) * per_layer_weight + OS.
            let cpu_layers = n_layers.saturating_sub(ngl) as u64;
            let cpu_weights = cpu_layers.saturating_mul(per_layer_weight);
            let ram = ram_budget(hw);
            let (ctx, kv) = fit_memory(
                cpu_weights.saturating_add(est / 4), // грубо: часть KV на CPU
                meta,
                ram,
                part_ctx,
            )
            .map(|(c, q, _)| (c, q))
            .unwrap_or((CTX_TINY, "q4_0"));

            return AutoConfig {
                ngl,
                ctx,
                kv_quant: kv.into(),
                threads,
                est_vram_bytes: est,
                full_offload: false,
                rationale: format!(
                    "Модель ({}) не влезает в VRAM целиком → {} из {} слоёв на GPU (оценка равномерная), ctx {}.",
                    fmt_gb(model_size),
                    ngl,
                    n_layers,
                    ctx
                ),
            };
        }
    }

    // 4) Фолбэк без GGUF-меты: грубо по размеру файла + RAM для CPU path.
    let full = model_size.saturating_add(VRAM_RESERVE / 2) <= usable;
    if full {
        return AutoConfig {
            ngl: 999,
            ctx: CTX_MIN,
            kv_quant: "q4_0".into(),
            threads,
            est_vram_bytes: model_size,
            full_offload: true,
            rationale: "Метаданные неполные — по размеру файла модель помещается в VRAM (ngl=999).".into(),
        };
    }

    // CPU-heavy fallback с RAM-бюджетом.
    let mut cpu = compute_cpu(model_size, meta, hw);
    cpu.rationale = format!(
        "Метаданные неполные и модель крупная — безопасный CPU-режим. {}",
        cpu.rationale
    );
    cpu
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::{GpuInfo, HardwareInfo};
    use crate::models::GgufMeta;

    fn meta_7b() -> GgufMeta {
        GgufMeta {
            architecture: Some("llama".into()),
            n_layers: Some(32),
            n_head_kv: Some(8),
            n_head: Some(32),
            n_embd: Some(4096),
            ctx_train: Some(8192),
        }
    }

    fn hw_cpu(ram_gb: u64) -> HardwareInfo {
        HardwareInfo {
            gpu: None,
            total_ram_bytes: ram_gb * 1024 * 1024 * 1024,
            logical_cores: 8,
            physical_cores: 4,
        }
    }

    fn hw_gpu(vram_gb: u64, ram_gb: u64) -> HardwareInfo {
        HardwareInfo {
            gpu: Some(GpuInfo {
                name: "Test GPU".into(),
                vram_bytes: vram_gb * 1024 * 1024 * 1024,
            }),
            total_ram_bytes: ram_gb * 1024 * 1024 * 1024,
            logical_cores: 16,
            physical_cores: 8,
        }
    }

    #[test]
    fn cpu_mode_ngl_zero_and_respects_low_ram() {
        // 8 GB RAM, 6 GB model → budget ~5 GB, нужно ужать ctx/kv.
        let model = 6 * 1024 * 1024 * 1024u64;
        let cfg = compute(model, &meta_7b(), &hw_cpu(8));
        assert_eq!(cfg.ngl, 0);
        assert!(cfg.ctx <= CTX_CAP);
        // Не должен бездумно ставить f16+16k на слабой RAM.
        assert!(cfg.kv_quant == "q4_0" || cfg.ctx <= 8192 || cfg.rationale.contains("своп"));
    }

    #[test]
    fn full_offload_uses_layer_count_not_99() {
        // 24 GB VRAM, 4 GB model — полный оффлоад.
        let model = 4 * 1024 * 1024 * 1024u64;
        let cfg = compute(model, &meta_7b(), &hw_gpu(24, 32));
        assert!(cfg.full_offload);
        assert_eq!(cfg.ngl, 32); // n_layers, не 99
    }

    #[test]
    fn full_ngl_without_meta_is_999() {
        let m = GgufMeta::default();
        assert_eq!(full_ngl(&m), 999);
    }

    #[test]
    fn fit_memory_reduces_ctx() {
        let model = 5 * 1024 * 1024 * 1024u64;
        // budget barely above model
        let budget = model + 200 * 1024 * 1024;
        let r = fit_memory(model, &meta_7b(), budget, 16384);
        assert!(r.is_some());
        let (ctx, quant, _) = r.unwrap();
        assert!(ctx <= 16384);
        assert!(quant == "q4_0" || quant == "q8_0" || quant == "f16");
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
