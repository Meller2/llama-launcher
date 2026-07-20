# llama-launcher

A desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp) — browse and download models from Hugging Face, auto-configure launch parameters for your hardware, and run `llama-server` with a single click.

Built with [Tauri](https://tauri.app/), [SvelteKit](https://svelte.dev/) and TypeScript.

## Features

- **Auto-install runtime** — downloads official `llama.cpp` builds (CUDA / Vulkan / CPU) next to the app (portable).
- **Model catalog** — browse and download GGUF models straight from Hugging Face.
- **Local models** — manage the models already on disk.
- **Auto-config** — detects your hardware (GPU/CPU/RAM) and picks sensible launch parameters.
- **One-click run** — start/stop the llama.cpp server and watch its status live.
- **Onboarding & settings** — first-run setup and configurable paths.

### Portable layout

When the folder next to the executable is writable (portable / USB / dev),
**everything** lives there — settings, engine, models. Copy the folder and go.

If that folder is not writable (e.g. Program Files install), data goes to
`%LOCALAPPDATA%\com.llamalauncher.app\`:

```
settings.json                                # UI prefs, onboarding, paths
runtime/<tag>/<backend>/llama-server.exe     # managed engine
models/                                      # default GGUF folder
```

## Development

Prerequisites: [Node.js](https://nodejs.org/), [Rust](https://www.rust-lang.org/tools/install), and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
npm install
npm run tauri dev
```

## Build

```bash
npm run tauri build
```

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
