<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    scanModels,
    readGgufMeta,
    detectHardware,
    autoConfig,
    formatBytes,
    revealInFolder,
    type Settings,
    type ModelInfo,
    type GgufMeta,
    type LaunchConfig,
    type HardwareInfo,
    type AutoConfig,
  } from "$lib/api";
  import { serverState } from "$lib/server.svelte";
  import { prefs } from "$lib/prefs.svelte";
  import Icon from "$lib/components/Icon.svelte";
  import ContextMenu, { type MenuItem } from "$lib/components/ContextMenu.svelte";
  import {
    LAUNCH_PROFILES,
    applyLaunchProfile,
    type LaunchProfileId,
  } from "$lib/profiles";

  let {
    settings,
    onlaunch,
    oncatalog,
  }: {
    settings: Settings;
    onlaunch: () => void;
    oncatalog?: () => void;
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
  /** Launch preset on top of auto-config (or settings defaults). */
  let profile = $state<LaunchProfileId>("balanced");
  let selectGen = 0;
  $effect(() => {
    if (!prefs.canDisableAuto) useAuto = true;
  });

  /** Красивое имя + квант из имени файла (Q4_K_M, IQ4_XS, F16…). */
  function parseLabel(filename: string): { title: string; quant: string | null } {
    const raw = filename.replace(/\.gguf$/i, "");
    const re =
      /(?:^|[-_.])((?:IQ|Q)\d+(?:_[A-Z0-9]+)?|F16|F32|BF16)(?:$|[-_.])/i;
    const m = raw.match(re);
    const quant = m ? m[1].toUpperCase().replace(/_/g, "_") : null;
    let title = raw;
    if (quant) {
      title = raw
        .replace(new RegExp(`[-_.]?${m![1]}[-_.]?`, "i"), "-")
        .replace(/[-_]+/g, "-")
        .replace(/^-|-$/g, "");
    }
    title = title.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim();
    return { title: title || raw, quant };
  }

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
    const gen = ++selectGen;
    const path = m.path;
    selected = path;
    meta = null;
    auto = null;
    metaLoading = true;
    autoLoading = true;
    readGgufMeta(path)
      .then((r) => {
        if (gen === selectGen) meta = r;
      })
      .catch(() => {
        if (gen === selectGen) meta = null;
      })
      .finally(() => {
        if (gen === selectGen) metaLoading = false;
      });
    autoConfig(path)
      .then((r) => {
        if (gen === selectGen) auto = r;
      })
      .catch(() => {
        if (gen === selectGen) auto = null;
      })
      .finally(() => {
        if (gen === selectGen) autoLoading = false;
      });
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

  const selectedLabel = $derived(
    selectedModel ? parseLabel(selectedModel.name) : null,
  );

  const waitingAuto = $derived(
    (!prefs.canDisableAuto || useAuto) && autoLoading,
  );

  const launchDisabled = $derived(
    !settings.llama_dir ||
      serverState.running ||
      serverState.starting ||
      waitingAuto,
  );

  const fitKind = $derived.by((): "full" | "partial" | "cpu" | "wait" | null => {
    if (!selectedModel) return null;
    if (autoLoading) return "wait";
    const p = resolvedParams;
    if (!p) return null;
    if (p.ngl === 0) return "cpu";
    if (auto?.full_offload && profile !== "cpu") return "full";
    if (p.ngl > 0) return "partial";
    return "cpu";
  });

  /** Effective launch knobs for the current profile / auto / defaults. */
  const resolvedParams = $derived.by(() => {
    if (!selectedModel) return null;
    const autoOn = !prefs.canDisableAuto || useAuto;
    if (!autoOn) {
      return {
        ctx: settings.defaults.ctx,
        kv_quant: settings.defaults.kv_quant,
        threads: settings.defaults.threads,
        ngl: settings.defaults.ngl,
      };
    }
    if (autoLoading) return null;
    return applyLaunchProfile(profile, auto, settings.defaults);
  });

  const activeProfileMeta = $derived(
    LAUNCH_PROFILES.find((p) => p.id === profile) ?? LAUNCH_PROFILES[0],
  );

  async function launch(m: ModelInfo) {
    if (!settings.llama_dir || launchDisabled) return;
    const p = resolvedParams;
    if (!p) return;
    const cfg: LaunchConfig = {
      llama_dir: settings.llama_dir,
      model_path: m.path,
      ctx: p.ctx,
      kv_quant: p.kv_quant,
      threads: p.threads,
      ngl: p.ngl,
      port: settings.defaults.port,
      tools: settings.defaults.tools,
    };
    onlaunch();
    await serverState.start(cfg);
  }

  $effect(() => {
    settings.model_folders;
    refresh();
  });

  let unlistenModels: UnlistenFn | null = null;
  $effect(() => {
    listen<string>("models-changed", () => {
      refresh();
    }).then((u) => (unlistenModels = u));
    return () => unlistenModels?.();
  });

  // ── ПКМ-меню по карточке модели ───────────────────────────────────────────
  let ctx = $state<{
    x: number;
    y: number;
    model: ModelInfo;
  } | null>(null);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  function showToast(msg: string) {
    toast = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 1600);
  }

  function openCtx(e: MouseEvent, m: ModelInfo) {
    e.preventDefault();
    e.stopPropagation();
    select(m);
    ctx = { x: e.clientX, y: e.clientY, model: m };
  }

  const ctxItems = $derived.by((): MenuItem[] => {
    if (!ctx) return [];
    return [
      {
        id: "launch",
        label: prefs.t("models.ctx.launch"),
        disabled: launchDisabled,
      },
      { type: "sep" },
      { id: "copy_name", label: prefs.t("models.ctx.copy_name") },
      { id: "copy_path", label: prefs.t("models.ctx.copy_path") },
      { id: "reveal", label: prefs.t("models.ctx.reveal") },
    ];
  });

  async function onCtxPick(id: string) {
    const m = ctx?.model;
    if (!m) return;
    if (id === "launch") {
      await launch(m);
      return;
    }
    if (id === "copy_name") {
      try {
        await navigator.clipboard.writeText(m.name);
        showToast(prefs.t("models.ctx.copied"));
      } catch {
        /* */
      }
      return;
    }
    if (id === "copy_path") {
      try {
        await navigator.clipboard.writeText(m.path);
        showToast(prefs.t("models.ctx.copied"));
      } catch {
        /* */
      }
      return;
    }
    if (id === "reveal") {
      try {
        await revealInFolder(m.path);
      } catch (e) {
        error = String(e);
      }
    }
  }
