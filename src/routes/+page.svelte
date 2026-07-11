<script lang="ts">
  import { loadSettings, needsSetup, type Settings } from "$lib/api";
  import { serverState } from "$lib/server.svelte";
  import { prefs } from "$lib/prefs.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import LocalModels from "$lib/components/LocalModels.svelte";
  import Catalog from "$lib/components/Catalog.svelte";
  import SettingsView from "$lib/components/Settings.svelte";
  import Running from "$lib/components/Running.svelte";
  import Titlebar from "$lib/components/Titlebar.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let settings = $state<Settings | null>(null);
  let loading = $state(true);
  let bootError = $state<string | null>(null);
  let tab = $state<"models" | "catalog" | "running" | "settings">("models");

  async function init() {
    loading = true;
    bootError = null;
    try {
      settings = await loadSettings();
      prefs.apply(settings);
      await serverState.init();
    } catch (e) {
      bootError = String(e);
      settings = null;
    } finally {
      loading = false;
    }
  }
  init();

  function onOnboarded(s: Settings) {
    settings = s;
    prefs.apply(s);
    if (prefs.isBeginner) tab = "catalog";
  }
  function onSettingsChanged(s: Settings) {
    settings = s;
    prefs.apply(s);
  }
  function goRunning() {
    tab = "running";
  }
  function goCatalog() {
    tab = "catalog";
  }
  function goModels() {
    tab = "models";
  }

  const showSetup = $derived(settings !== null && needsSetup(settings));

  // Основные вкладки сверху; настройки — внизу сайдбара (как в большинстве desktop-UI).
  const mainTabs = $derived([
    {
      id: "models" as const,
      label: prefs.t("app.tab.models"),
      icon: "models" as const,
    },
    {
      id: "catalog" as const,
      label: prefs.t("app.tab.catalog"),
      icon: "catalog" as const,
    },
    {
      id: "running" as const,
      label: prefs.t("app.tab.running"),
      icon: "running" as const,
    },
  ]);
  const settingsTab = $derived({
    id: "settings" as const,
    label: prefs.t("app.tab.settings"),
    icon: "settings" as const,
  });
</script>

