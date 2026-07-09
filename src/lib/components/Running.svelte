<script lang="ts">
  import { serverState } from "$lib/server.svelte";

  let logEl = $state<HTMLDivElement | null>(null);
  let autoScroll = $state(true);

  // Автоскролл к низу при новых строках, если пользователь не проскроллил вверх.
  $effect(() => {
    serverState.log.length; // зависимость
    if (autoScroll && logEl) {
      logEl.scrollTop = logEl.scrollHeight;
    }
  });

  function onScroll() {
    if (!logEl) return;
    const nearBottom =
      logEl.scrollHeight - logEl.scrollTop - logEl.clientHeight < 40;
    autoScroll = nearBottom;
  }

  const statusLabel = $derived(
    serverState.starting
      ? "Запуск…"
      : serverState.ready
        ? "Работает"
        : serverState.running
          ? "Загрузка модели…"
          : "Остановлен",
  );
</script>

<div class="page">
  <header class="head">
    <div class="title-row">
      <span
        class="dot {serverState.ready
          ? 'on'
          : serverState.running
            ? 'load'
            : 'off'}"
      ></span>
      <div>
        <h2>{serverState.modelName ?? "Сервер"}</h2>
        <p class="sub">
          {statusLabel}{#if serverState.port && serverState.running}
            · 127.0.0.1:{serverState.port}{/if}
        </p>
      </div>
    </div>
    <div class="actions">
      {#if serverState.ready}
        <button class="btn btn-primary" onclick={() => serverState.openWebUi()}>
          ⧉ Открыть Web-UI
        </button>
      {/if}
      {#if serverState.running}
        <button
          class="btn stop"
          onclick={() => serverState.stop()}
          disabled={serverState.stopping}
        >
          {serverState.stopping ? "Останавливаю…" : "■ Стоп"}
        </button>
      {/if}
    </div>
  </header>

  {#if serverState.error}
    <div class="glass err">
      <span>{serverState.error}</span>
      <button class="x" onclick={() => serverState.clearError()} aria-label="Скрыть">✕</button>
    </div>
  {/if}

  {#if !serverState.running && serverState.log.length === 0}
    <div class="glass empty">
      <div class="empty-orb"></div>
      <h3>Сервер не запущен</h3>
      <p>Выбери модель на вкладке «Модели» и нажми «Запустить».</p>
    </div>
  {:else}
    <div class="glass console" bind:this={logEl} onscroll={onScroll}>
      {#each serverState.log as line, i (i)}
        <div class="line">{line}</div>
      {/each}
      {#if serverState.log.length === 0}
        <div class="line dim">Ожидание вывода…</div>
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
        ↓ К последним строкам
      </button>
    {/if}
  {/if}
</div>

<style>
  .page { display: flex; flex-direction: column; gap: 16px; height: 100%; min-height: 0; }
  .head {
    display: flex; justify-content: space-between; align-items: center; gap: 16px;
  }
  .title-row { display: flex; align-items: center; gap: 13px; }
  h2 { font-size: 18px; word-break: break-word; }
  .sub { margin: 3px 0 0; color: var(--text-2); font-size: 12.5px; font-family: var(--font-mono); font-variant-numeric: tabular-nums; letter-spacing: -.02em; }
  .dot {
    width: 11px; height: 11px; border-radius: 50%; flex: none;
    box-shadow: 0 0 0 4px rgba(255,255,255,.04);
  }
  .dot.on { background: var(--ok); box-shadow: 0 0 12px var(--ok), 0 0 0 4px rgba(56,211,159,.15); }
  .dot.load { background: var(--warn); animation: blink 1s infinite; }
  .dot.off { background: var(--text-2); }
  @keyframes blink { 50% { opacity: .35; } }

  .actions { display: flex; gap: 10px; }
  .stop { color: var(--danger); border-color: rgba(255,107,107,.35); }
  .stop:hover { background: rgba(255,107,107,.12); border-color: var(--danger); }

  .err {
    padding: 12px 16px; display: flex; justify-content: space-between; align-items: center;
    gap: 12px; color: var(--danger); border-color: rgba(255,107,107,.3);
  }
  .err .x { color: var(--danger); padding: 2px 6px; }

  .console {
    flex: 1; min-height: 0; overflow-y: auto;
    padding: 14px 16px;
    font-family: var(--font-mono);
    font-size: 12px; line-height: 1.55; letter-spacing: -.02em;
    background: rgba(0,0,0,.42);
    border-radius: var(--radius-m);
  }
  .line { white-space: pre-wrap; word-break: break-word; color: var(--text-1); }
  .line.dim { color: var(--text-2); }

  .scroll-btn { align-self: center; margin-top: -6px; }

  .empty {
    padding: 44px; text-align: center;
    display: flex; flex-direction: column; align-items: center; gap: 8px;
  }
  .empty-orb {
    width: 56px; height: 56px; border-radius: 50%;
    background: radial-gradient(circle at 32% 30%, var(--accent-glow), transparent 70%);
    margin-bottom: 8px;
  }
  .empty h3 { font-size: 17px; }
  .empty p { margin: 0; color: var(--text-1); }
</style>
