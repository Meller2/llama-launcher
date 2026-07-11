<script lang="ts">
  /** Компактные stroke-иконки (1.5px), единый стиль навигации / chrome. */
  let {
    name,
    size = 18,
    stroke = 1.5,
  }: {
    name:
      | "models"
      | "catalog"
      | "running"
      | "settings"
      | "minimize"
      | "maximize"
      | "restore"
      | "close"
      | "refresh"
      | "play"
      | "search"
      | "stop"
      | "external"
      | "copy"
      | "send"
      | "trash";
    size?: number;
    stroke?: number;
  } = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if name === "models"}
    <path d="M4 6.5h16M4 12h16M4 17.5h10" />
    <circle cx="18.5" cy="17.5" r="1.5" fill="currentColor" stroke="none" />
  {:else if name === "catalog"}
    <circle cx="11" cy="11" r="6.5" />
    <path d="M16.2 16.2 20 20" />
  {:else if name === "running"}
    <circle cx="12" cy="12" r="8" />
    <path d="M10 9.5v5l5-2.5-5-2.5z" fill="currentColor" stroke="none" />
  {:else if name === "settings"}
    <circle cx="12" cy="12" r="3" />
    <path
      d="M12 3.5v2.2M12 18.3v2.2M4.9 6.4l1.6 1.5M17.5 16.1l1.6 1.5M3.5 12h2.2M18.3 12h2.2M4.9 17.6l1.6-1.5M17.5 7.9l1.6-1.5"
    />
  {:else if name === "minimize"}
    <path d="M6 12.5h12" />
  {:else if name === "maximize"}
    <rect x="6" y="6" width="12" height="12" rx="1.5" />
  {:else if name === "restore"}
    <rect x="8" y="8" width="10" height="10" rx="1.2" />
    <path d="M8 10H7a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h2a1 1 0 0 1 1 1v1" />
  {:else if name === "close"}
    <path d="M7 7l10 10M17 7 7 17" />
  {:else if name === "refresh"}
    <path d="M4.5 12a7.5 7.5 0 0 1 13-4.5M19.5 12a7.5 7.5 0 0 1-13 4.5" />
    <path d="M17.5 3.5v4.2h-4.2M6.5 20.5v-4.2h4.2" />
  {:else if name === "play"}
    <path d="M9 7.5v9l8-4.5-8-4.5z" fill="currentColor" stroke="none" />
  {:else if name === "search"}
    <circle cx="11" cy="11" r="6.5" />
    <path d="M16.2 16.2 20 20" />
  {:else if name === "stop"}
    <rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" stroke="none" />
  {:else if name === "external"}
    <path d="M14 5h5v5" />
    <path d="M10 14 19 5" />
    <path d="M19 13v5a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5" />
  {:else if name === "copy"}
    <rect x="9" y="9" width="10" height="10" rx="1.5" />
    <path d="M7 15H6a1.5 1.5 0 0 1-1.5-1.5v-8A1.5 1.5 0 0 1 6 4h8A1.5 1.5 0 0 1 15.5 5.5V7" />
  {:else if name === "send"}
    <path d="M5 12h12" />
    <path d="m12 6 6 6-6 6" />
  {:else if name === "trash"}
    <path d="M5 8h14" />
    <path d="M9 8V6a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
    <path d="M7 8l1 12h8l1-12" />
  {/if}
</svg>

<style>
  .icon {
    display: block;
    flex-shrink: 0;
  }
</style>