<div class="app-frame">
  <Titlebar />

  {#if loading}
    <div class="boot">
      <div class="boot-card">
        <img class="logo-mark boot-mark" src="/logo1.png" alt="LlamaLauncher" />
        <p class="boot-label">{prefs.t("app.name")}</p>
      </div>
    </div>
  {:else if bootError}
    <div class="boot boot-err">
      <img class="logo-mark sm" src="/logo1.png" alt="" />
      <h2>{prefs.t("app.boot_err.title")}</h2>
      <p class="boot-msg selectable">{bootError}</p>
      <button class="btn btn-primary" onclick={init}>{prefs.t("app.boot_err.retry")}</button>
    </div>
  {:else if settings && showSetup}
    <div class="onboard-wrap">
      <Onboarding {settings} oncomplete={onOnboarded} />
    </div>
  {:else if settings}
    <div class="shell">
      <nav class="sidebar">
        <div class="nav">
          {#each mainTabs as t (t.id)}
            <button
              type="button"
              class="nav-item {tab === t.id ? 'active' : ''}"
              onclick={() => (tab = t.id)}
              aria-current={tab === t.id ? "page" : undefined}
            >
              <span class="ic"><Icon name={t.icon} size={17} /></span>
              <span class="lab">{t.label}</span>
              {#if t.id === "running" && serverState.running}
                <span
                  class="run-dot {serverState.ready ? 'ready' : 'loading'}"
                  aria-hidden="true"
                ></span>
              {/if}
            </button>
          {/each}
        </div>
        <div class="side-foot">
          <button
            type="button"
            class="nav-item settings-item {tab === settingsTab.id ? 'active' : ''}"
            onclick={() => (tab = settingsTab.id)}
            aria-current={tab === settingsTab.id ? "page" : undefined}
          >
            <span class="ic"><Icon name={settingsTab.icon} size={17} /></span>
            <span class="lab">{settingsTab.label}</span>
          </button>
          <span class="ver mono">local · llama.cpp</span>
        </div>
      </nav>

      <main class="content">
        {#if tab === "models"}
          <LocalModels {settings} onlaunch={goRunning} oncatalog={goCatalog} />
        {:else if tab === "settings"}
          <SettingsView {settings} onchange={onSettingsChanged} />
        {:else if tab === "catalog"}
          <Catalog {settings} />
        {:else if tab === "running"}
          <Running onmodels={goModels} />
        {/if}
      </main>
    </div>
  {/if}
</div>

<style>
  .boot {
    flex: 1;
    display: grid;
    place-items: center;
    min-height: 0;
  }
  .boot-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
  }
  .boot-label {
    margin: 0;
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-2);
  }
  .boot-err {
    gap: 12px;
    padding: 32px;
    text-align: center;
    max-width: 420px;
    margin: 0 auto;
  }
  .boot-err h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 18px;
    font-weight: 600;
  }
  .boot-msg {
    margin: 0;
    color: var(--text-1);
    font-size: 13px;
    line-height: 1.45;
    word-break: break-word;
  }
  .logo-mark {
    display: block;
    object-fit: contain;
    -webkit-user-drag: none;
    user-select: none;
  }
  .boot-mark {
    width: 96px;
    height: 96px;
    filter: drop-shadow(0 8px 28px var(--accent-glow));
    animation: float-in 0.6s ease both;
  }
  .logo-mark.sm {
    width: 48px;
    height: 48px;
  }
  @keyframes float-in {
    from {
      opacity: 0;
      transform: translateY(8px) scale(0.96);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .onboard-wrap {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .shell {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: var(--sidebar-w) 1fr;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 14px 12px 12px;
    background: linear-gradient(180deg, rgba(0, 0, 0, 0.22), rgba(0, 0, 0, 0.12));
    border-right: 1px solid var(--border);
  }
  .nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
  }
  .nav-item {
    position: relative;
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
    color: var(--text-1);
    font-size: 13.5px;
    font-weight: 500;
    transition: background 0.14s ease, color 0.14s ease, box-shadow 0.14s ease;
  }
  .nav-item:hover {
    background: var(--surface-hover);
    color: var(--text-0);
  }
  .nav-item.active {
    background: var(--accent-soft);
    color: var(--accent-hover);
    box-shadow: inset 0 0 0 1px rgba(255, 154, 61, 0.12);
  }
  .nav-item.active::before {
    content: "";
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%);
    width: 3px;
    height: 16px;
    border-radius: 0 3px 3px 0;
    background: var(--accent);
    box-shadow: 0 0 12px var(--accent-glow);
  }
  .nav-item .ic {
    display: grid;
    place-items: center;
    width: 20px;
    opacity: 0.9;
  }
  .nav-item .lab {
    flex: 1;
    text-align: left;
  }
  .run-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: none;
  }
  .run-dot.ready {
    background: var(--ok);
    box-shadow: 0 0 8px var(--ok);
  }
  .run-dot.loading {
    background: var(--warn);
    animation: blink 1s infinite;
  }
  @keyframes blink {
    50% {
      opacity: 0.3;
    }
  }

  .side-foot {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 0 4px;
    border-top: 1px solid var(--border);
    margin-top: auto;
  }
  .settings-item {
    width: 100%;
  }
  /* У нижней «Настройки» индикатор слева не обязателен — чуть тише active. */
  .settings-item.active::before {
    height: 14px;
  }
  .ver {
    font-size: 11px;
    color: var(--text-2);
    opacity: 0.85;
    padding: 0 12px 2px;
  }

  .content {
    padding: 22px 28px 24px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: radial-gradient(
        800px 400px at 100% 0%,
        rgba(255, 138, 40, 0.04),
        transparent 55%
      ),
      transparent;
  }
</style>
