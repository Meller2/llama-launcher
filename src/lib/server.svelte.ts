// Реактивный стор жизненного цикла сервера (Svelte 5 runes в .svelte.ts).
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  startServer,
  stopServer,
  serverStatus,
  openExternal,
  type LaunchConfig,
} from "$lib/api";
import { prefs } from "$lib/prefs.svelte";

const MAX_LINES = 2000;

class ServerStore {
  running = $state(false);
  ready = $state(false);
  starting = $state(false);
  stopping = $state(false);
  port = $state<number | null>(null);
  modelName = $state<string | null>(null);
  error = $state<string | null>(null);
  log = $state<string[]>([]);

  #unlisten: UnlistenFn[] = [];
  #initialized = false;
  /** Уже авто-открывали UI для текущего ready-цикла. */
  #autoOpened = false;

  /** Подписаться на события бэкенда. Вызывать один раз при старте UI. */
  async init() {
    if (this.#initialized) return;
    this.#initialized = true;

    this.#unlisten.push(
      await listen<string>("server-log", (e) => {
        this.log.push(e.payload);
        if (this.log.length > MAX_LINES) {
          this.log = this.log.slice(this.log.length - MAX_LINES);
        }
      }),
    );
    this.#unlisten.push(
      await listen<number>("server-ready", (e) => {
        this.ready = true;
        this.running = true;
        this.port = e.payload;
        // Авто-открытие чата — один раз за цикл готовности.
        if (prefs.openUiOnReady && !this.#autoOpened) {
          this.#autoOpened = true;
          openExternal(`http://127.0.0.1:${e.payload}`).catch(() => {});
        }
      }),
    );
    this.#unlisten.push(
      await listen<string>("server-timeout", (e) => {
        // Сервер не поднялся за отведённое время — показываем ошибку, но процесс
        // мог остаться живым (висит на загрузке модели), пусть пользователь решает.
        if (!this.ready) this.error = e.payload;
      }),
    );
    this.#unlisten.push(
      await listen<number>("server-exit", (e) => {
        const wasManualStop = this.stopping;
        this.running = false;
        this.ready = false;
        this.stopping = false;
        // Не оставляем устаревшие port/model после выхода процесса.
        this.port = null;
        this.modelName = null;
        // Ручной стоп через taskkill даёт ненулевой код — это норма, не ошибка.
        // Ошибку показываем только при самопадении (краш, не загрузилась модель).
        if (!wasManualStop && e.payload !== 0) {
          this.error = `Сервер неожиданно завершился (код ${e.payload}). Смотрите лог.`;
        }
      }),
    );

    // Синхронизировать начальный статус (на случай hot-reload).
    // ready приходит с backend — не застреваем в «Загрузка модели», если сервер уже up.
    try {
      const st = await serverStatus();
      this.running = st.running;
      this.ready = st.ready ?? false;
      this.port = st.port;
      this.modelName = st.model_name;
      if (this.ready && this.port && prefs.openUiOnReady) {
        // Не авто-открываем UI при hot-reload — только синхронизируем флаги.
        this.#autoOpened = true;
      }
    } catch {
      /* игнор */
    }
  }

  async start(config: LaunchConfig) {
    this.error = null;
    this.starting = true;
    this.ready = false;
    this.#autoOpened = false;
    this.log = [];
    try {
      const st = await startServer(config);
      this.running = st.running;
      this.ready = st.ready ?? false;
      this.port = st.port;
      this.modelName = st.model_name;
    } catch (e) {
      this.error = String(e);
      this.running = false;
      this.ready = false;
    } finally {
      this.starting = false;
    }
  }

  async stop() {
    this.stopping = true;
    try {
      await stopServer();
    } catch (e) {
      this.error = String(e);
    }
    // running/ready сбросит событие server-exit; подстрахуемся.
    this.ready = false;
  }

  openWebUi() {
    if (this.port) openExternal(`http://127.0.0.1:${this.port}`);
  }

  clearError() {
    this.error = null;
  }
}

export const serverState = new ServerStore();
