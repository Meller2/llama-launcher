# AGENTS.md — llama-launcher

Desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp). Wraps `llama-server`: scans local GGUF models, downloads from Hugging Face, installs managed runtimes, auto-configures launch flags for detected hardware, and manages the server process lifecycle.

**Stack:** Tauri v2 (Rust) + SvelteKit (Svelte 5, TypeScript).  
**Windows-first.** DXGI + `GlobalMemoryStatusEx` for hardware; `taskkill` / `CREATE_NO_WINDOW` for process control. Code comments are largely in Russian; UI is i18n (`ru` / `en`).

## Commands

```bash
npm install
npm run tauri dev      # full app (Tauri window + Vite on :1420)
npm run tauri build    # production bundle
npm run check          # svelte-kit sync + svelte-check
npm run dev            # frontend only — invoke() will fail without Tauri
```

Rust (from `src-tauri/`): `cargo build`, `cargo clippy`. **No test suite.**

## Architecture

Three layers, kept in sync **by hand**:

1. **Rust backend** (`src-tauri/src/`) — domain modules registered as Tauri commands in `lib.rs` `invoke_handler`. **Any new command must be added there.**

   | Module | Role |
   |--------|------|
   | `config` | Settings load/save (app config dir), `setup_version`, path validation |
   | `models` | GGUF folder scan + metadata parse |
   | `server` | `llama-server` lifecycle (start/stop/status, logs, readiness) |
   | `hardware` | VRAM/RAM/CPU detect (Windows DXGI; non-Windows nvidia-smi / meminfo) |
   | `autoconfig` | Launch flags from hardware + GGUF meta |
   | `hf` | Hugging Face search + resumable download |
   | `runtime` | Managed llama.cpp install (GitHub releases → portable `runtime/<tag>/<backend>/`) |

2. **API layer** (`src/lib/api.ts`) — `invoke()` wrappers + TypeScript interfaces that **mirror Rust structs** (`Settings`, `LaunchConfig`, `ModelInfo`, `GgufMeta`, `ServerStatus`, `RuntimeStatus`, …). Changing a `#[derive(Serialize/Deserialize)]` struct in Rust **requires** updating the matching interface here or the boundary breaks silently.

3. **UI** (`src/routes/+page.svelte` + `src/lib/components/`) — single SPA page, tabs: Модели / Каталог / Запущено / Настройки. SvelteKit `adapter-static` SPA mode (`fallback: index.html`); no SSR.

Other frontend modules:

- `src/lib/server.svelte.ts` — Svelte 5 runes store for server lifecycle events
- `src/lib/prefs.svelte.ts` — locale / expertise / open-UI prefs applied from Settings
- `src/lib/i18n.ts` — `ru` / `en` string dictionaries (`prefs.t(key)`)
- Components: `LocalModels`, `Catalog`, `Running`, `Settings`, `Onboarding`

### Server lifecycle (event-driven)

Start/stop are commands; status returns via Tauri **events**, not command return values. `serverState` listens for:

| Event | Meaning |
|-------|---------|
| `server-log` | stdout/stderr line (buffer capped at 2000) |
| `server-ready` | bound port; payload = port |
| `server-timeout` | not ready within `READY_TIMEOUT` (180s); process may still be alive |
| `server-exit` | process ended; payload = exit code |

Readiness: log watcher matches `"listening"` + port, plus TCP poll watchdog. `mark_ready` (one-shot `AtomicBool`) ensures `server-ready` fires at most once. Watcher/watchdog threads use a generation `id` so stale threads cannot clobber a newer launch.

- Non-zero exit after **manual** stop (`taskkill`) is expected — **not** an error; only self-crashes are.
- `server::shutdown` runs on window `CloseRequested` so the child is not orphaned.
- All `ServerState` / `DownloadState` / `RuntimeInstallState` mutex access uses poison-tolerant `.lock()` (`unwrap_or_else(|e| e.into_inner())`).

### Launch flags

Defaults: `config::LaunchDefaults` (16k ctx, q4_0 KV, ngl 99, port 8080).  
Mapping: `server::build_args` (`LaunchConfig` → CLI args).  
Overrides: `autoconfig` from hardware + GGUF meta.

When adding a launch parameter, update all three: `LaunchDefaults` / `LaunchConfig` (Rust) → `api.ts` → `build_args`.

### Managed runtime (`runtime.rs`)

Downloads official llama.cpp release zips (CUDA 12.4 / Vulkan / CPU), unpacks under portable layout:

```
{app_dir}/runtime/<tag>/<backend>/llama-server.exe
{app_dir}/models/   # default GGUF folder
```

`app_dir` = folder next to the executable if writable; else `%LOCALAPPDATA%\com.ilzat.llama-launcher\`. Backend pick: NVIDIA → CUDA, other GPU → Vulkan, else CPU. Commands: `runtime_status`, `runtime_install`, `runtime_cancel_install`, `ensure_default_models_dir`.

### Downloads (`hf.rs`)

Stream to `<file>.part`, rename on success. Cancel/fail keeps `.part`; resume via HTTP `Range` (206 append; 200 = restart). After stream, check `downloaded` vs `Content-Length` before rename. Writes use `tokio::fs`. Single-slot mutex — no concurrent downloads.

### Settings forward-compat

`Settings` and `LaunchDefaults` use `#[serde(default)]` at struct level so older config files deserialize field-by-field (missing fields get defaults) instead of wiping the whole file. Keep this when adding fields.

`setup_version` / `CURRENT_SETUP_VERSION` (api.ts) gate the onboarding wizard; bump both when the wizard shape changes.

### Hardware detection

`hardware.rs`: `#[cfg(windows)]` DXGI + `GlobalMemoryStatusEx`; `#[cfg(not(windows))]` nvidia-smi / `/proc/meminfo` or `sysctl`. Non-NVIDIA GPUs off Windows fall back toward CPU mode.

## Conventions

- Prefer small, focused diffs; match existing style (Russian comments in Rust/TS are fine).
- New Tauri command → register in `lib.rs` → add TS wrapper + types in `api.ts` → wire UI.
- Svelte 5 runes (`$state`, `$derived`, `.svelte.ts` stores) — not legacy stores for new code.
- UI strings go through `i18n.ts` / `prefs.t()`, not hard-coded (except rare technical labels).
- Do not add a test suite or refactor for its own sake unless asked.
- Do not commit secrets, large GGUF binaries, or `src-tauri/target/` artifacts.

## Key paths

```
src-tauri/src/lib.rs          # command registration, window close → shutdown
src-tauri/src/server.rs       # process spawn, readiness, events
src-tauri/src/runtime.rs      # managed llama.cpp install
src-tauri/src/config.rs       # Settings, LaunchDefaults, SETUP_VERSION
src/lib/api.ts                # IPC boundary (keep in sync with Rust)
src/lib/server.svelte.ts      # frontend server state
src/routes/+page.svelte       # shell + tabs + onboarding gate
src/lib/components/           # feature UI
```
