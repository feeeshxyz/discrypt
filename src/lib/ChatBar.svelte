<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, setStatus, loadKeys, type HandshakeResult } from "./stores.svelte";
  import { Lock, Handshake, Loader, SendHorizonal } from "lucide-svelte";

  let chatText = $state("");

  let encryptTarget = $derived.by(() => {
    for (const u of app.dmUsernames) {
      if (app.contacts.some((c) => c.username === u)) return u;
    }
    return null;
  });

  let isEncrypted = $derived.by(() => {
    if (!encryptTarget) return false;
    const c = app.contacts.find((c) => c.username === encryptTarget);
    return c?.handshake_status === "complete";
  });

  async function doSend() {
    const text = chatText.trim();
    if (!text) return;
    try {
      await invoke("cdp_send_message", {
        message: text,
        encryptFor: encryptTarget,
      });
      chatText = "";
    } catch (_) {}
  }

  async function doHandshake() {
    app.handshaking = true;
    try {
      const result = await invoke<HandshakeResult>("cdp_handshake");
      await loadKeys();
      setStatus("connected", result.message);
    } catch (_) {}
    app.handshaking = false;
  }

  function onChatKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      doSend();
    }
  }
</script>

<div class="chat-bar">
  {#if isEncrypted}
    <span
      class="encrypt-indicator active"
      title={`Encrypted with ${encryptTarget}`}
    ><Lock size={18} /></span>
  {:else}
    <button
      class="btn-handshake btn-icon"
      onclick={doHandshake}
      disabled={!app.connected || app.handshaking}
      title="Exchange encryption keys with DM partner"
    >
      {#if app.handshaking}<Loader size={18} />{:else}<Handshake size={18} />{/if}
    </button>
  {/if}
  <input
    type="text"
    class="chat-input"
    bind:value={chatText}
    onkeydown={onChatKeydown}
    placeholder="Type a message…"
    disabled={!app.connected}
  />
  <button class="btn-send btn-icon" onclick={doSend} disabled={!app.connected} title="Send">
    <SendHorizonal size={18} />
  </button>
</div>
