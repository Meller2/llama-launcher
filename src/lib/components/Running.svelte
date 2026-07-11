<script lang="ts">
  import { serverState } from "$lib/server.svelte";
  import { prefs } from "$lib/prefs.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let {
    onmodels,
  }: {
    onmodels?: () => void;
  } = $props();

  let logEl = $state<HTMLDivElement | null>(null);
  let autoScroll = $state(true);
  let showLog = $state(false);
  let toast = $state<string | null>(null);
  let toastTimer: ReturnType<typeof setTimeout> | null = null;

  $effect(() => {
    showLog = prefs.logExpandedByDefault;
  });

  $effect(() => {
    serverState.log.length;
    if (autoScroll && logEl && showLog) {
      logEl.scrollTop = logEl.scrollHeight;
    }
  });

  function onScroll() {
    if (!logEl) return;
    const nearBottom =
      logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
    autoScroll = nearBottom;
  }

  function parseLabel(filename: string): string {
    const raw = filename.replace(/\.gguf$/i, "");
    const re =
      /(?:^|[-_.])((?:IQ|Q)\d+(?:_[A-Z0-9]+)?|F16|F32|BF16)(?:$|[-_.])/i;
    const m = raw.match(re);
    let title = raw;
    if (m) {
      title = raw
        .replace(new RegExp(`[-_.]?${m[1]}[-_.]?`, "i"), "-")
        .replace(/[-_]+/g, "-")
        .replace(/^-|-$/g, "");
    }
    return title.replace(/[-_]+/g, " ").replace(/\s+/g, " ").trim() || raw;
  }

  const displayName = $derived(
    serverState.modelName
      ? parseLabel(serverState.modelName)
      : prefs.t("run.title"),
  );

  const statusLabel = $derived(
    serverState.starting
      ? prefs.t("run.starting")
      : serverState.ready
        ? prefs.t("run.ready")
        : serverState.running
          ? prefs.t("run.loading")
          : prefs.t("run.stopped"),
  );

  const statusKind = $derived(
    serverState.ready
      ? "on"
      : serverState.starting || serverState.running
        ? "load"
        : "off",
  );

  const endpoint = $derived(
    serverState.port ? `http://127.0.0.1:${serverState.port}` : null,
  );

  function showToast(msg: string) {
    toast = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => (toast = null), 1600);
  }

  async function copyText(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      showToast(prefs.t("run.copied"));
    } catch {
      /* */
    }
  }

  async function copyLog() {
    await copyText(serverState.log.join("\n"));
  }

  async function copyAddr() {
    if (endpoint) await copyText(endpoint);
  }
</script>

