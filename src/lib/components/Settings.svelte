<script lang="ts">
  import {
    validateLlamaDir,
    pickFolder,
    saveSettings,
    type Settings,
  } from "$lib/api";

  let { settings, onchange }: {
    settings: Settings;
    onchange: (s: Settings) => void;
  } = $props();

  // Локальная рабочая копия.
  let draft = $state<Settings>(structuredClone($state.snapshot(settings)));
  let llamaValid = $state<boolean | null>(null);
  let saved = $state(false);

  async function checkLlama() {
    if (!draft.llama_dir) { llamaValid = false; return; }
    llamaValid = await validateLlamaDir(draft.llama_dir);
  }

  async function browseLlama() {
    const dir = await pickFolder("Папка с llama-server.exe");
    if (dir) { draft.llama_dir = dir; await checkLlama(); }
  }

  async function addFolder() {
    const dir = await pickFolder("Папка с моделями (.gguf)");
    if (dir && !draft.model_folders.includes(dir)) {
      draft.model_folders = [...draft.model_folders, dir];
    }
  }

  function removeFolder(f: string) {
    draft.model_folders = draft.model_folders.filter((x) => x !== f);
  }

  async function save() {
    await saveSettings($state.snapshot(draft));
    onchange($state.snapshot(draft));
    saved = true;
    setTimeout(() => (saved = false), 1800);
  }

  $effect(() => { if (llamaValid === null) checkLlama(); });

  const KV_OPTS = ["f16", "q8_0", "q4_0"];
</script>

<div class="page">
  <header><h2>Настройки</h2></header>

  <div class="glass block">
    <span class="lbl">Папка llama.cpp</span>
    <div class="row">
      <input class="input" bind:value={draft.llama_dir}
        oninput={() => (llamaValid = null)} onblur={checkLlama} />
      <button class="btn" onclick={browseLlama}>Обзор…</button>
    </div>
    <div class="hint">
      {#if llamaValid === true}<span class="ok">✓ llama-server.exe найден</span>
      {:else if llamaValid === false}<span class="bad">✕ llama-server.exe не найден</span>
      {:else}<span class="muted">Проверка…</span>{/if}
    </div>
  </div>

  <div class="glass block">
    <span class="lbl">Папки с моделями</span>
    {#each draft.model_folders as f (f)}
      <div class="chip">
        <span class="path" title={f}>{f}</span>
        <button class="x" onclick={() => removeFolder(f)} aria-label="Убрать">✕</button>
      </div>
    {/each}
    <button class="btn add" onclick={addFolder}>+ Добавить папку</button>
  </div>

  <div class="glass block">
    <span class="lbl">Параметры запуска по умолчанию</span>
    <div class="grid">
      <div class="fld">
        <span class="fl">Контекст</span>
        <input class="input" type="number" min="512" step="512" bind:value={draft.defaults.ctx} />
      </div>
      <div class="fld">
        <span class="fl">KV-квант</span>
        <select class="input" bind:value={draft.defaults.kv_quant}>
          {#each KV_OPTS as k}<option value={k}>{k}</option>{/each}
        </select>
      </div>
      <div class="fld">
        <span class="fl">Потоки CPU</span>
        <input class="input" type="number" min="1" max="64" bind:value={draft.defaults.threads} />
      </div>
      <div class="fld">
        <span class="fl">Слоёв на GPU (-ngl)</span>
        <input class="input" type="number" min="0" max="999" bind:value={draft.defaults.ngl} />
      </div>
      <div class="fld">
        <span class="fl">Порт</span>
        <input class="input" type="number" min="1024" max="65535" bind:value={draft.defaults.port} />
      </div>
      <div class="fld check">
        <label class="chk">
          <input type="checkbox" bind:checked={draft.defaults.tools} />
          <span>Инструменты (--tools all)</span>
        </label>
      </div>
    </div>
  </div>

  <div class="save-row">
    <button class="btn btn-primary" onclick={save}>Сохранить</button>
    {#if saved}<span class="saved-msg">✓ Сохранено</span>{/if}
  </div>
</div>

<style>
  .page { display: flex; flex-direction: column; gap: 16px; overflow-y: auto; padding-right: 6px; }
  h2 { font-size: 20px; }
  .block { padding: 18px 20px; display: flex; flex-direction: column; gap: 12px; }
  .lbl {
    font-size: 12px; text-transform: uppercase; letter-spacing: .06em;
    color: var(--text-2); font-weight: 600;
  }
  .row { display: flex; gap: 10px; }
  .row .input { flex: 1; }
  .hint { font-size: 12.5px; }
  .ok { color: var(--ok); } .bad { color: var(--danger); } .muted { color: var(--text-2); }
  .chip {
    display: flex; align-items: center; gap: 10px;
    padding: 9px 12px; background: rgba(0,0,0,.22);
    border: 1px solid var(--border); border-radius: var(--radius-m);
  }
  .chip .path {
    flex: 1; font-size: 12.5px; color: var(--text-1);
    font-family: var(--font-mono); letter-spacing: -.02em;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
    direction: rtl; text-align: left;
  }
  .x { color: var(--text-2); font-size: 12px; padding: 2px 6px; border-radius: 6px; }
  .x:hover { color: var(--danger); background: rgba(255,107,107,.12); }
  .add { align-self: flex-start; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 14px; }
  .fld { display: flex; flex-direction: column; gap: 6px; }
  .fl { font-size: 12.5px; color: var(--text-1); }
  .check { justify-content: flex-end; }
  .chk { display: flex; align-items: center; gap: 8px; font-size: 13px; cursor: pointer; }
  .chk input { width: 16px; height: 16px; accent-color: var(--accent); }
  .save-row { display: flex; align-items: center; gap: 14px; padding-bottom: 8px; }
  .saved-msg { color: var(--ok); font-size: 13px; }
</style>
