# llama-launcher

A desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp) — browse and download models from Hugging Face, auto-configure launch parameters for your hardware, and run `llama-server` with a single click.

Built with [Tauri](https://tauri.app/), [SvelteKit](https://svelte.dev/) and TypeScript.

## Features

- **Model catalog** — browse and download GGUF models straight from Hugging Face.
- **Local models** — manage the models already on disk.
- **Auto-config** — detects your hardware (GPU/CPU/RAM) and picks sensible launch parameters.
- **One-click run** — start/stop the llama.cpp server and watch its status live.
- **Onboarding & settings** — first-run setup and configurable paths.

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
