<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import {
    chatStream,
    chatCancel,
    type ChatMessage,
    type ChatDelta,
  } from "$lib/api";
  import { prefs } from "$lib/prefs.svelte";
  import Icon from "$lib/components/Icon.svelte";

  let {
    port,
    ready,
    modelKey,
  }: {
    port: number | null;
    ready: boolean;
    /** Смена модели/порта → очистить историю. */
    modelKey: string;
  } = $props();

  type UiMsg = {
    id: number;
    role: "user" | "assistant";
    content: string;
  };

  let messages = $state<UiMsg[]>([]);
  let input = $state("");
  let sending = $state(false);
  let error = $state<string | null>(null);
  let listEl = $state<HTMLDivElement | null>(null);
  let nextId = 1;
  let lastModelKey = "";

  $effect(() => {
    if (modelKey && modelKey !== lastModelKey) {
      lastModelKey = modelKey;
      messages = [];
      error = null;
      input = "";
      if (sending) {
        chatCancel().catch(() => {});
        sending = false;
      }
    }
  });

  $effect(() => {
    messages.length;
    if (listEl) {
      listEl.scrollTop = listEl.scrollHeight;
    }
  });

  let unlisten: UnlistenFn | null = null;
  $effect(() => {
    listen<ChatDelta>("chat-delta", (e) => {
      const p = e.payload;
      if (p.error) {
        error = prefs.t("chat.error", { err: p.error });
      }
      if (p.delta) {
        const last = messages[messages.length - 1];
        if (last && last.role === "assistant") {
          // immutable update for Svelte 5
          messages = [
            ...messages.slice(0, -1),
            { ...last, content: last.content + p.delta },
          ];
        }
      }
      if (p.done) {
        sending = false;
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  async function send() {
    const text = input.trim();
    if (!text || sending || !ready || !port) return;
    error = null;
    input = "";

    const prev = messages;
    const userMsg: UiMsg = { id: nextId++, role: "user", content: text };
    const asstMsg: UiMsg = { id: nextId++, role: "assistant", content: "" };
    messages = [...prev, userMsg, asstMsg];
    sending = true;

    // История для API = всё до пустого assistant.
    const hist: ChatMessage[] = [...prev, userMsg].map((m) => ({
      role: m.role,
      content: m.content,
    }));

    try {
      await chatStream(port, hist);
    } catch (e) {
      const msg = String(e);
      if (!msg.includes("отменена") && !msg.toLowerCase().includes("cancel")) {
        error = prefs.t("chat.error", { err: msg });
        const last = messages[messages.length - 1];
        if (last?.role === "assistant" && !last.content) {
          messages = messages.slice(0, -1);
        }
      }
      sending = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  async function stop() {
    await chatCancel();
    sending = false;
  }

  function clear() {
    if (sending) stop();
    messages = [];
    error = null;
  }
</script>

<div class="chat">
  <div class="chat-head">
    <span class="chat-title">{prefs.t("chat.title")}</span>
    <div class="chat-actions">
      {#if messages.length > 0}
        <button type="button" class="btn btn-sm" onclick={clear} disabled={sending}>
          <Icon name="trash" size={13} />
          {prefs.t("chat.clear")}
        </button>
      {/if}
    </div>
  </div>

  <div class="msgs selectable" bind:this={listEl}>
    {#if !ready}
      <div class="empty muted">{prefs.t("chat.need_ready")}</div>
    {:else if messages.length === 0}
      <div class="empty muted">{prefs.t("chat.empty")}</div>
    {:else}
      {#each messages as m (m.id)}
        <div class="bubble {m.role}">
          <div class="role">{m.role === "user" ? "You" : "AI"}</div>
          <div class="body">
            {#if m.role === "assistant" && !m.content && sending}
              <span class="thinking">{prefs.t("chat.thinking")}</span>
            {:else}
              {m.content}
            {/if}
          </div>
        </div>
      {/each}
    {/if}
  </div>

  {#if error}
    <div class="chat-err">{error}</div>
  {/if}

  <div class="composer">
    <textarea
      class="input area"
      rows="2"
      placeholder={prefs.t("chat.placeholder")}
      bind:value={input}
      onkeydown={onKey}
      disabled={!ready || sending}
    ></textarea>
    <div class="composer-btns">
      {#if sending}
        <button type="button" class="btn stop" onclick={stop}>
          <Icon name="stop" size={13} />
          {prefs.t("chat.stop")}
        </button>
      {:else}
        <button
          type="button"
          class="btn btn-primary"
          onclick={send}
          disabled={!ready || !input.trim()}
        >
          <Icon name="send" size={15} />
          {prefs.t("chat.send")}
        </button>
      {/if}
    </div>
  </div>
</div>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    gap: 10px;
    flex: 1;
    min-height: 0;
    min-width: 0;
  }
  .chat-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .chat-title {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--text-1);
  }
  .chat-actions {
    display: flex;
    gap: 6px;
  }
  .btn-sm {
    padding: 6px 10px;
    font-size: 12px;
  }

  .msgs {
    flex: 1;
    min-height: 160px;
    overflow-y: auto;
    padding: 12px;
    border-radius: var(--radius-m);
    border: 1px solid var(--border);
    background: rgba(0, 0, 0, 0.28);
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .empty {
    margin: auto;
    text-align: center;
    max-width: 280px;
    font-size: 13px;
    line-height: 1.45;
    padding: 24px 12px;
  }
  .bubble {
    max-width: min(92%, 560px);
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--border);
  }
  .bubble.user {
    align-self: flex-end;
    background: var(--accent-soft);
    border-color: var(--accent-line);
  }
  .bubble.assistant {
    align-self: flex-start;
    background: var(--surface);
  }
  .role {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-2);
    margin-bottom: 4px;
  }
  .bubble.user .role {
    color: var(--accent);
  }
  .body {
    font-size: 13.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-0);
  }
  .thinking {
    color: var(--text-2);
    font-style: italic;
  }

  .chat-err {
    font-size: 12.5px;
    color: var(--danger);
    padding: 8px 10px;
    border-radius: var(--radius-m);
    background: var(--danger-soft);
    border: 1px solid var(--danger-line);
  }

  .composer {
    display: flex;
    gap: 10px;
    align-items: flex-end;
  }
  .area {
    flex: 1;
    resize: none;
    min-height: 52px;
    max-height: 120px;
    line-height: 1.4;
    font-family: var(--font-sans);
  }
  .composer-btns {
    flex: none;
  }
  .stop {
    color: var(--danger);
    border-color: var(--danger-line);
  }
</style>
