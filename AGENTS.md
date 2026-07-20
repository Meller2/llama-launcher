# AGENTS.md — llama-launcher

Desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp) (v0.3.6). Wraps `llama-server`: scans local GGUF models, downloads from Hugging Face, installs managed runtimes (CPU / Vulkan / CUDA 12.4), auto-configures launch flags for detected hardware, manages the server process lifecycle, and ships signed app updates.

**Stack:** Tauri v2 (Rust) + SvelteKit (Svelte 5, TypeScript).  
**Windows-first.** DXGI + `GlobalMemoryStatusEx` for hardware; `taskkill` / `CREATE_NO_WINDOW` for process control. Code comments are largely in Russian; UI is i18n (`ru` / `en`) with expertise levels (`beginner` / `intermediate` / `expert`).

**Plugins:** `dialog`, `opener`, `process`, `updater` (signed NSIS + auto-check on boot). Custom frameless window (`decorations: false`) with `Titlebar.svelte`.

## Commands

```bash
npm install
npm run tauri dev      # full app (Tauri window + Vite on :1420)
npm run tauri build    # production NSIS bundle (+ updater artifacts when signed)
npm run check          # svelte-kit sync + svelte-check
npm run dev            # frontend only — invoke() will fail without Tauri
```

Rust (from `src-tauri/`): `cargo build`, `cargo clippy`, `cargo test`, `cargo fmt --check`.  
CI: `.github/workflows/ci.yml` (Windows: check/build/clippy/test/tauri; weekly/manual runtime integration against real pinned zips). Release: `.github/workflows/release.yml` on `v*.*.*` tags.

## Architecture

Three layers, kept in sync **by hand**:

1. **Rust backend** (`src-tauri/src/`) — domain modules registered as Tauri commands in `lib.rs` `invoke_handler`. **Any new command must be added there.**

   | Module | Role |
   |--------|------|
   | `config` | Settings load/save in `app_dir()` (portable-first; atomic tmp→bak→replace), legacy migrate, `setup_version` |
   | `models` | GGUF folder scan (depth≤8, max 5000, no symlinks) + metadata parse + `reveal_in_folder` |
   | `server` | `llama-server` lifecycle (start/stop/status, logs, readiness) |
   | `hardware` | VRAM/RAM/CPU detect (Windows DXGI; non-Windows nvidia-smi / meminfo) |
   | `autoconfig` | Launch flags from hardware + GGUF meta (VRAM/RAM-aware) |
   | `hf` | Hugging Face search + resumable download |
   | `runtime` | Managed llama.cpp install (pinned GitHub release → portable `runtime/<tag>/<backend>/`) |
   | `diagnostics` | Snapshot for bug reports (`diagnostic_report`) |
   | `data_reset` | In-app wipe: runtime / default models / cache / settings (`wipe_app_data`) |

2. **API layer** (`src/lib/api.ts`) — `invoke()` wrappers + TypeScript interfaces that **mirror Rust structs** (`Settings`, `LaunchConfig`, `ModelInfo`, `GgufMeta`, `ServerStatus`, `RuntimeStatus`, `DiagnosticReport`, …). Changing a `#[derive(Serialize/Deserialize)]` struct in Rust **requires** updating the matching interface here or the boundary breaks silently. Also wraps plugin APIs: folder picker, external URLs, signed app update / relaunch.

3. **UI** (`src/routes/+page.svelte` + `src/lib/components/`) — single SPA page, tabs: Модели / Каталог / Запущено / Настройки. SvelteKit `adapter-static` SPA mode (`fallback: index.html`); no SSR.

Other frontend modules:

- `src/lib/server.svelte.ts` — Svelte 5 runes store for server lifecycle events
- `src/lib/prefs.svelte.ts` — locale / expertise / open-UI prefs applied from Settings (gates advanced UI by expertise)
- `src/lib/i18n.ts` — `ru` / `en` string dictionaries (`prefs.t(key)`)
- `src/lib/recommended.ts` — curated model list for beginners (categories + VRAM fit hints); refresh periodically
- Components: `LocalModels`, `Catalog`, `Running`, `Settings`, `Onboarding`, `Titlebar`, `ContextMenu`, `Icon`

