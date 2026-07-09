<script lang="ts">
  import {
    scanModels,
    readGgufMeta,
    detectHardware,
    autoConfig,
    formatBytes,
    type Settings,
    type ModelInfo,
    type GgufMeta,
    type LaunchConfig,
    type HardwareInfo,
    type AutoConfig,
  } from "$lib/api";
  import { serverState } from "$lib/server.svelte";

  let { settings, onlaunch }: {
    settings: Settings;
    onlaunch: () => void;
  } = $props();

  let models = $state<ModelInfo[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let query = $state("");
  let selected = $state<string | null>(null);
  let meta = $state<GgufMeta | null>(null);
  let metaLoading = $state(false);

  let hw = $state<HardwareInfo | null>(null);
  let auto = $state<AutoConfig | null>(null);
  let autoLoading = $state(false);
  let useAuto = $state(true);

  async function loadHardware() {
    try {
      hw = await detectHardware();
    } catch {
      hw = null;
    }
  }
  loadHardware();

  async function refresh() {
    loading = true;
    error = null;
    try {
      models = await scanModels(settings.model_folders);
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function select(m: ModelInfo) {
    selected = m.path;
    meta = null;
    auto = null;
    metaLoading = true;
    autoLoading = true;
    // Мета и авто-конфиг независимы — тянем параллельно.
    readGgufMeta(m.path)
      .then((r) => (meta = r))
      .catch(() => (meta = null))
      .finally(() => (metaLoading = false));
    autoConfig(m.path)
      .then((r) => (auto = r))
      .catch(() => (auto = null))
      .finally(() => (autoLoading = false));
  }

  const filtered = $derived(
    query.trim()
      ? models.filter((m) =>
          m.name.toLowerCase().includes(query.trim().toLowerCase()),
        )
      : models,
  );

  const selectedModel = $derived(
    models.find((m) => m.path === selected) ?? null,
  );

  async function launch(m: ModelInfo) {
    if (!settings.llama_dir) return;
    // Авто-режим (если посчитался) переопределяет ngl/ctx/kv/threads под железо;
    // иначе — ручные дефолты из настроек.
    const cfg: LaunchConfig = {
      llama_dir: settings.llama_dir,
      model_path: m.path,
      ctx: useAuto && auto ? auto.ctx : settings.defaults.ctx,
      kv_quant: useAuto && auto ? auto.kv_quant : settings.defaults.kv_quant,
      threads: useAuto && auto ? auto.threads : settings.defaults.threads,
      ngl: useAuto && auto ? auto.ngl : settings.defaults.ngl,
      port: settings.defaults.port,
      tools: settings.defaults.tools,
    };
    onlaunch(); // переключить на таб «Запущено»
    await serverState.start(cfg);
  }

  $effect(() => {
    // Пересканировать при изменении списка папок.
    settings.model_folders;
    refresh();
  });
</script>

<div class="page">
  <header class="head">
    <div>
      <h2>Локальные модели</h2>
      <p class="sub">
        {#if loading}Сканирую…{:else}{models.length} моделей найдено{/if}
      </p>
    </div>
    <div class="head-actions">
      <input class="input search" placeholder="Поиск…" bind:value={query} />
      <button class="btn" onclick={refresh} disabled={loading}>⟳ Обновить</button>
    </div>
  </header>

  {#if hw}
    <div class="hwbar">
      {#if hw.gpu}
        <span class="hw-item">
          <span class="hw-k">GPU</span>
          {hw.gpu.name} · {formatBytes(hw.gpu.vram_bytes)}
        </span>
      {:else}
        <span class="hw-item hw-warn"><span class="hw-k">GPU</span> не обнаружена</span>
      {/if}
      <span class="hw-item"><span class="hw-k">RAM</span> {formatBytes(hw.total_ram_bytes)}</span>
      <span class="hw-item"><span class="hw-k">CPU</span> {hw.physical_cores} ядер / {hw.logical_cores} потоков</span>
    </div>
  {/if}

  {#if error}
    <div class="glass note bad">
      Не удалось просканировать папки: {error}
    </div>
  {:else if loading}
    <div class="muted center">Загрузка…</div>
  {:else if models.length === 0}
    <div class="glass empty">
      <div class="empty-orb"></div>
      <h3>Моделей пока нет</h3>
      <p>В указанных папках не найдено ни одного .gguf файла.</p>
      <p class="dim">Проверь папки в Настройках или скачай модель в Каталоге.</p>
    </div>
  {:else}
    <div class="layout">
      <div class="list">
        {#each filtered as m (m.path)}
          <button
            class="card {selected === m.path ? 'sel' : ''}"
            onclick={() => select(m)}
          >
            <div class="card-main">
              <span class="name" title={m.name}>{m.name}</span>
              <span class="size">{formatBytes(m.size)}</span>
            </div>
          </button>
        {/each}
        {#if filtered.length === 0}
          <div class="muted center small">Ничего не найдено по «{query}»</div>
        {/if}
      </div>

      <aside class="detail glass">
        {#if selectedModel}
          <h3 class="d-name">{selectedModel.name}</h3>
          <div class="d-size">{formatBytes(selectedModel.size)}</div>

          <div class="meta">
            {#if metaLoading}
              <span class="muted">Читаю метаданные…</span>
            {:else if meta}
              <div class="meta-grid">
                {#if meta.architecture}
                  <span class="k">Архитектура</span><span class="v">{meta.architecture}</span>
                {/if}
                {#if meta.n_layers}
                  <span class="k">Слоёв</span><span class="v">{meta.n_layers}</span>
                {/if}
                {#if meta.n_head}
                  <span class="k">Attn heads</span><span class="v">{meta.n_head}</span>
                {/if}
                {#if meta.n_head_kv}
                  <span class="k">KV heads</span><span class="v">{meta.n_head_kv}</span>
                {/if}
                {#if meta.n_embd}
                  <span class="k">Embedding</span><span class="v">{meta.n_embd}</span>
                {/if}
                {#if meta.ctx_train}
                  <span class="k">Контекст (train)</span><span class="v">{meta.ctx_train.toLocaleString()}</span>
                {/if}
              </div>
            {:else}
              <span class="muted">Метаданные недоступны</span>
            {/if}
          </div>

          <div class="reco">
            <div class="reco-top">
              <span class="reco-title">Авто-настройка</span>
              <label class="toggle">
                <input type="checkbox" bind:checked={useAuto} />
                <span>Авто</span>
              </label>
            </div>
            {#if autoLoading}
              <span class="muted small">Подбираю параметры…</span>
            {:else if auto}
              <div class="reco-grid">
                <span class="k">Слои на GPU</span>
                <span class="v">{auto.ngl >= 99 ? "все" : auto.ngl}</span>
                <span class="k">Контекст</span>
                <span class="v">{auto.ctx.toLocaleString()}</span>
                <span class="k">KV-кэш</span>
                <span class="v">{auto.kv_quant}</span>
                <span class="k">Потоки CPU</span>
                <span class="v">{auto.threads}</span>
                {#if auto.est_vram_bytes > 0}
                  <span class="k">≈ VRAM</span>
                  <span class="v">{formatBytes(auto.est_vram_bytes)}</span>
                {/if}
              </div>
              <p class="reco-why {auto.full_offload ? 'ok' : 'warn'}">{auto.rationale}</p>
            {:else}
              <span class="muted small">Не удалось рассчитать — будут ручные настройки.</span>
            {/if}
          </div>

          <button
            class="btn btn-primary launch"
            disabled={serverState.running || serverState.starting}
            onclick={() => launch(selectedModel)}
          >
            {#if serverState.running}
              ● Сервер уже запущен
            {:else if serverState.starting}
              Запуск…
            {:else}
              ▶ Запустить
            {/if}
          </button>
          <p class="path-hint" title={selectedModel.path}>{selectedModel.path}</p>
        {:else}
          <div class="pick-hint muted">← Выбери модель, чтобы увидеть детали</div>
        {/if}
      </aside>
    </div>
  {/if}
</div>

<style>
  .page { display: flex; flex-direction: column; gap: 18px; height: 100%; }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
  }
  h2 { font-size: 20px; }
  .sub { margin: 4px 0 0; color: var(--text-2); font-size: 13px; }
  .head-actions { display: flex; gap: 10px; align-items: center; }
  .search { width: 220px; }

  .hwbar {
    display: flex; flex-wrap: wrap; gap: 8px 10px;
    padding: 9px 13px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    font-family: var(--font-mono); letter-spacing: -.02em;
    font-size: 12px; color: var(--text-1);
  }
  .hw-item { display: inline-flex; align-items: center; gap: 6px; }
  .hw-item + .hw-item { padding-left: 10px; border-left: 1px solid var(--border); }
  .hw-k {
    font-size: 10px; font-weight: 600; letter-spacing: .06em;
    text-transform: uppercase; color: var(--accent); opacity: .9;
  }
  .hw-warn { color: var(--warn); }

  .layout {
    display: grid;
    grid-template-columns: 1fr 300px;
    gap: 18px;
    flex: 1;
    min-height: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
    padding-right: 6px;
  }
  .card {
    text-align: left;
    padding: 13px 15px;
    border-radius: var(--radius-m);
    background: var(--surface);
    border: 1px solid var(--border);
    transition: background .14s, border-color .14s, transform .06s;
  }
  .card:hover { background: var(--surface-hover); border-color: var(--border-strong); }
  .card.sel {
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .card-main { display: flex; justify-content: space-between; align-items: center; gap: 12px; }
  .name {
    font-weight: 500;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .size { color: var(--text-2); font-size: 12px; flex: none; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; }

  .detail {
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    align-self: start;
    position: sticky;
    top: 0;
  }
  .d-name { font-size: 15px; word-break: break-word; }
  .d-size { color: var(--accent); font-weight: 600; margin-top: -6px; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
  .meta-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 7px 14px;
    font-size: 13px;
  }
  .meta-grid .k { color: var(--text-2); }
  .meta-grid .v { text-align: right; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
  .launch { margin-top: 4px; }

  .reco {
    display: flex; flex-direction: column; gap: 10px;
    padding: 13px;
    background: var(--accent-soft);
    border: 1px solid var(--accent-line);
    border-radius: var(--radius-m);
  }
  .reco-top { display: flex; justify-content: space-between; align-items: center; }
  .reco-title { font-size: 13px; font-weight: 600; }
  .toggle {
    display: inline-flex; align-items: center; gap: 6px;
    font-size: 12px; color: var(--text-1); cursor: pointer;
  }
  .toggle input { accent-color: var(--accent); }
  .reco-grid {
    display: grid; grid-template-columns: auto 1fr; gap: 6px 14px; font-size: 13px;
  }
  .reco-grid .k { color: var(--text-2); }
  .reco-grid .v { text-align: right; font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-weight: 500; letter-spacing: -.02em; color: var(--accent-hover); }
  .reco-why { margin: 0; font-size: 11.5px; line-height: 1.45; }
  .reco-why.ok { color: var(--ok); }
  .reco-why.warn { color: var(--warn); }

  .path-hint {
    font-size: 11px; color: var(--text-2); margin: 0;
    font-family: var(--font-mono); letter-spacing: -.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    direction: rtl; text-align: left;
  }
  .pick-hint { text-align: center; padding: 30px 0; }

  .note { padding: 16px 18px; }
  .bad { color: var(--danger); }
  .center { text-align: center; padding: 40px 0; }
  .small { font-size: 13px; }
  .muted { color: var(--text-2); }

  .empty {
    padding: 44px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }
  .empty-orb {
    width: 56px; height: 56px; border-radius: 50%;
    background: radial-gradient(circle at 32% 30%, var(--accent-glow), transparent 70%);
    margin-bottom: 8px;
  }
  .empty h3 { font-size: 17px; }
  .empty p { margin: 0; color: var(--text-1); }
  .empty .dim { color: var(--text-2); font-size: 13px; }
</style>
