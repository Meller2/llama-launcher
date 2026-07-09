<script lang="ts">
  import {
    validateLlamaDir,
    pickFolder,
    saveSettings,
    type Settings,
  } from "$lib/api";

  let { settings, oncomplete }: {
    settings: Settings;
    oncomplete: (s: Settings) => void;
  } = $props();

  // Дефолты-подсказки (не хардкод путей в логике — просто предзаполнение).
  let llamaDir = $state(settings.llama_dir ?? "C:\\Programs\\llama.cpp");
  let modelFolders = $state<string[]>(
    settings.model_folders.length
      ? [...settings.model_folders]
      : ["F:\\programs\\lm studio models"],
  );

  let llamaValid = $state<boolean | null>(null);
  let checking = $state(false);
  let saving = $state(false);

  async function checkLlama() {
    checking = true;
    llamaValid = await validateLlamaDir(llamaDir);
    checking = false;
  }

  async function browseLlama() {
    const dir = await pickFolder("Папка с llama-server.exe");
    if (dir) {
      llamaDir = dir;
      await checkLlama();
    }
  }

  async function addModelFolder() {
    const dir = await pickFolder("Папка с моделями (.gguf)");
    if (dir && !modelFolders.includes(dir)) {
      modelFolders = [...modelFolders, dir];
    }
  }

  function removeFolder(f: string) {
    modelFolders = modelFolders.filter((x) => x !== f);
  }

  const canFinish = $derived(llamaValid === true && modelFolders.length > 0);

  async function finish() {
    if (!canFinish) return;
    saving = true;
    const updated: Settings = {
      ...settings,
      llama_dir: llamaDir,
      model_folders: modelFolders,
      onboarded: true,
    };
    await saveSettings(updated);
    saving = false;
    oncomplete(updated);
  }

  // Проверим стартовый путь сразу.
  $effect(() => {
    if (llamaValid === null) checkLlama();
  });
</script>

<div class="onb-wrap">
  <div class="glass onb-card">
    <div class="brand">
      <div class="logo-orb"></div>
      <div>
        <h1>LlamaLauncher</h1>
        <p class="sub">Запуск локальных нейросетей в один клик</p>
      </div>
    </div>

    <p class="lead">
      Давай настроим за минуту. Укажи, где лежит <b>llama.cpp</b> и где хранятся
      твои модели.
    </p>

    <!-- Шаг 1: llama.cpp -->
    <section class="field">
      <span class="lbl">1 · Папка llama.cpp</span>
      <div class="row">
        <input
          class="input"
          bind:value={llamaDir}
          oninput={() => (llamaValid = null)}
          onblur={checkLlama}
          placeholder="C:\Programs\llama.cpp"
        />
        <button class="btn" onclick={browseLlama}>Обзор…</button>
      </div>
      <div class="hint">
        {#if checking}
          <span class="muted">Проверяю…</span>
        {:else if llamaValid === true}
          <span class="ok">✓ llama-server.exe найден</span>
        {:else if llamaValid === false}
          <span class="bad">✕ llama-server.exe не найден в этой папке</span>
        {:else}
          <span class="muted">Здесь должен лежать llama-server.exe</span>
        {/if}
      </div>
    </section>

    <!-- Шаг 2: папки моделей -->
    <section class="field">
      <span class="lbl">2 · Папки с моделями (.gguf)</span>
      {#each modelFolders as folder (folder)}
        <div class="folder-chip">
          <span class="path" title={folder}>{folder}</span>
          <button class="x" onclick={() => removeFolder(folder)} aria-label="Убрать">✕</button>
        </div>
      {/each}
      <button class="btn add" onclick={addModelFolder}>+ Добавить папку</button>
    </section>

    <button class="btn btn-primary finish" disabled={!canFinish || saving} onclick={finish}>
      {saving ? "Сохраняю…" : "Начать"}
    </button>
  </div>
</div>

<style>
  .onb-wrap {
    height: 100vh;
    display: grid;
    place-items: center;
    padding: 24px;
  }
  .onb-card {
    width: min(560px, 100%);
    padding: 34px 34px 30px;
    display: flex;
    flex-direction: column;
    gap: 22px;
    animation: rise 0.4s cubic-bezier(0.2, 0.7, 0.2, 1);
  }
  @keyframes rise {
    from { opacity: 0; transform: translateY(14px); }
    to { opacity: 1; transform: translateY(0); }
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 16px;
  }
  .logo-orb {
    width: 46px;
    height: 46px;
    border-radius: 13px;
    background:
      radial-gradient(circle at 32% 26%, #ffbb74, var(--accent) 42%, #b45f16 78%, #5c2f08);
    box-shadow:
      0 6px 22px var(--accent-glow),
      inset 0 1px 2px rgba(255, 236, 210, 0.6),
      inset 0 -4px 10px rgba(0, 0, 0, 0.4);
    flex: none;
  }
  h1 { font-size: 22px; }
  .sub { margin: 2px 0 0; color: var(--text-1); font-size: 13px; }
  .lead { margin: 0; color: var(--text-1); line-height: 1.5; }
  .field { display: flex; flex-direction: column; gap: 10px; }
  .lbl {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
    font-weight: 600;
  }
  .row { display: flex; gap: 10px; }
  .row .input { flex: 1; }
  .hint { font-size: 12.5px; min-height: 16px; }
  .ok { color: var(--ok); }
  .bad { color: var(--danger); }
  .muted { color: var(--text-2); }
  .folder-chip {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 9px 12px;
    background: rgba(0,0,0,.22);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
  }
  .folder-chip .path {
    flex: 1;
    font-size: 12.5px;
    color: var(--text-1);
    font-family: var(--font-mono);
    letter-spacing: -.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .x {
    color: var(--text-2);
    font-size: 12px;
    padding: 2px 6px;
    border-radius: 6px;
  }
  .x:hover { color: var(--danger); background: rgba(255,107,107,.12); }
  .add { align-self: flex-start; }
  .finish { margin-top: 4px; padding: 13px; font-size: 15px; }
</style>