### Server lifecycle (event-driven)

Start/stop are commands; status returns via Tauri **events**, not command return values. `serverState` listens for:

| Event | Meaning |
|-------|---------|
| `server-log` | stdout/stderr line (buffer capped at 2000) |
| `server-ready` | model loaded / bound; payload = port |
| `server-timeout` | not ready within `READY_TIMEOUT` (180s); process may still be alive |
| `server-exit` | process ended; payload = exit code |

Other progress events (UI listens ad hoc): `download-progress`, `runtime-progress`, `models-changed` (after successful HF download).

Readiness: log watcher matches `"listening"` + port, plus watchdog that polls HTTP `GET /health` (not bare TCP — avoids false ready on a foreign process). `mark_ready` (one-shot `AtomicBool`) ensures `server-ready` fires at most once. Watcher/watchdog threads use a generation `id` so stale threads cannot clobber a newer launch.

- Non-zero exit after **manual** stop (`taskkill`) is expected — **not** an error; only self-crashes are.
- `server::shutdown` runs on window `CloseRequested` so the child is not orphaned.
- All `ServerState` / `DownloadState` / `RuntimeInstallState` mutex access uses poison-tolerant `.lock()` (`unwrap_or_else(|e| e.into_inner())`).

### Launch flags

Defaults: `config::LaunchDefaults` (16k ctx, q4_0 KV, ngl 99, port 8080, tools off).  
Mapping: `server::build_args` (`LaunchConfig` → CLI args). Always passes host `127.0.0.1`, flash-attn on, jinja, batch sizes; optional `--tools all --ui-mcp-proxy`.  
Overrides: `autoconfig` from hardware + GGUF meta (VRAM reserve ~1.5 GiB, RAM reserve ~3 GiB, ctx floor 2k–4k).

When adding a launch parameter, update all three: `LaunchDefaults` / `LaunchConfig` (Rust) → `api.ts` → `build_args`.

### Managed runtime (`runtime.rs`)

Downloads a **pinned** llama.cpp release (`PINNED_TAG` + `PINNED_DIGESTS` SHA-256 table — not `/releases/latest`). Currently `b9963` (Windows x64 zips: cpu, vulkan, cuda-12.4, **plus** `cudart` zip for CUDA). Flow: download → verify hash → extract to `*.staging` (CUDA merges cudart DLLs) → smoke-test `llama-server --version` → atomic swap into live (old → `*.bak` → delete). Failed extract/smoke must not wipe a working install.

```
{app_dir}/runtime/<tag>/<backend>/llama-server.exe
{app_dir}/models/   # default GGUF folder
```

`app_dir` = folder next to the executable if writable; else `%LOCALAPPDATA%\com.llamalauncher.app\` (product id, no personal names). **True portable:** `settings.json`, `runtime/`, `models/` all under `app_dir`. Legacy Roaming/LocalAppData paths (`com.llamalauncher.app`, `com.ilzat.llama-launcher`) are scanned for runtime and one-shot settings migration. Backend pick: NVIDIA → CUDA 12.4, other GPU → Vulkan, else CPU. Commands: `runtime_status`, `runtime_check_update`, `runtime_install`, `runtime_cancel_install`, `ensure_default_models_dir`, `wipe_app_data`. When bumping the pin, update both `PINNED_TAG` and digests for **all four** Windows zip assets.

### Downloads (`hf.rs`)

Stream to keyed `{basename}.{hash12}.part` (hash of repo+path — no cross-repo collisions), rename on success. Cancel/fail keeps `.part`; resume via HTTP `Range` (206 append + Content-Range start check; 200 = restart). Free-space check when `expected_size` known. Emits `models-changed` on success so LocalModels auto-refreshes. Connect timeout 20s; no total body timeout (large GGUF). Single-slot mutex.

### Settings (portable-first)

Path: `{app_dir}/settings.json` via `runtime::app_dir()` — **same root as runtime/models**, not Tauri Roaming `app_config_dir`.

`Settings` and `LaunchDefaults` use `#[serde(default)]` at struct level so older config files deserialize field-by-field (missing fields get defaults) instead of wiping the whole file. Keep this when adding fields. Save is Windows-safe: write `.json.tmp` → move old → `.json.bak` → rename tmp → target (POSIX `rename` overwrite does not apply on Windows).

