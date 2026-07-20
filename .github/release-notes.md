## GGFlow {{VERSION}} (pre-release)

Desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp) — download GGUF models, auto-install the engine, and run `llama-server` with one click.

Formerly **llama-launcher**.

### Download

| File | Use when |
|------|----------|
| **`ggflow_{{VERSION_NUM}}_x64-setup.exe`** | Normal install (**recommended**) |
| **`ggflow-v{{VERSION_NUM}}-portable.exe`** | No installer — drop and run |
| **`ggflow-v{{VERSION_NUM}}-portable.zip`** | Same portable binary, zipped |

> `*.sig` and `latest.json` are for **in-app auto-update**. You do not need to download them manually.

### Highlights

- Managed llama.cpp runtime (CUDA / Vulkan / CPU), pinned + checksummed
- True portable: settings + engine + models next to the exe when writable
- Hugging Face catalog with resumable downloads
- Hardware auto-config (VRAM / RAM aware)
- Onboarding, RU/EN UI, expertise levels, in-app data reset
- Signed automatic app updates

### Requirements

- Windows 10/11 x64

### Changelog

```
https://github.com/Meller2/llama-launcher/compare/vPREV...v{{VERSION_NUM}}
```
