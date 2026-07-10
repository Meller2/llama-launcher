<script lang="ts">
  import { loadSettings, needsSetup, type Settings } from "$lib/api";
  import { serverState } from "$lib/server.svelte";
  import { prefs } from "$lib/prefs.svelte";
  import Onboarding from "$lib/components/Onboarding.svelte";
  import LocalModels from "$lib/components/LocalModels.svelte";
  import Catalog from "$lib/components/Catalog.svelte";
  import SettingsView from "$lib/components/Settings.svelte";
  import Running from "$lib/components/Running.svelte";

  let settings = $state<Settings | null>(null);
  let loading = $state(true);
  let tab = $state<"models" | "catalog" | "running" | "settings">("models");

  async function init() {
    settings = await loadSettings();
    prefs.apply(settings);
    loading = false;
    await serverState.init();
  }
  init();

  function onOnboarded(s: Settings) {
    settings = s;
    prefs.apply(s);
    // Новичку удобнее сразу в каталог.
    if (prefs.isBeginner) tab = "catalog";
  }
  function onSettingsChanged(s: Settings) {
    settings = s;
    prefs.apply(s);
  }
  function goRunning() {
    tab = "running";
  }

  const showSetup = $derived(settings !== null && needsSetup(settings));

  const tabs = $derived([
    { id: "models" as const, label: prefs.t("app.tab.models"), icon: "▤" },
    { id: "catalog" as const, label: prefs.t("app.tab.catalog"), icon: "⌕" },
    { id: "running" as const, label: prefs.t("app.tab.running"), icon: "◉" },
    { id: "settings" as const, label: prefs.t("app.tab.settings"), icon: "⚙" },
  ]);
</script>

{#if loading}
  <div class="boot">
    <img class="logo-mark boot-mark" src="/logo1.png" alt="LlamaLauncher" />
  </div>
{:else if settings && showSetup}
  <Onboarding {settings} oncomplete={onOnboarded} />
{:else if settings}
  <div class="shell">
    <nav class="sidebar">
      <div class="brand">
        <img class="logo-mark sm" src="/logo1.png" alt="" />
        <span class="brand-name">{prefs.t("app.name")}</span>
      </div>
      <div class="nav">
        {#each tabs as t}
          <button
            class="nav-item {tab === t.id ? 'active' : ''}"
            onclick={() => (tab = t.id)}
          >
            <span class="ic">{t.icon}</span>
            <span>{t.label}</span>
            {#if t.id === "running" && serverState.running}
              <span class="run-dot {serverState.ready ? 'ready' : 'loading'}"></span>
            {/if}
          </button>
        {/each}
      </div>
    </nav>

    <main class="content">
      {#if tab === "models"}
        <LocalModels {settings} onlaunch={goRunning} />
      {:else if tab === "settings"}
        <SettingsView {settings} onchange={onSettingsChanged} />
      {:else if tab === "catalog"}
        <Catalog {settings} />
      {:else if tab === "running"}
        <Running />
      {/if}
    </main>
  </div>
{/if}

<style>
  .boot {
    height: 100vh; display: grid; place-items: center;
  }
  .logo-mark {
    display: block;
    object-fit: contain;
    -webkit-user-drag: none;
    user-select: none;
  }
  .boot-mark {
    width: 132px; height: 132px;
    animation: pulse 1.6s ease-in-out infinite;
  }
  @keyframes pulse {
    0%, 100% { transform: scale(1);    opacity: .9;  }
    50%      { transform: scale(1.05); opacity: 1;   }
  }

  .shell {
    height: 100vh;
    display: grid;
    grid-template-columns: 216px 1fr;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    gap: 24px;
    padding: 20px 14px;
    background: rgba(0, 0, 0, 0.28);
    border-right: 1px solid var(--border);
  }
  .brand { display: flex; align-items: center; gap: 9px; padding: 2px 6px 0; }
  .logo-mark.sm { width: 38px; height: 38px; margin: -4px -2px -4px -4px; }
  .brand-name {
    font-family: var(--font-display);
    font-weight: 600; font-size: 15.5px; letter-spacing: -0.02em;
  }
  .nav { display: flex; flex-direction: column; gap: 3px; }
  .nav-item {
    position: relative;
    display: flex; align-items: center; gap: 12px;
    padding: 10px 12px;
    border-radius: var(--radius-m);
    color: var(--text-1);
    font-size: 13.5px; font-weight: 500;
    transition: background .14s, color .14s;
  }
  .nav-item:hover { background: var(--surface-hover); color: var(--text-0); }
  .nav-item.active {
    background: var(--accent-soft);
    color: var(--accent-hover);
  }
  .nav-item.active::before {
    content: "";
    position: absolute; left: 0; top: 50%; transform: translateY(-50%);
    width: 3px; height: 17px; border-radius: 0 3px 3px 0;
    background: var(--accent);
    box-shadow: 0 0 10px var(--accent-glow);
  }
  .nav-item .ic { width: 18px; text-align: center; opacity: .85; font-size: 13px; }
  .run-dot {
    width: 7px; height: 7px; border-radius: 50%; margin-left: auto;
  }
  .run-dot.ready { background: var(--ok); box-shadow: 0 0 8px var(--ok); }
  .run-dot.loading { background: var(--warn); animation: blink 1s infinite; }
  @keyframes blink { 50% { opacity: .3; } }

  .content {
    padding: 26px 30px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
</style>
