// Тонкие обёртки над Tauri invoke() + типы, зеркалящие Rust-структуры.
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";

// ── Типы (зеркало config.rs / models.rs) ─────────────────────────────────────

export interface LaunchDefaults {
  ctx: number;
  kv_quant: string; // "f16" | "q8_0" | "q4_0"
  threads: number;
  ngl: number;
  port: number;
  tools: boolean;
}

export interface Settings {
  llama_dir: string | null;
  model_folders: string[];
  defaults: LaunchDefaults;
  onboarded: boolean;
  /**
   * Версия завершённого wizard'а. 0 / отсутствует = показать setup снова
   * (даже если onboarded=true от старой версии приложения).
   */
  setup_version: number;
  /** Runtime поставлен лаунчером (portable). */
  runtime_managed: boolean;
  runtime_tag: string | null;
  /** "cpu" | "vulkan" | "cuda-12.4" */
  runtime_backend: string | null;
  /** "ru" | "en" */
  locale: string;
  /** "beginner" | "intermediate" | "expert" */
  expertise: string;
  /** Открывать Web-UI при server-ready. */
  open_ui_on_ready: boolean;
}

/** Актуальная версия wizard'а — должна совпадать с config::SETUP_VERSION в Rust. */
export const CURRENT_SETUP_VERSION = 1;

/** Нужен ли экран первоначальной настройки. */
export function needsSetup(s: Settings): boolean {
  return !s.onboarded || (s.setup_version ?? 0) < CURRENT_SETUP_VERSION;
}

export interface ModelInfo {
  path: string;
  name: string;
  size: number;
}

export interface GgufMeta {
  architecture: string | null;
  n_layers: number | null;
  n_head_kv: number | null;
  n_head: number | null;
  n_embd: number | null;
  ctx_train: number | null;
}

export interface LaunchConfig {
  llama_dir: string;
  model_path: string;
  ctx: number;
  kv_quant: string;
  threads: number;
  ngl: number;
  port: number;
  tools: boolean;
}

export interface ServerStatus {
  running: boolean;
  /** Backend: модель загружена (log listening /health), не только процесс жив. */
  ready: boolean;
  port: number | null;
  model_name: string | null;
}

// ── Железо и авто-настройка (Фаза 3, зеркало hardware.rs / autoconfig.rs) ─────

export interface GpuInfo {
  name: string;
  vram_bytes: number;
}

export interface HardwareInfo {
  gpu: GpuInfo | null;
  total_ram_bytes: number;
  logical_cores: number;
  physical_cores: number;
}

export interface AutoConfig {
  ngl: number;
  ctx: number;
  kv_quant: string;
  threads: number;
  est_vram_bytes: number;
  full_offload: boolean;
  rationale: string;
}

// ── Каталог Hugging Face (Фаза 4, зеркало hf.rs) ──────────────────────────────

export interface HfModel {
  id: string;
  downloads: number;
  likes: number;
  last_modified: string | null;
}

export interface HfFile {
  path: string;
  size: number;
}

/** Событие `download-progress` из бэкенда. */
export interface DownloadProgress {
  file: string;
  downloaded: number;
  total: number; // 0 = размер неизвестен
  done: boolean;
  error: string | null;
  canceled: boolean;
}

// ── Команды ──────────────────────────────────────────────────────────────────

export const loadSettings = (): Promise<Settings> => invoke("load_settings");

export const saveSettings = (settings: Settings): Promise<void> =>
  invoke("save_settings", { settings });

export const validateLlamaDir = (dir: string): Promise<boolean> =>
  invoke("validate_llama_dir", { dir });

export const scanModels = (folders: string[]): Promise<ModelInfo[]> =>
  invoke("scan_models", { folders });

export const readGgufMeta = (path: string): Promise<GgufMeta> =>
  invoke("read_gguf_meta", { path });

/** Открыть Проводник и выделить файл. */
export const revealInFolder = (path: string): Promise<void> =>
  invoke("reveal_in_folder", { path });