<div class="page">
  <header class="head">
    <div class="title-row">
      <span class="dot {statusKind}" aria-hidden="true"></span>
      <div class="titles">
        <h2 title={serverState.modelName ?? undefined}>{displayName}</h2>
        <p class="sub">
          <span class="status-pill {statusKind}">{statusLabel}</span>
          {#if serverState.modelName && serverState.modelName !== displayName}
            <span class="file mono" title={serverState.modelName}
              >{serverState.modelName}</span
            >
          {/if}
        </p>
      </div>
    </div>
    <div class="actions">
      {#if serverState.ready}
        <button class="btn btn-primary" onclick={() => serverState.openWebUi()}>
          <Icon name="external" size={15} />
          {prefs.t("run.open")}
        </button>
      {/if}
      {#if serverState.running}
        <button
          class="btn stop"
          onclick={() => serverState.stop()}
          disabled={serverState.stopping}
        >
          <Icon name="stop" size={13} />
          {serverState.stopping ? prefs.t("run.stopping") : prefs.t("run.stop")}
        </button>
      {/if}
    </div>
  </header>

  {#if serverState.error}
    <div class="glass err">
      <span class="selectable">{serverState.error}</span>
      <button class="x" onclick={() => serverState.clearError()} aria-label="dismiss"
        >✕</button
      >
    </div>
  {/if}

  {#if !serverState.running && serverState.log.length === 0}
    <div class="glass empty">
      <div class="empty-visual">
        <div class="empty-orb"></div>
        <Icon name="running" size={26} />
      </div>
      <h3>{prefs.t("run.empty.title")}</h3>
      <p>{prefs.t("run.empty.body")}</p>
      {#if onmodels}
        <button class="btn btn-primary empty-cta" onclick={onmodels}>
          <Icon name="models" size={15} />
          {prefs.t("run.empty.cta")}
        </button>
      {/if}
    </div>
  {:else}
    {#if serverState.running}
      <div class="meta-row">
        {#if endpoint}
          <div class="meta-card glass">
            <span class="mk">{prefs.t("run.endpoint")}</span>
            <button
              type="button"
              class="mv mono selectable"
              onclick={copyAddr}
              title={prefs.t("run.copy_addr")}
            >
              {endpoint}
            </button>
          </div>
        {/if}
        {#if serverState.port}
          <div class="meta-card glass">
            <span class="mk">{prefs.t("run.port")}</span>
            <span class="mv mono">{serverState.port}</span>
          </div>
        {/if}
        <div class="meta-card glass">
          <span class="mk">Log</span>
          <span class="mv mono"
            >{prefs.t("run.lines", { n: serverState.log.length })}</span
          >
        </div>
      </div>
    {/if}

    {#if serverState.running && !serverState.ready && !showLog && !prefs.isExpert}
      <div class="glass loading-card">
        <div class="load-ring"></div>
        <p>{prefs.t("run.loading")}</p>
        <p class="dim">{prefs.t("run.log_show")} →</p>
      </div>
    {/if}

    <div class="log-toolbar">
      {#if !prefs.isExpert}
        <button class="btn log-toggle" onclick={() => (showLog = !showLog)}>
          {showLog ? prefs.t("run.log_hide") : prefs.t("run.log_show")}
        </button>
      {/if}
      {#if (showLog || prefs.isExpert) && serverState.log.length > 0}
        <button class="btn log-toggle" onclick={copyLog} title={prefs.t("run.copy_log")}>
          <Icon name="copy" size={14} />
          {prefs.t("run.copy_log")}
        </button>
      {/if}
    </div>

    {#if showLog || prefs.isExpert}
      <div class="glass console selectable" bind:this={logEl} onscroll={onScroll}>
        {#each serverState.log as line, i (i)}
          <div class="line">{line}</div>
        {/each}
        {#if serverState.log.length === 0}
          <div class="line dim">{prefs.t("run.log_wait")}</div>
        {/if}
      </div>
      {#if !autoScroll}
        <button
          class="btn scroll-btn"
          onclick={() => {
            autoScroll = true;
            if (logEl) logEl.scrollTop = logEl.scrollHeight;
          }}
        >
          ↓ {prefs.t("run.scroll")}
        </button>
      {/if}
    {/if}
  {/if}
</div>

{#if toast}
  <div class="toast" role="status">{toast}</div>
{/if}

<style>
  .page {
    display: flex;
    flex-direction: column;
    gap: 14px;
    height: 100%;
    min-height: 0;
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
  }
  .title-row {
    display: flex;
    align-items: flex-start;
    gap: 13px;
    min-width: 0;
  }
  .titles {
    min-width: 0;
  }
  h2 {
    font-size: 18px;
    letter-spacing: -0.02em;
    word-break: break-word;
  }
  .sub {
    margin: 6px 0 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .status-pill {
    font-size: 11.5px;
    font-weight: 600;
    padding: 2px 9px;
    border-radius: 999px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text-1);
  }
  .status-pill.on {
    color: var(--ok);
    border-color: rgba(75, 208, 127, 0.35);
    background: rgba(75, 208, 127, 0.1);
  }
  .status-pill.load {
    color: var(--warn);
    border-color: rgba(255, 194, 71, 0.35);
    background: rgba(255, 194, 71, 0.1);
  }
  .file {
    font-size: 11.5px;
    color: var(--text-2);
    max-width: 280px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dot {
    width: 11px;
    height: 11px;
    border-radius: 50%;
    flex: none;
    margin-top: 6px;
    box-shadow: 0 0 0 4px rgba(255, 255, 255, 0.04);
  }
  .dot.on {
    background: var(--ok);
    box-shadow:
      0 0 12px var(--ok),
      0 0 0 4px rgba(56, 211, 159, 0.15);
  }
  .dot.load {
    background: var(--warn);
    animation: blink 1s infinite;
  }
  .dot.off {
    background: var(--text-2);
  }
  @keyframes blink {
    50% {
      opacity: 0.35;
    }
  }

  .actions {
    display: flex;
    gap: 10px;
    flex: none;
  }
  .stop {
    color: var(--danger);
    border-color: var(--danger-line);
  }
  .stop:hover:not(:disabled) {
    background: var(--danger-soft);
    border-color: var(--danger);
  }

  .err {
    padding: 12px 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    color: var(--danger);
    border-color: var(--danger-line);
    background: var(--danger-soft);
  }
  .err .x {
    color: var(--danger);
    padding: 2px 6px;
  }

  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }
  .meta-card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 10px 14px;
    min-width: 120px;
  }
  .mk {
    font-size: 10.5px;
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-2);
  }
  .mv {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-0);
    text-align: left;
    padding: 0;
    background: none;
    border: none;
  }
  button.mv:hover {
    color: var(--accent-hover);
  }

  .log-toolbar {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .log-toggle {
    align-self: flex-start;
  }

  .console {
    flex: 1;
    min-height: 120px;
    overflow-y: auto;
    padding: 14px 16px;
    font-family: var(--font-mono);
    font-size: 12px;
    line-height: 1.55;
    letter-spacing: -0.02em;
    background: rgba(0, 0, 0, 0.45);
    border-radius: var(--radius-m);
    border-color: rgba(255, 244, 230, 0.08);
  }
  .line {
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-1);
  }
  .line.dim {
    color: var(--text-2);
  }

  .scroll-btn {
    align-self: center;
    margin-top: -4px;
  }

  .loading-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    padding: 36px 20px;
    color: var(--text-1);
  }
  .loading-card p {
    margin: 0;
  }
  .loading-card .dim {
    font-size: 12.5px;
    color: var(--text-2);
  }
  .load-ring {
    width: 32px;
    height: 32px;
    border-radius: 50%;
    border: 2px solid var(--border-strong);
    border-top-color: var(--accent);
    animation: spin 0.75s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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
    min-height: 260px;
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
</style>
