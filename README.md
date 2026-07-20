# GGFlow

**[English](#english)** · **[Русский](#русский)**

[![CI](https://github.com/Meller2/llama-launcher/actions/workflows/ci.yml/badge.svg)](https://github.com/Meller2/llama-launcher/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Meller2/llama-launcher?include_prereleases)](https://github.com/Meller2/llama-launcher/releases)

Desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp).  
Formerly **llama-launcher**.

---

<a id="english"></a>

## English

[↑ language](#ggflow) · [Русский ↓](#русский)

Download GGUF models from Hugging Face, auto-install the engine for your GPU, tune launch flags for your hardware, and run `llama-server` with one click.

Built with [Tauri](https://tauri.app/), [SvelteKit](https://svelte.dev/) and TypeScript. **Windows-first** (x64). Powered by **llama.cpp**.

### Download

Pre-release builds: **[Releases](https://github.com/Meller2/llama-launcher/releases)**

| Asset | Use when |
|-------|----------|
| `ggflow_*_x64-setup.exe` | Normal install (**recommended**) |
| `ggflow-v*-portable.exe` / `.zip` | No installer — drop into a folder and run |

> `*.sig` and `latest.json` are for **in-app auto-update**. You do not need them manually.

### Features

- **Managed runtime** — official llama.cpp builds (CUDA / Vulkan / CPU), pinned + checksummed
- **True portable mode** — if the app folder is writable, **settings, engine, and models** all live next to the exe
- **Model catalog** — curated recommendations + Hugging Face search / resumable GGUF downloads
- **Local models** — scan folders, reveal in Explorer, one-click launch
- **Auto-config** — VRAM / RAM / CPU aware launch parameters
- **Server lifecycle** — start / stop / logs / ready status; optional open chat UI when ready
- **Onboarding** — language (RU/EN), expertise level, first-run engine install
- **Data reset** — Settings → remove runtime / default models / cache / settings without uninstalling
- **Signed app updates** — check and install from inside the app

### Portable layout

When the folder next to the executable is writable (portable / USB / dev):

```
ggflow.exe
settings.json
runtime/<tag>/<backend>/llama-server.exe
models/
```

Copy the whole folder to another PC — prefs come with you.

If that folder is not writable (e.g. Program Files), data goes to:

```
%LOCALAPPDATA%\com.ggflow.app\
  settings.json
  runtime\...
  models\
```

### Development

Prerequisites: [Node.js](https://nodejs.org/), [Rust](https://www.rust-lang.org/tools/install), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev      # full app (Tauri + Vite on :1420)
npm run check          # svelte-check
npm run tauri build    # NSIS + updater artifacts (when signing keys present)
```

Rust only (`src-tauri/`): `cargo build`, `cargo clippy`, `cargo test`.

### License

[MIT](LICENSE)

---

<a id="русский"></a>

## Русский

[↑ к выбору языка](#ggflow) · [English ↑](#english)

Десктопный лаунчер для [llama.cpp](https://github.com/ggml-org/llama.cpp).  
Раньше назывался **llama-launcher**.

Скачивание моделей с Hugging Face, автоустановка движка под видеокарту, подбор параметров под железо и запуск `llama-server` в один клик.

Стек: [Tauri](https://tauri.app/), [SvelteKit](https://svelte.dev/), TypeScript. **Windows-first** (x64). На базе **llama.cpp**.

### Скачать

Pre-release: **[Releases](https://github.com/Meller2/llama-launcher/releases)**

| Файл | Когда |
|------|--------|
| `ggflow_*_x64-setup.exe` | Обычная установка (**рекомендуется**) |
| `ggflow-v*-portable.exe` / `.zip` | Без установщика — положил в папку и запустил |

> `*.sig` и `latest.json` — только для **автообновления**. Вручную не нужны.

### Возможности

- **Managed runtime** — официальные сборки llama.cpp (CUDA / Vulkan / CPU), pin + SHA-256
- **Настоящий portable** — настройки, движок и модели рядом с exe (если папка writable)
- **Каталог** — рекомендации + поиск HF + докачка GGUF
- **Локальные модели** — скан папок, Проводник, запуск одной кнопкой
- **Автонастройка** — VRAM / RAM / CPU
- **Сервер** — старт / стоп / лог / ready; опционально открыть чат
- **Онбординг** — RU/EN, уровень, установка движка
- **Сброс данных** — в Настройках, без удаления программы
- **Подписанные обновления** — из интерфейса

### Раскладка portable

```
ggflow.exe
settings.json
runtime/<tag>/<backend>/llama-server.exe
models/
```

Иначе (Program Files):

```
%LOCALAPPDATA%\com.ggflow.app\
```

### Разработка

```bash
npm install
npm run tauri dev
npm run check
npm run tauri build
```

### Лицензия

[MIT](LICENSE)
