<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    hfSearch,
    hfListFiles,
    hfDownload,
    hfCancelDownload,
    formatBytes,
    type Settings,
    type HfModel,
    type HfFile,
    type DownloadProgress,
  } from "$lib/api";

  let { settings }: { settings: Settings } = $props();

  let query = $state("");
  let results = $state<HfModel[]>([]);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let searched = $state(false);

  // Раскрытый репозиторий + кэш его файлов.
  let expanded = $state<string | null>(null);
  let files = $state<Record<string, HfFile[]>>({});
  let filesLoading = $state<string | null>(null);
  let filesError = $state<string | null>(null);

  // Папка назначения (по умолчанию первая из настроек).
  let destDir = $state<string>(settings.model_folders[0] ?? "");

  // Прогресс активной загрузки.
  let dl = $state<DownloadProgress | null>(null);
  let dlDoneMsg = $state<string | null>(null);
  const busy = $derived(dl !== null && !dl.done && !dl.canceled && !dl.error);

  let unlisten: UnlistenFn | null = null;
  $effect(() => {
    listen<DownloadProgress>("download-progress", (e) => {
      const p = e.payload;
      if (p.done) {
        dl = null;
        dlDoneMsg = `«${p.file}» скачан в папку моделей. Найдёшь его во вкладке «Модели».`;
      } else if (p.canceled) {
        dl = null;
      } else if (p.error) {
        dl = null;
        searchError = `Ошибка загрузки: ${p.error}`;
      } else {
        dl = p;
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  async function runSearch() {
    const q = query.trim();
    if (!q || searching) return;
    searching = true;
    searchError = null;
    searched = true;
    expanded = null;
    try {
      results = await hfSearch(q);
    } catch (e) {
      searchError = String(e);
      results = [];
    } finally {
      searching = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") runSearch();
  }

  async function toggle(repo: string) {
    if (expanded === repo) {
      expanded = null;
      return;
    }
    expanded = repo;
    filesError = null;
    if (files[repo]) return; // из кэша
    filesLoading = repo;
    try {
      files[repo] = await hfListFiles(repo);
    } catch (e) {
      filesError = String(e);
    } finally {
      filesLoading = null;
    }
  }

  async function download(repo: string, file: HfFile) {
    if (!destDir) {
      searchError = "Не задана папка для моделей — укажи её в Настройках.";
      return;
    }
    dlDoneMsg = null;
    searchError = null;
    // Оптимистично покажем полосу до первого события прогресса.
    dl = {
      file: file.path.split("/").pop() ?? file.path,
      downloaded: 0,
      total: file.size,
      done: false,
      error: null,
      canceled: false,
    };
    try {
      await hfDownload(repo, file.path, destDir);
    } catch (e) {
      // Отмена/сбой уже отражены событием; подстрахуемся.
      dl = null;
      const msg = String(e);
      if (!msg.includes("отменена")) searchError = msg;
    }
  }

  async function cancel() {
    await hfCancelDownload();
  }

  const pct = $derived(
    dl && dl.total > 0 ? Math.min(100, (dl.downloaded / dl.total) * 100) : 0,
  );

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return String(n);
  }
</script>

<div class="page">
  <header class="head">
    <div>
      <h2>Каталог Hugging Face</h2>
      <p class="sub">Поиск и загрузка GGUF-моделей</p>
    </div>
    {#if settings.model_folders.length > 1}
      <label class="dest">
        <span>Скачивать в</span>
        <select class="input sel" bind:value={destDir}>
          {#each settings.model_folders as f}
            <option value={f}>{f}</option>
          {/each}
        </select>
      </label>
    {/if}
  </header>

  <div class="searchbar">
    <input
      class="input"
      placeholder="Например: qwen3 gguf, llama 3 8b…"
      bind:value={query}
      onkeydown={onKey}
    />
    <button class="btn btn-primary" onclick={runSearch} disabled={searching || !query.trim()}>
      {searching ? "Ищу…" : "⌕ Найти"}
    </button>
  </div>

  {#if dlDoneMsg}
    <div class="glass note ok">✓ {dlDoneMsg}</div>
  {/if}
  {#if searchError}
    <div class="glass note bad">{searchError}</div>
  {/if}

  <div class="scroll">
    {#if searching}
      <div class="muted center">Ищу на Hugging Face…</div>
    {:else if searched && results.length === 0 && !searchError}
      <div class="muted center">Ничего не найдено по «{query}».</div>
    {:else if !searched}
      <div class="hint center">
        <div class="hint-orb"></div>
        <p>Введи запрос, чтобы найти модели в формате GGUF.</p>
        <p class="dim">Скачанные файлы попадут в твою папку моделей и появятся во вкладке «Модели».</p>
      </div>
    {:else}
      <div class="list">
        {#each results as m (m.id)}
          <div class="repo {expanded === m.id ? 'open' : ''}">
            <button class="repo-head" onclick={() => toggle(m.id)}>
              <span class="repo-id" title={m.id}>{m.id}</span>
              <span class="repo-stats">
                <span title="Загрузок">↓ {fmtCount(m.downloads)}</span>
                <span title="Лайков">♥ {fmtCount(m.likes)}</span>
                <span class="chev">{expanded === m.id ? "▲" : "▼"}</span>
              </span>
            </button>

            {#if expanded === m.id}
              <div class="files">
                {#if filesLoading === m.id}
                  <div class="muted small pad">Читаю список файлов…</div>
                {:else if filesError}
                  <div class="bad small pad">{filesError}</div>
                {:else if files[m.id]?.length}
                  {#each files[m.id] as f (f.path)}
                    <div class="file">
                      <span class="file-name" title={f.path}>{f.path}</span>
                      <span class="file-size">{f.size > 0 ? formatBytes(f.size) : "—"}</span>
                      <button
                        class="btn dl"
                        disabled={busy || !destDir}
                        onclick={() => download(m.id, f)}
                      >
                        ↓ Скачать
                      </button>
                    </div>
                  {/each}
                {:else}
                  <div class="muted small pad">В этом репозитории нет .gguf-файлов.</div>
                {/if}
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if dl}
    <div class="glass dlbar">
      <div class="dl-top">
        <span class="dl-file" title={dl.file}>{dl.file}</span>
        <span class="dl-num">
          {#if dl.total > 0}
            {formatBytes(dl.downloaded)} / {formatBytes(dl.total)} · {pct.toFixed(0)}%
          {:else}
            {formatBytes(dl.downloaded)}
          {/if}
        </span>
        <button class="btn dl-cancel" onclick={cancel}>Отмена</button>
      </div>
      <div class="bar">
        <div class="bar-fill {dl.total > 0 ? '' : 'indet'}" style="width:{dl.total > 0 ? pct : 100}%"></div>
      </div>
    </div>
  {/if}
</div>

<style>
  .page { display: flex; flex-direction: column; gap: 16px; height: 100%; }
  .head { display: flex; justify-content: space-between; align-items: flex-end; gap: 16px; }
  h2 { font-size: 20px; }
  .sub { margin: 4px 0 0; color: var(--text-2); font-size: 13px; }
  .dest { display: flex; flex-direction: column; gap: 4px; font-size: 12px; color: var(--text-2); }
  .sel { width: 260px; padding: 7px 10px; font-size: 13px; }

  .searchbar { display: flex; gap: 10px; }
  .searchbar .input { flex: 1; }

  .scroll { flex: 1; min-height: 0; overflow-y: auto; padding-right: 6px; }
  .list { display: flex; flex-direction: column; gap: 8px; }

  .repo {
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: var(--surface);
    overflow: hidden;
    transition: border-color .14s;
  }
  .repo.open { border-color: var(--accent); }
  .repo-head {
    width: 100%;
    display: flex; justify-content: space-between; align-items: center; gap: 12px;
    padding: 12px 15px; text-align: left;
    transition: background .14s;
  }
  .repo-head:hover { background: var(--surface-hover); }
  .repo-id {
    font-weight: 500; font-size: 13.5px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis;
  }
  .repo-stats {
    display: inline-flex; align-items: center; gap: 12px; flex: none;
    color: var(--text-2); font-size: 12px; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em;
  }
  .chev { font-size: 9px; color: var(--accent); }

  .files {
    border-top: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.18);
    display: flex; flex-direction: column;
  }
  .file {
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center; gap: 12px;
    padding: 9px 15px;
    font-size: 13px;
  }
  .file + .file { border-top: 1px solid var(--border); }
  .file-name { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; color: var(--text-1); font-family: var(--font-mono); font-size: 12.5px; letter-spacing: -.02em; }
  .file-size { color: var(--text-2); font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; flex: none; }
  .dl { padding: 6px 12px; font-size: 12.5px; }
  .pad { padding: 11px 15px; }

  .note { padding: 12px 16px; font-size: 13px; }
  .ok { color: var(--ok); }
  .bad { color: var(--danger); }
  .small { font-size: 12.5px; }
  .muted { color: var(--text-2); }
  .center { text-align: center; padding: 30px 0; }

  .hint { color: var(--text-1); display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 40px 0; }
  .hint-orb {
    width: 52px; height: 52px; border-radius: 50%;
    background: radial-gradient(circle at 32% 30%, var(--accent-glow), transparent 70%);
    margin-bottom: 6px;
  }
  .hint p { margin: 0; }
  .hint .dim { color: var(--text-2); font-size: 12.5px; max-width: 380px; text-align: center; }

  .dlbar { padding: 13px 16px; display: flex; flex-direction: column; gap: 9px; }
  .dl-top { display: flex; align-items: center; gap: 12px; }
  .dl-file {
    font-weight: 500; font-size: 13px;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; flex: 1;
  }
  .dl-num { color: var(--text-1); font-size: 12px; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; flex: none; }
  .dl-cancel { padding: 6px 12px; font-size: 12.5px; }
  .bar { height: 7px; border-radius: 4px; background: rgba(0,0,0,.35); overflow: hidden; }
  .bar-fill {
    height: 100%; border-radius: 4px;
    background: linear-gradient(90deg, var(--accent-press), var(--accent-hover));
    box-shadow: 0 0 10px var(--accent-glow);
    transition: width .2s ease;
  }
  .bar-fill.indet { animation: indet 1.1s ease-in-out infinite; }
  @keyframes indet {
    0% { opacity: .5; } 50% { opacity: 1; } 100% { opacity: .5; }
  }
</style>