On first load, if canonical file is missing, copy from the first found legacy path (Roaming/LocalAppData current + old identifier). Wipe removes all known settings locations so migrate cannot resurrect them.

`setup_version` / `CURRENT_SETUP_VERSION` (api.ts) gate the onboarding wizard; bump both when the wizard shape changes. Fields: `locale`, `expertise`, `open_ui_on_ready`, managed-runtime tag/backend.

### Hardware detection

`hardware.rs`: `#[cfg(windows)]` DXGI + `GlobalMemoryStatusEx`; `#[cfg(not(windows))]` nvidia-smi / `/proc/meminfo` or `sysctl`. Non-NVIDIA GPUs off Windows fall back toward CPU mode.

### App updates & GitHub Releases

Tauri updater plugin: pubkey + endpoint `…/releases/latest/download/latest.json` in `tauri.conf.json`. Boot path silently checks/installs (`checkAppUpdate` → `installAppUpdate` → relaunch). Release workflow signs with `TAURI_SIGNING_*` secrets. CI builds use `tauri.ci.conf.json` (no signing key on PRs).

Tag `v*.*.*` → `.github/workflows/release.yml` creates a **draft pre-release** with:

| Asset | Purpose |
|-------|---------|
| `llama-launcher_<ver>_x64-setup.exe` (+ `.sig`) | NSIS installer (signed for updater) |
| `latest.json` | Updater manifest (not for humans) |
| `llama-launcher-v<ver>-portable.exe` / `.zip` | Portable, no installer |

Notes: default template `.github/release-notes.md` (`{{VERSION}}` / `{{VERSION_NUM}}`); optional per-tag override `.github/releases/vX.Y.Z.md`. After the draft is created, open it on GitHub, edit highlights if needed, then publish.

### Diagnostics

`diagnostics::diagnostic_report` aggregates app version, OS/arch, GPU/RAM, runtime paths, free disk, server state — for Settings “copy report” / bug reports (`formatDiagnosticReport` on the frontend).

## Conventions

- Prefer small, focused diffs; match existing style (Russian comments in Rust/TS are fine).
- New Tauri command → register in `lib.rs` → add TS wrapper + types in `api.ts` → wire UI.
- Svelte 5 runes (`$state`, `$derived`, `.svelte.ts` stores) — not legacy stores for new code.
- UI strings go through `i18n.ts` / `prefs.t()`, not hard-coded (except rare technical labels). Respect expertise gates in `prefs.svelte.ts` when adding advanced controls.
- Unit tests live next to modules (`#[cfg(test)]`); keep them for pure logic. Do not add a large frontend test suite or drive-by refactors unless asked.
- Do not commit secrets, large GGUF binaries, or `src-tauri/target/` artifacts.
- Bumping app version: `package.json` + `src-tauri/Cargo.toml` + `tauri.conf.json` (keep in sync).

## Key paths

```
src-tauri/src/lib.rs          # command registration, window close → shutdown
src-tauri/src/server.rs       # process spawn, readiness, events
src-tauri/src/runtime.rs      # managed llama.cpp install (PINNED_TAG / digests)
src-tauri/src/config.rs       # Settings, LaunchDefaults, SETUP_VERSION
src-tauri/src/diagnostics.rs  # diagnostic_report
src-tauri/src/data_reset.rs   # wipe_app_data (settings / runtime / models / cache)
src/lib/api.ts                # IPC boundary (keep in sync with Rust)
src/lib/server.svelte.ts      # frontend server state
src/lib/recommended.ts        # curated catalog recommendations
src/routes/+page.svelte       # shell + tabs + onboarding + auto-update
src/lib/components/           # feature UI
.github/workflows/            # ci.yml, release.yml
```