export const startServer = (config: LaunchConfig): Promise<ServerStatus> =>
  invoke("start_server", { config });

export const stopServer = (): Promise<void> => invoke("stop_server");

export const serverStatus = (): Promise<ServerStatus> =>
  invoke("server_status");

/** Сведения о железе (VRAM / RAM / ядра). */
export const detectHardware = (): Promise<HardwareInfo> =>
  invoke("detect_hardware");

/** Рекомендованные параметры запуска модели под текущее железо. */
export const autoConfig = (modelPath: string): Promise<AutoConfig> =>
  invoke("auto_config", { modelPath });

// ── Hugging Face ──────────────────────────────────────────────────────────────

/** Поиск GGUF-репозиториев по подстроке (сортировка по загрузкам). limit по умолчанию 40. */
export const hfSearch = (query: string, limit?: number): Promise<HfModel[]> =>
  invoke("hf_search", { query, limit });

/** Список .gguf-файлов репозитория с размерами. */
export const hfListFiles = (repo: string): Promise<HfFile[]> =>
  invoke("hf_list_files", { repo });

/** Скачать файл репо в папку назначения. Прогресс — через событие `download-progress`. */
export const hfDownload = (
  repo: string,
  file: string,
  destDir: string,
  /** Ожидаемый размер (байт) — проверка свободного места на диске. */
  expectedSize?: number | null,
): Promise<string> =>
  invoke("hf_download", {
    repo,
    file,
    destDir,
    expectedSize: expectedSize ?? null,
  });

/** Отменить текущую загрузку. */
export const hfCancelDownload = (): Promise<void> =>
  invoke("hf_cancel_download");

// ── Чат (прокси к llama-server /v1/chat/completions) ─────────────────────────

export interface ChatMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

/** Событие `chat-delta` со стримингом. */
export interface ChatDelta {
  delta: string;
  done: boolean;
  error: string | null;
}

/** Стрим ответа; токены приходят событием `chat-delta`. */
export const chatStream = (
  port: number,
  messages: ChatMessage[],
): Promise<void> => invoke("chat_stream", { port, messages });

export const chatCancel = (): Promise<void> => invoke("chat_cancel");

// ── Managed runtime (llama.cpp) ───────────────────────────────────────────────

export interface RuntimeStatus {
  llama_dir: string | null;
  installed: boolean;
  tag: string | null;
  backend: string | null;
  backend_label: string | null;
  recommended_backend: string;
  recommended_label: string;
  app_dir: string;
  default_models_dir: string;
  runtime_root: string;
}

/** Событие `runtime-progress` при установке движка. */
export interface RuntimeProgress {
  stage: string;
  file: string;
  downloaded: number;
  total: number;
  done: boolean;
  error: string | null;
  canceled: boolean;
}

export const runtimeStatus = (): Promise<RuntimeStatus> =>
  invoke("runtime_status");

/** backend: null/"auto" | "cpu" | "vulkan" | "cuda-12.4" */
export const runtimeInstall = (backend?: string | null): Promise<RuntimeStatus> =>
  invoke("runtime_install", { backend: backend ?? null });

export const runtimeCancelInstall = (): Promise<void> =>
  invoke("runtime_cancel_install");

export const ensureDefaultModelsDir = (): Promise<string> =>
  invoke("ensure_default_models_dir");

/** Открыть URL во внешнем браузере. */
export const openExternal = (url: string): Promise<void> => openUrl(url);

// ── Диалог выбора папки ──────────────────────────────────────────────────────

/** Открыть системный диалог выбора папки. null если отменили. */
export async function pickFolder(title?: string): Promise<string | null> {
  const result = await open({ directory: true, multiple: false, title });
  return typeof result === "string" ? result : null;
}

// ── Утилиты форматирования ───────────────────────────────────────────────────

export function formatBytes(bytes: number): string {
  if (bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}
