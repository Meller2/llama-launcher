//! Детект железа для авто-настройки: VRAM (DXGI), RAM (GlobalMemoryStatusEx),
//! число ядер CPU. Всё нативно через `windows`-крейт — без битого wmic.

use serde::Serialize;

// ── Типы, уходящие во фронтенд ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Default)]
pub struct GpuInfo {
    /// Имя адаптера, напр. "AMD Radeon RX 5700".
    pub name: String,
    /// Выделенная видеопамять в байтах (DedicatedVideoMemory).
    pub vram_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct HardwareInfo {
    /// Дискретная GPU с наибольшей VRAM (None, если не нашли).
    pub gpu: Option<GpuInfo>,
    /// Всего физической RAM, байты.
    pub total_ram_bytes: u64,
    /// Логические процессоры (потоки).
    pub logical_cores: u32,
    /// Оценка физических ядер (logical/2 при SMT).
    pub physical_cores: u32,
}

// ── DXGI: перечисление видеоадаптеров ────────────────────────────────────────

#[cfg(windows)]
fn detect_gpu() -> Option<GpuInfo> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};

    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1::<IDXGIFactory1>().ok()?;
        let mut best: Option<GpuInfo> = None;
        let mut i = 0u32;
        loop {
            // EnumAdapters возвращает ошибку (DXGI_ERROR_NOT_FOUND) за границей списка.
            let adapter = match factory.EnumAdapters(i) {
                Ok(a) => a,
                Err(_) => break,
            };
            i += 1;

            let desc = match adapter.GetDesc() {
                Ok(d) => d,
                Err(_) => continue,
            };

            // Description — UTF-16, дополнен нулями.
            let raw = &desc.Description;
            let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
            let name = String::from_utf16_lossy(&raw[..end]);

            // Софт-адаптер (Microsoft Basic Render Driver, VRAM 0) нам не нужен.
            if name.contains("Basic Render") {
                continue;
            }

            let cand = GpuInfo {
                name,
                vram_bytes: desc.DedicatedVideoMemory as u64,
            };
            // Берём адаптер с наибольшей VRAM — это дискретная карта.
            best = match best {
                Some(b) if b.vram_bytes >= cand.vram_bytes => Some(b),
                _ => Some(cand),
            };
        }
        best
    }
}

// ── RAM ──────────────────────────────────────────────────────────────────────

#[cfg(windows)]
fn detect_ram() -> u64 {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut ms = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe {
        if GlobalMemoryStatusEx(&mut ms).is_ok() {
            ms.ullTotalPhys
        } else {
            0
        }
    }
}

// ── Не-Windows: фолбэки без нативных API ─────────────────────────────────────

/// GPU через `nvidia-smi` (если установлен): суммарная видеопамять первой карты.
/// AMD/Intel/Apple на этих платформах пока не покрываем — вернём None (→ CPU-режим).
#[cfg(not(windows))]
fn detect_gpu() -> Option<GpuInfo> {
    use std::process::Command;
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // Первая строка: "NVIDIA GeForce RTX 4090, 24564" (имя, МиБ).
    let line = String::from_utf8_lossy(&out.stdout);
    let first = line.lines().next()?;
    let (name, mib) = first.split_once(',')?;
    let mib: u64 = mib.trim().parse().ok()?;
    Some(GpuInfo {
        name: name.trim().to_string(),
        vram_bytes: mib * 1024 * 1024,
    })
}

/// RAM: Linux — /proc/meminfo (MemTotal, кБ); macOS — sysctl hw.memsize (байты).
#[cfg(not(windows))]
fn detect_ram() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(text) = std::fs::read_to_string("/proc/meminfo") {
            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(kb) = rest.split_whitespace().next() {
                        if let Ok(kb) = kb.parse::<u64>() {
                            return kb * 1024;
                        }
                    }
                }
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        use std::process::Command;
        Command::new("sysctl")
            .args(["-n", "hw.memsize"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0)
    }
}

// ── CPU ──────────────────────────────────────────────────────────────────────

/// (логические, оценка физических). Физические оцениваем как logical/2 (SMT).
fn detect_cores() -> (u32, u32) {
    let logical = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);
    let physical = (logical / 2).max(1);
    (logical, physical)
}

// ── Tauri-команда ─────────────────────────────────────────────────────────────

/// Собрать сведения о железе. Вызывается фронтом и авто-конфигом.
#[tauri::command]
pub fn detect_hardware() -> HardwareInfo {
    let (logical_cores, physical_cores) = detect_cores();
    HardwareInfo {
        gpu: detect_gpu(),
        total_ram_bytes: detect_ram(),
        logical_cores,
        physical_cores,
    }
}
