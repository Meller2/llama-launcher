## LlamaLauncher {{VERSION}} (pre-release)

Desktop launcher for [llama.cpp](https://github.com/ggml-org/llama.cpp) — download GGUF models, auto-install the engine, and run `llama-server` with one click.

### Download

| File | Use when |
|------|----------|
| **`llama-launcher_{{VERSION_NUM}}_x64-setup.exe`** | Normal install (**recommended**) |
| **`llama-launcher-v{{VERSION_NUM}}-portable.exe`** | No installer — drop and run |
| **`llama-launcher-v{{VERSION_NUM}}-portable.zip`** | Same portable binary, zipped |

> `*.sig` and `latest.json` are for **in-app auto-update**. You do not need to download them manually.

### Highlights

- Managed llama.cpp runtime (CUDA / Vulkan / CPU), pinned + checksummed
- Hugging Face catalog with resumable downloads
- Hardware auto-config (VRAM / RAM aware)
- Onboarding, RU/EN UI, expertise levels
- Signed automatic app updates

### Requirements

- Windows 10/11 x64

### Changelog

See commits since the previous tag on the Releases page, or:

```
https://github.com/Meller2/llama-launcher/compare/vPREV...v{{VERSION_NUM}}
```

<!-- Maintainers: replace the Highlights section with real notes for this tag
     before publishing the draft, or pass a custom body via workflow input. -->
