<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    hfSearch,
    hfListFiles,
    hfDownload,
    hfCancelDownload,
    detectHardware,
    formatBytes,
    type Settings,
    type HfModel,
    type HfFile,
    type DownloadProgress,
    type HardwareInfo,
  } from "$lib/api";
  import { prefs } from "$lib/prefs.svelte";
  import Icon from "$lib/components/Icon.svelte";
  import {
    RECOMMENDED_MODELS,
    REC_CATEGORIES,
    fitInfo,
    fitFromFileBytes,
    sortByFit,
    bestForHardware,
    type RecCategory,
    type RecommendedModel,
    type FitLevel,
    type FitInfo,
  } from "$lib/recommended";

  let { settings }: { settings: Settings } = $props();

  let query = $state("");
  let results = $state<HfModel[]>([]);
  let searching = $state(false);
  let searchError = $state<string | null>(null);
  let searched = $state(false);
  const PAGE = 40;
  let limit = $state(PAGE);
  let loadingMore = $state(false);

  let expanded = $state<string | null>(null);
  let files = $state<Record<string, HfFile[]>>({});
  let filesLoading = $state<string | null>(null);
  let filesError = $state<string | null>(null);

  let destDir = $state<string>(settings.model_folders[0] ?? "");

  let dl = $state<DownloadProgress | null>(null);
  let dlDoneMsg = $state<string | null>(null);
  const busy = $derived(dl !== null && !dl.done && !dl.canceled && !dl.error);

  let hw = $state<HardwareInfo | null>(null);
  let recFilter = $state<RecCategory | "all">("all");
  /** Only show models that fit (ok / tight). */
  let onlyFits = $state(false);

  detectHardware()
    .then((h) => (hw = h))
    .catch(() => (hw = null));

  let unlisten: UnlistenFn | null = null;
  $effect(() => {
    listen<DownloadProgress>("download-progress", (e) => {
      const p = e.payload;
      if (p.done) {
        dl = null;
        dlDoneMsg = prefs.t("cat.done", { file: p.file });
      } else if (p.canceled) {
        dl = null;
      } else if (p.error) {
        dl = null;
        searchError = prefs.t("cat.err_dl", { err: p.error });
      } else {
        dl = p;
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  const vram = $derived(hw?.gpu?.vram_bytes ?? null);
  const ram = $derived(hw?.total_ram_bytes ?? null);

  const filteredRecs = $derived.by(() => {
    let list =
      recFilter === "all"
        ? RECOMMENDED_MODELS
        : RECOMMENDED_MODELS.filter((m) => m.category === recFilter);
    if (onlyFits) {
      list = list.filter((m) => {
        const lvl = fitInfo(m, vram, ram).level;
        return lvl === "ok" || lvl === "tight";
      });
    }
    return sortByFit(list, vram, ram);
  });

  const pick = $derived(bestForHardware(vram, ram));

  function fitOf(m: RecommendedModel): FitInfo {
    return fitInfo(m, vram, ram);
  }

  function fitLabel(f: FitLevel): string {
    if (f === "ok") return prefs.t("rec.fit.ok");
    if (f === "tight") return prefs.t("rec.fit.tight");
    if (f === "no") return prefs.t("rec.fit.no");
    return prefs.t("rec.fit.unknown");
  }

  function hwSummary(): string {
    if (!hw) return prefs.t("rec.hw.loading");
    if (hw.gpu) {
      return prefs.t("rec.hw.gpu", {
        name: hw.gpu.name,
        vram: formatBytes(hw.gpu.vram_bytes),
        ram: formatBytes(hw.total_ram_bytes),
      });
    }
    return prefs.t("rec.hw.cpu", { ram: formatBytes(hw.total_ram_bytes) });
  }

  async function runSearch() {
    const q = query.trim();
    if (!q || searching) return;
    searching = true;
    searchError = null;
    searched = true;
    expanded = null;
    limit = PAGE;
    try {
      results = await hfSearch(q, limit);
    } catch (e) {
      searchError = String(e);
      results = [];
    } finally {
      searching = false;
    }
  }

  const canLoadMore = $derived(
    searched && !searching && results.length >= limit && limit < 100,
  );
  async function loadMore() {
    if (loadingMore || !canLoadMore) return;
    loadingMore = true;
    searchError = null;
    limit = Math.min(100, limit + PAGE);
    try {
      results = await hfSearch(query.trim(), limit);
    } catch (e) {
      searchError = String(e);
    } finally {
      loadingMore = false;
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
    if (files[repo]) return;
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
      searchError = prefs.t("cat.err_folder");
      return;
    }
    dlDoneMsg = null;
    searchError = null;
    dl = {
      file: file.path.split("/").pop() ?? file.path,
      downloaded: 0,
      total: file.size,
      done: false,
      error: null,
      canceled: false,
    };
    try {
      await hfDownload(
        repo,
        file.path,
        destDir,
        file.size > 0 ? file.size : null,
      );
    } catch (e) {
      dl = null;
      const msg = String(e);
      if (!msg.includes("отменена") && !msg.toLowerCase().includes("cancel")) {
        searchError = msg;
      }
    }
  }

  async function downloadRecommended(m: RecommendedModel) {
    await download(m.hfRepo, { path: m.file, size: m.fileBytes });
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

  function fileFit(f: HfFile): FitInfo {
    return fitFromFileBytes(f.size, vram, ram);
  }
</script>

<div class="page">
  <header class="head">
    <div>
      <h2>{prefs.t("cat.title")}</h2>
      <p class="sub">
        {prefs.isBeginner ? prefs.t("cat.sub_beginner") : prefs.t("cat.sub")}
      </p>
    </div>
    {#if settings.model_folders.length > 1 && prefs.showPowerPaths}
      <label class="dest">
        <span>{prefs.t("cat.dest")}</span>
        <select class="input sel" bind:value={destDir}>
          {#each settings.model_folders as f}
            <option value={f}>{f}</option>
          {/each}
        </select>
      </label>
    {/if}
  </header>

  {#if dlDoneMsg}
    <div class="glass note ok">✓ {dlDoneMsg}</div>
  {/if}
  {#if searchError}
    <div class="glass note bad selectable">{searchError}</div>
  {/if}

  <div class="scroll">
    <!-- ── Recommended ──────────────────────────────────────────────── -->
    <section class="rec-block">
      <div class="rec-head">
        <div>
          <h3 class="rec-title">{prefs.t("rec.title")}</h3>
          <p class="rec-sub">{prefs.t("rec.sub")}</p>
        </div>
        <span class="rec-meta mono">{prefs.t("rec.updated")}</span>
      </div>

      <div class="hw-bar glass" title={hwSummary()}>
        <span class="hw-ic" aria-hidden="true">◈</span>
        <span class="hw-text">{hwSummary()}</span>
        {#if pick && hw}
          <span class="hw-pick mono">
            {prefs.t("rec.hw.pick", { name: prefs.t(pick.titleKey) })}
          </span>
        {/if}
      </div>

      <div class="chips">
        <button
          type="button"
          class="chip fit-chip {onlyFits ? 'on' : ''}"
          onclick={() => (onlyFits = !onlyFits)}
        >
          {prefs.t("rec.filter.fits")}
        </button>
        {#each REC_CATEGORIES as c}
          <button
            type="button"
            class="chip {recFilter === c.id ? 'on' : ''}"
            onclick={() => (recFilter = c.id)}
          >
            {prefs.t(c.labelKey)}
          </button>
        {/each}
      </div>

      {#if filteredRecs.length === 0}
        <p class="muted center rec-empty">{prefs.t("rec.filter.empty")}</p>
      {:else}
        <div class="rec-grid">
          {#each filteredRecs as m (m.id)}
            {@const fit = fitOf(m)}
            {@const isPick = pick?.id === m.id}
            <article
              class="rec-card {m.featured ? 'featured' : ''} {isPick ? 'is-pick' : ''} fit-{fit.level}"
            >
              <div class="rec-top">
                <span class="family">{m.family}</span>
                <div class="rec-tags">
                  {#if isPick}
                    <span class="star">{prefs.t("rec.for_you")}</span>
                  {:else if m.featured}
                    <span class="star soft">{prefs.t("rec.featured")}</span>
                  {/if}
                </div>
              </div>
              <h4 class="rec-name">{prefs.t(m.titleKey)}</h4>
              <p class="rec-blurb">{prefs.t(m.blurbKey)}</p>
              <div class="rec-badges">
                <span class="badge">{m.sizeLabel}</span>
                <span class="badge mono">{formatBytes(m.fileBytes)}</span>
                <span class="badge fit {fit.level}">{fitLabel(fit.level)}</span>
              </div>
              <div class="rec-foot">
                <span class="vram muted">
                  {prefs.t("rec.vram", { n: formatBytes(m.vramHintBytes) })}
                </span>
                <button
                  type="button"
                  class="btn btn-primary rec-dl"
                  disabled={busy || !destDir}
                  onclick={() => downloadRecommended(m)}
                >
                  <Icon name="catalog" size={14} />
                  {busy ? prefs.t("rec.downloading") : prefs.t("rec.download")}
                </button>
              </div>
            </article>
          {/each}
        </div>
      {/if}
    </section>

    <!-- ── HF search ─────────────────────────────────────────────────── -->
    <section class="search-block">
      <h3 class="sec-title">{prefs.t("cat.search_section")}</h3>
      <div class="searchbar">
        <div class="search-wrap">
          <span class="search-ic"><Icon name="search" size={15} /></span>
          <input
            class="input"
            placeholder={prefs.t("cat.ph")}
            bind:value={query}
            onkeydown={onKey}
          />
        </div>
        <button
          class="btn btn-primary"
          onclick={runSearch}
          disabled={searching || !query.trim()}
        >
          {searching ? prefs.t("cat.searching") : prefs.t("cat.find")}
        </button>
      </div>

      {#if searching}
        <div class="muted center">{prefs.t("cat.searching_long")}</div>
      {:else if searched && results.length === 0 && !searchError}
        <div class="muted center">{prefs.t("cat.none", { q: query })}</div>
      {:else if !searched}
        <div class="hint center">
          <p class="dim">{prefs.t("cat.hint_dim")}</p>
        </div>
      {:else}
        <div class="list">
          {#each results as m (m.id)}
            <div class="repo {expanded === m.id ? 'open' : ''}">
              <button class="repo-head" onclick={() => toggle(m.id)}>
                <span class="repo-id" title={m.id}>{m.id}</span>
                <span class="repo-stats">
                  <span>↓ {fmtCount(m.downloads)}</span>
                  <span>♥ {fmtCount(m.likes)}</span>
                  <span class="chev">{expanded === m.id ? "▲" : "▼"}</span>
                </span>
              </button>

              {#if expanded === m.id}
                <div class="files">
                  {#if filesLoading === m.id}
                    <div class="muted small pad">{prefs.t("cat.files_loading")}</div>
                  {:else if filesError}
                    <div class="bad small pad">{filesError}</div>
                  {:else if files[m.id]?.length}
                    {#each files[m.id] as f (f.path)}
                      {@const ff = fileFit(f)}
                      <div class="file">
                        <span class="file-name" title={f.path}>{f.path}</span>
                        <span class="file-size">
                          {f.size > 0 ? formatBytes(f.size) : "—"}
                        </span>
                        {#if f.size > 0}
                          <span class="badge fit tiny {ff.level}"
                            >{fitLabel(ff.level)}</span
                          >
                        {/if}
                        <button
                          class="btn dl"
                          disabled={busy || !destDir}
                          onclick={() => download(m.id, f)}
                        >
                          {prefs.t("cat.dl")}
                        </button>
                      </div>
                    {/each}
                  {:else}
                    <div class="muted small pad">{prefs.t("cat.no_gguf")}</div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>
        {#if canLoadMore}
          <div class="more">
            <button class="btn" onclick={loadMore} disabled={loadingMore}>
              {loadingMore ? prefs.t("cat.loading_more") : prefs.t("cat.more")}
            </button>
          </div>
        {/if}
      {/if}
    </section>
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
        <button class="btn dl-cancel" onclick={cancel}>{prefs.t("cat.cancel")}</button>
      </div>
      <div class="bar">
        <div
          class="bar-fill {dl.total > 0 ? '' : 'indet'}"
          style="width:{dl.total > 0 ? pct : 100}%"
        ></div>
      </div>
    </div>
  {/if}
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 16px;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 16px;
  }
  h2 {
    font-size: 20px;
    letter-spacing: -0.02em;
  }
  .sub {
    margin: 4px 0 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .dest {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: var(--text-2);
    min-width: 200px;
  }
  .sel {
    font-size: 12px;
    max-width: 280px;
  }
  .note {
    padding: 10px 14px;
    font-size: 13px;
  }
  .note.ok {
    color: var(--ok);
  }
  .note.bad {
    color: var(--danger);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding-right: 4px;
  }

  /* ── Recommended ── */
  .rec-block {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .rec-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-end;
    gap: 12px;
  }
  .rec-title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .rec-sub {
    margin: 4px 0 0;
    font-size: 12.5px;
    color: var(--text-2);
  }
  .rec-meta {
    font-size: 11px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .hw-bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 16px;
    padding: 10px 14px;
    font-size: 12.5px;
  }
  .hw-ic {
    color: var(--accent);
    font-size: 12px;
  }
  .hw-text {
    color: var(--text-1);
    flex: 1;
    min-width: 160px;
  }
  .hw-pick {
    font-size: 11.5px;
    color: var(--accent-hover);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    padding: 6px 12px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.2);
    font-size: 12px;
    color: var(--text-1);
    transition:
      border-color 0.12s,
      background 0.12s,
      color 0.12s;
  }
  .chip:hover {
    background: var(--surface-hover);
    color: var(--text-0);
  }
  .chip.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-hover);
  }
  .chip.fit-chip.on {
    border-color: rgba(75, 208, 127, 0.45);
    background: rgba(75, 208, 127, 0.12);
    color: var(--ok);
  }
  .rec-empty {
    margin: 8px 0;
    font-size: 13px;
  }
  .rec-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 12px;
  }
  .rec-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 16px;
    border-radius: var(--radius-m);
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.18);
    transition:
      border-color 0.15s,
      box-shadow 0.15s;
  }
  .rec-card.featured,
  .rec-card.is-pick {
    border-color: rgba(var(--accent-rgb, 120, 160, 255), 0.35);
  }
  .rec-card.is-pick {
    box-shadow: 0 0 0 1px rgba(75, 208, 127, 0.15);
    border-color: rgba(75, 208, 127, 0.4);
  }
  .rec-card.fit-no {
    opacity: 0.82;
  }
  .rec-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .family {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--accent);
  }
  .rec-tags {
    display: flex;
    gap: 6px;
  }
  .star {
    font-size: 10.5px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 999px;
    color: var(--accent-ink);
    background: linear-gradient(180deg, var(--accent-hover), var(--accent));
  }
  .star.soft {
    color: var(--text-1);
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid var(--border);
  }
  .rec-name {
    margin: 0;
    font-size: 14.5px;
    font-weight: 600;
    letter-spacing: -0.015em;
    line-height: 1.3;
  }
  .rec-blurb {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--text-1);
    flex: 1;
  }
  .rec-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .badge {
    font-size: 10.5px;
    font-weight: 600;
    padding: 2px 8px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.22);
    color: var(--text-1);
  }
  .badge.tiny {
    font-size: 10px;
    padding: 1px 6px;
    flex-shrink: 0;
  }
  .badge.fit.ok {
    color: var(--ok);
    border-color: rgba(75, 208, 127, 0.35);
    background: rgba(75, 208, 127, 0.1);
  }
  .badge.fit.tight {
    color: var(--warn);
    border-color: rgba(255, 194, 71, 0.35);
    background: rgba(255, 194, 71, 0.1);
  }
  .badge.fit.no {
    color: var(--danger);
    border-color: var(--danger-line);
    background: var(--danger-soft);
  }
  .badge.fit.unknown {
    color: var(--text-2);
  }
  .rec-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    margin-top: 4px;
  }
  .vram {
    font-size: 11.5px;
  }
  .rec-dl {
    padding: 7px 12px;
    font-size: 12.5px;
  }

  /* ── HF search ── */
  .sec-title {
    margin: 0 0 10px;
    font-size: 14px;
    font-weight: 600;
    color: var(--text-1);
  }
  .searchbar {
    display: flex;
    gap: 10px;
    margin-bottom: 14px;
  }
  .search-wrap {
    flex: 1;
    position: relative;
    display: flex;
    align-items: center;
  }
  .search-ic {
    position: absolute;
    left: 11px;
    color: var(--text-2);
    pointer-events: none;
    display: grid;
    place-items: center;
  }
  .search-wrap .input {
    padding-left: 34px;
    width: 100%;
  }
  .center {
    text-align: center;
    padding: 20px 12px;
  }
  .muted {
    color: var(--text-2);
    font-size: 13px;
  }
  .dim {
    margin: 0;
    color: var(--text-2);
    font-size: 13px;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .repo {
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    background: rgba(0, 0, 0, 0.15);
    overflow: hidden;
  }
  .repo.open {
    border-color: var(--border-strong, var(--border));
  }
  .repo-head {
    width: 100%;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 11px 14px;
    background: none;
    border: none;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .repo-head:hover {
    background: var(--surface-hover);
  }
  .repo-id {
    font-family: var(--font-mono);
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .repo-stats {
    display: flex;
    gap: 12px;
    font-size: 11.5px;
    color: var(--text-2);
    flex-shrink: 0;
  }
  .chev {
    color: var(--text-2);
  }
  .files {
    border-top: 1px solid var(--border);
    display: flex;
    flex-direction: column;
  }
  .file {
    display: grid;
    grid-template-columns: 1fr auto auto auto;
    align-items: center;
    gap: 10px;
    padding: 8px 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.04);
  }
  .file:last-child {
    border-bottom: none;
  }
  .file-name {
    font-family: var(--font-mono);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file-size {
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .dl {
    padding: 5px 10px;
    font-size: 12px;
  }
  .pad {
    padding: 12px 14px;
  }
  .small {
    font-size: 12.5px;
  }
  .more {
    display: flex;
    justify-content: center;
    margin-top: 12px;
  }
  .bad {
    color: var(--danger);
  }

  /* ── Download bar ── */
  .dlbar {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .dl-top {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .dl-file {
    flex: 1;
    font-family: var(--font-mono);
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dl-num {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-2);
    white-space: nowrap;
  }
  .dl-cancel {
    padding: 5px 10px;
    font-size: 12px;
  }
  .bar {
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.08);
    overflow: hidden;
  }
  .bar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: 2px;
    transition: width 0.2s ease;
  }
  .bar-fill.indet {
    width: 40% !important;
    animation: indet 1.2s ease-in-out infinite;
  }
  @keyframes indet {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(350%);
    }
  }
  .mono {
    font-family: var(--font-mono);
  }
  .selectable {
    user-select: text;
  }
</style>