</script>

<div class="page">
  <header class="head">
    <div>
      <h2>{prefs.t("models.title")}</h2>
      <p class="sub">
        {#if loading}
          {prefs.t("models.scanning")}
        {:else}
          {prefs.t("models.count", { n: models.length })}
        {/if}
      </p>
    </div>
    <div class="head-actions">
      <div class="search-wrap">
        <span class="search-ic"><Icon name="search" size={15} /></span>
        <input
          class="input search"
          placeholder={prefs.t("models.search")}
          bind:value={query}
        />
      </div>
      <button class="btn btn-icon" onclick={refresh} disabled={loading} title={prefs.t("models.refresh")}>
        <Icon name="refresh" size={15} />
        <span class="btn-lab">{prefs.t("models.refresh")}</span>
      </button>
    </div>
  </header>

  {#if hw && !prefs.isBeginner}
    <div class="hwbar">
      {#if hw.gpu}
        <span class="hw-item">
          <span class="hw-k">GPU</span>
          {hw.gpu.name} · {formatBytes(hw.gpu.vram_bytes)}
        </span>
      {:else}
        <span class="hw-item hw-warn"
          ><span class="hw-k">GPU</span> {prefs.t("models.gpu_none")}</span
        >
      {/if}
      <span class="hw-item"
        ><span class="hw-k">RAM</span> {formatBytes(hw.total_ram_bytes)}</span
      >
      <span class="hw-item">
        <span class="hw-k">CPU</span>
        {prefs.t("models.cpu", {
          phys: hw.physical_cores,
          log: hw.logical_cores,
        })}
      </span>
    </div>
  {/if}

  {#if error}
    <div class="glass note bad selectable">
      {prefs.t("models.scan_err", { err: error })}
    </div>
  {:else if loading}
    <div class="muted center load-state">
      <div class="load-ring"></div>
      {prefs.t("models.scanning")}
    </div>
  {:else if models.length === 0}
    <div class="glass empty">
      <div class="empty-visual">
        <div class="empty-orb"></div>
        <Icon name="models" size={28} />
      </div>
      <h3>{prefs.t("models.empty.title")}</h3>
      <p>{prefs.t("models.empty.body")}</p>
      <p class="dim">
        {prefs.isBeginner
          ? prefs.t("models.empty.hint")
          : prefs.t("models.empty.hint_pro")}
      </p>
      {#if oncatalog}
        <button class="btn btn-primary empty-cta" onclick={oncatalog}>
          <Icon name="catalog" size={16} />
          {prefs.t("models.empty.cta")}
        </button>
      {/if}
    </div>
  {:else}
    <div class="layout">
      <div class="list">
        {#each filtered as m (m.path)}
          {@const label = parseLabel(m.name)}
          <button
            type="button"
            class="card {selected === m.path ? 'sel' : ''}"
            onclick={() => select(m)}
            oncontextmenu={(e) => openCtx(e, m)}
          >
            <div class="card-top">
              <span class="name" title={m.name}>{label.title}</span>
              <span class="size mono">{formatBytes(m.size)}</span>
            </div>
            <div class="card-badges">
              {#if label.quant}
                <span class="badge quant">{label.quant}</span>
              {/if}
              {#if meta && selected === m.path && meta.architecture}
                <span class="badge mute">{meta.architecture}</span>
              {/if}
            </div>
            {#if prefs.showAdvanced}
              <span class="file mono" title={m.name}>{m.name}</span>
            {/if}
          </button>
        {/each}
        {#if filtered.length === 0}
          <div class="muted center small">
            {prefs.t("models.none_query", { q: query })}
          </div>
        {/if}
      </div>

      <aside class="detail glass">
        {#if selectedModel && selectedLabel}
          <div class="d-head">
            <h3 class="d-name">{selectedLabel.title}</h3>
            <div class="d-badges">
              {#if selectedLabel.quant}
                <span class="badge quant">{selectedLabel.quant}</span>
              {/if}
              <span class="badge size-b mono">{formatBytes(selectedModel.size)}</span>
              {#if fitKind === "wait"}
                <span class="badge fit wait">{prefs.t("models.fit.wait")}</span>
              {:else if fitKind === "full"}
                <span class="badge fit full">{prefs.t("models.fit.full")}</span>
              {:else if fitKind === "partial"}
                <span class="badge fit partial">{prefs.t("models.fit.partial")}</span>
              {:else if fitKind === "cpu"}
                <span class="badge fit cpu">{prefs.t("models.fit.cpu")}</span>
              {/if}
            </div>
          </div>

          {#if prefs.showAdvanced}
            <div class="meta">
              {#if metaLoading}
                <span class="muted">{prefs.t("models.meta.loading")}</span>
              {:else if meta}
                <div class="meta-grid">
                  {#if meta.architecture}
                    <span class="k">{prefs.t("models.meta.arch")}</span><span
                      class="v">{meta.architecture}</span
                    >
                  {/if}
                  {#if meta.n_layers}
                    <span class="k">{prefs.t("models.meta.layers")}</span><span
                      class="v">{meta.n_layers}</span
                    >
                  {/if}
                  {#if meta.n_head}
                    <span class="k">{prefs.t("models.meta.heads")}</span><span
                      class="v">{meta.n_head}</span
                    >
                  {/if}
                  {#if meta.n_head_kv}
                    <span class="k">{prefs.t("models.meta.kv")}</span><span
                      class="v">{meta.n_head_kv}</span
                    >
                  {/if}
                  {#if meta.n_embd}
                    <span class="k">{prefs.t("models.meta.embd")}</span><span
                      class="v">{meta.n_embd}</span
                    >
                  {/if}
                  {#if meta.ctx_train}
                    <span class="k">{prefs.t("models.meta.ctx")}</span><span
                      class="v">{meta.ctx_train.toLocaleString()}</span
                    >
                  {/if}
                </div>
              {:else}
                <span class="muted">{prefs.t("models.meta.none")}</span>
              {/if}
            </div>
          {:else if prefs.isIntermediate && meta?.architecture}
            <div class="meta soft">
              <span class="muted"
                >{meta.architecture}{#if meta.n_layers}
                  · {meta.n_layers}
                  {prefs.t("models.meta.layers").toLowerCase()}{/if}</span
              >
            </div>
          {/if}

          <div class="reco">
            <div class="reco-top">
              <span class="reco-title">{prefs.t("prof.title")}</span>
              {#if prefs.canDisableAuto}
                <label class="toggle">
                  <input type="checkbox" bind:checked={useAuto} />
                  <span>{prefs.t("models.auto.on")}</span>
                </label>
              {/if}
            </div>

            {#if !prefs.canDisableAuto || useAuto}
              <div class="prof-chips" role="group" aria-label={prefs.t("prof.title")}>
                {#each LAUNCH_PROFILES as p}
                  <button
                    type="button"
                    class="prof-chip {profile === p.id ? 'on' : ''}"
                    onclick={() => (profile = p.id)}
                    title={prefs.t(p.descKey)}
                  >
                    {prefs.t(p.labelKey)}
                  </button>
                {/each}
              </div>
              <p class="prof-desc muted">{prefs.t(activeProfileMeta.descKey)}</p>
            {:else}
              <p class="prof-desc muted">{prefs.t("prof.manual")}</p>
            {/if}

            {#if autoLoading}
              <span class="muted small">{prefs.t("models.auto.loading")}</span>
            {:else if resolvedParams}
              {#if prefs.showAutoDetails}
                <div class="reco-grid">
                  <span class="k">{prefs.t("models.auto.ngl")}</span>
                  <span class="v">
                    {resolvedParams.ngl === 0
                      ? "0"
                      : auto?.full_offload &&
                          profile !== "cpu" &&
                          resolvedParams.ngl >= (auto?.ngl ?? 0)
                        ? prefs.t("models.auto.ngl_all")
                        : resolvedParams.ngl}
                  </span>
                  <span class="k">{prefs.t("models.auto.ctx")}</span>
                  <span class="v">{resolvedParams.ctx.toLocaleString()}</span>
                  {#if prefs.showAdvanced}
                    <span class="k">{prefs.t("models.auto.kv")}</span>
                    <span class="v">{resolvedParams.kv_quant}</span>
                    <span class="k">{prefs.t("models.auto.threads")}</span>
                    <span class="v">{resolvedParams.threads}</span>
                  {/if}
                  {#if auto && auto.est_vram_bytes > 0 && profile === "balanced"}
                    <span class="k">{prefs.t("models.auto.vram")}</span>
                    <span class="v">{formatBytes(auto.est_vram_bytes)}</span>
                  {/if}
                </div>
                {#if auto && profile === "balanced"}
                  <p class="reco-why {auto.full_offload ? 'ok' : 'warn'}">
                    {auto.rationale}
                  </p>
                {/if}
              {:else}
                <p class="reco-why {fitKind === 'full' ? 'ok' : 'warn'}">
                  {#if fitKind === "full"}
                    {prefs.t("models.auto.simple_ok")}
                  {:else if fitKind === "cpu"}
                    {prefs.t("prof.simple_cpu")}
                  {:else}
                    {prefs.t("models.auto.simple_warn")}
                  {/if}
                </p>
              {/if}
            {:else if !autoLoading}
              <span class="muted small">{prefs.t("models.auto.fail")}</span>
            {/if}
          </div>

          <button
            class="btn btn-primary launch"
            disabled={launchDisabled}
            onclick={() => launch(selectedModel)}
          >
            {#if serverState.running}
              {prefs.t("models.already")}
            {:else if serverState.starting}
              {prefs.t("models.launching")}
            {:else if waitingAuto}
              {prefs.t("models.auto.loading")}
            {:else}
              <Icon name="play" size={15} />
              {prefs.t("models.launch")}
            {/if}
          </button>
          {#if prefs.showAdvanced}
            <p class="path-hint selectable" title={selectedModel.path}>
              {selectedModel.path}
            </p>
          {/if}
        {:else}
          <div class="pick-hint">
            <div class="pick-ic"><Icon name="models" size={22} /></div>
            <p class="muted">{prefs.t("models.pick")}</p>
            <p class="dim">{prefs.t("models.pick.sub")}</p>
          </div>
        {/if}
      </aside>
    </div>
  {/if}
</div>

{#if ctx}
  <ContextMenu
    x={ctx.x}
    y={ctx.y}
    items={ctxItems}
    onpick={onCtxPick}
    onclose={() => (ctx = null)}
  />
{/if}

{#if toast}
  <div class="toast" role="status">{toast}</div>
{/if}

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
  .head-actions {
    display: flex;
    gap: 10px;
    align-items: center;
  }
  .search-wrap {
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
  }
  .search {
    width: 240px;
    padding-left: 34px;
  }
  .btn-icon {
    gap: 7px;
  }
  .btn-lab {
    font-size: 13px;
  }

  .hwbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 10px;
    padding: 9px 13px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-m);
    font-family: var(--font-mono);
    letter-spacing: -0.02em;
    font-size: 12px;
    color: var(--text-1);
  }
  .hw-item {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .hw-item + .hw-item {
    padding-left: 10px;
    border-left: 1px solid var(--border);
  }
  .hw-k {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--accent);
    opacity: 0.9;
  }
  .hw-warn {
    color: var(--warn);
  }

  .layout {
    display: grid;
    grid-template-columns: 1fr minmax(300px, 340px);
    gap: 16px;
    flex: 1;
    min-height: 0;
  }
  .list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
    padding-right: 4px;
  }

  .card {
    text-align: left;
    padding: 12px 14px;
    border-radius: var(--radius-m);
    background: var(--surface);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 7px;
    transition:
      background 0.14s ease,
      border-color 0.14s ease,
      box-shadow 0.14s ease,
      transform 0.06s ease;
  }
  .card:hover {
    background: var(--surface-hover);
    border-color: var(--border-strong);
  }
  .card.sel {
    background: var(--accent-soft);
    border-color: var(--accent-line);
    box-shadow: 0 0 0 1px rgba(255, 154, 61, 0.12), 0 8px 24px rgba(0, 0, 0, 0.25);
  }
  .card-top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .name {
    font-weight: 560;
    font-size: 13.5px;
    letter-spacing: -0.01em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .size {
    color: var(--text-2);
    font-size: 11.5px;
    flex: none;
  }
  .card-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .file {
    font-size: 10.5px;
    color: var(--text-2);
    opacity: 0.75;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .badge {
    display: inline-flex;
    align-items: center;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.02em;
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.22);
    color: var(--text-1);
  }
  .badge.quant {
    color: var(--accent-hover);
    border-color: var(--accent-line);
    background: var(--accent-soft);
    font-family: var(--font-mono);
    letter-spacing: -0.02em;
  }
  .badge.mute {
    font-weight: 500;
    text-transform: lowercase;
  }
  .badge.size-b {
    font-weight: 500;
    color: var(--text-0);
  }
  .badge.fit.full {
    color: var(--ok);
    border-color: rgba(75, 208, 127, 0.35);
    background: rgba(75, 208, 127, 0.1);
  }
  .badge.fit.partial {
    color: var(--warn);
    border-color: rgba(255, 194, 71, 0.35);
    background: rgba(255, 194, 71, 0.1);
  }
  .badge.fit.cpu {
    color: var(--text-1);
  }
  .badge.fit.wait {
    color: var(--text-2);
  }

  .detail {
    padding: 18px;
    display: flex;
    flex-direction: column;
    gap: 14px;
    align-self: stretch;
    min-height: 0;
    overflow-y: auto;
  }
  .d-head {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .d-name {
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.02em;
    word-break: break-word;
    line-height: 1.3;
  }
  .d-badges {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .meta.soft {
    font-size: 12.5px;
  }
  .meta-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 7px 14px;
    font-size: 13px;
  }
  .meta-grid .k {
    color: var(--text-2);
  }
  .meta-grid .v {
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }
  .launch {
    margin-top: 2px;
    width: 100%;
  }

  .reco {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 13px;
    background: linear-gradient(
      165deg,
      rgba(255, 154, 61, 0.1),
      rgba(255, 154, 61, 0.04)
    );
    border: 1px solid var(--accent-line);
    border-radius: var(--radius-m);
  }
  .reco-top {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  .reco-title {
    font-size: 13px;
    font-weight: 600;
  }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-1);
    cursor: pointer;
  }
  .toggle input {
    accent-color: var(--accent);
  }
  .prof-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .prof-chip {
    padding: 6px 11px;
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
  .prof-chip:hover {
    background: var(--surface-hover);
    color: var(--text-0);
  }
  .prof-chip.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-hover);
  }
  .prof-desc {
    margin: 0;
    font-size: 12px;
    line-height: 1.4;
  }
  .reco-grid {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 6px 14px;
    font-size: 13px;
  }
  .reco-grid .k {
    color: var(--text-2);
  }
  .reco-grid .v {
    text-align: right;
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    font-weight: 500;
    letter-spacing: -0.02em;
    color: var(--accent-hover);
  }
  .reco-why {
    margin: 0;
    font-size: 11.5px;
    line-height: 1.45;
  }
  .reco-why.ok {
    color: var(--ok);
  }
  .reco-why.warn {
    color: var(--warn);
  }

  .path-hint {
    font-size: 11px;
    color: var(--text-2);
    margin: 0;
    font-family: var(--font-mono);
    letter-spacing: -0.02em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .pick-hint {
    text-align: center;
    padding: 36px 12px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
  }
  .pick-ic {
    width: 48px;
    height: 48px;
    border-radius: 14px;
    display: grid;
    place-items: center;
    color: var(--text-2);
    background: var(--surface);
    border: 1px solid var(--border);
    margin-bottom: 6px;
  }
  .pick-hint p {
    margin: 0;
  }
  .pick-hint .dim {
    font-size: 12.5px;
    color: var(--text-2);
  }

  .note {
    padding: 16px 18px;
  }
  .bad {
    color: var(--danger);
  }
  .center {
    text-align: center;
    padding: 40px 0;
  }
  .load-state {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }
  .load-ring {
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    animation: spin 0.7s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .small {
    font-size: 13px;
  }
  .muted {
    color: var(--text-2);
  }

  .empty {
    padding: 48px 32px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    flex: 1;
    justify-content: center;
    min-height: 280px;
  }
  .empty-visual {
    position: relative;
    width: 72px;
    height: 72px;
    display: grid;
    place-items: center;
    margin-bottom: 8px;
    color: var(--accent-hover);
  }
  .empty-orb {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: radial-gradient(
      circle at 35% 30%,
      var(--accent-glow),
      transparent 68%
    );
    border: 1px solid var(--accent-line);
    opacity: 0.9;
  }
  .empty-visual :global(.icon) {
    position: relative;
    z-index: 1;
  }
  .empty h3 {
    font-size: 17px;
  }
  .empty p {
    margin: 0;
    color: var(--text-1);
    max-width: 360px;
  }
  .empty .dim {
    color: var(--text-2);
    font-size: 13px;
  }
  .empty-cta {
    margin-top: 12px;
  }

  .mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
  }

  .toast {
    position: fixed;
    left: 50%;
    bottom: 28px;
    transform: translateX(-50%);
    z-index: 210;
    padding: 9px 16px;
    border-radius: 999px;
    font-size: 12.5px;
    font-weight: 500;
    color: var(--text-0);
    background: #1c1c21;
    border: 1px solid var(--border-strong);
    box-shadow: var(--shadow-lift);
    animation: toast-in 0.16s ease both;
    pointer-events: none;
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(-50%) translateY(6px);
    }
    to {
      opacity: 1;
      transform: translateX(-50%);
    }
  }

  @media (max-width: 980px) {
    .layout {
      grid-template-columns: 1fr;
    }
    .detail {
      position: relative;
    }
    .btn-lab {
      display: none;
    }
  }
</style>
