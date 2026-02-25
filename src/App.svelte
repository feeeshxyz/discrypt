<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import LockScreen from "./lib/LockScreen.svelte";
  import Toolbar from "./lib/Toolbar.svelte";
  import MessageList from "./lib/MessageList.svelte";
  import ChatBar from "./lib/ChatBar.svelte";
  import KeysPanel from "./lib/KeysPanel.svelte";
  import Settings from "./lib/Settings.svelte";
  import { Settings as SettingsIcon } from "lucide-svelte";
  import { app, setStatus, loadKeys, type FetchResult } from "./lib/stores.svelte";

  let dmRecipient = $derived(app.dmUsernames.length > 0 ? app.dmUsernames[0] : "");

  let unlocked = $state(false);
  let messageList: MessageList;

  async function checkUnlocked() {
    unlocked = await invoke<boolean>("is_store_unlocked");
  }
  checkUnlocked();

  function handleUnlocked() {
    unlocked = true;
    loadKeys();
  }

  onMount(() => {
    loadKeys();

    listen<FetchResult>("cdp-messages-updated", (event) => {
      app.messages = event.payload.messages;
      app.dmUsernames = event.payload.dm_usernames || [];
      messageList?.autoScroll();
    });

    listen("cdp-disconnected", () => {
      setStatus("disconnected", "Disconnected");
      app.connected = false;
    });

    listen("cdp-reconnecting", () => {
      setStatus("connecting", "Reconnecting…");
    });

    listen("cdp-reconnected", () => {
      setStatus("connected", "Connected — Live");
      app.connected = true;
    });

    function onGlobalKeydown(e: KeyboardEvent) {
      if (e.key === "Escape") {
        if (app.settingsOpen) { app.settingsOpen = false; return; }
        if (app.keysOpen) { app.keysOpen = false; return; }
      }
    }
    window.addEventListener("keydown", onGlobalKeydown);
    return () => window.removeEventListener("keydown", onGlobalKeydown);
  });
</script>

{#if !unlocked}
  <LockScreen onUnlocked={handleUnlocked} />
{:else}
<div class="app">
  <header>
    <h1>Discrypt {#if dmRecipient} <span class="header-recipient"> - {dmRecipient}</span>{/if}</h1>
    <button class="btn-header-settings btn-icon" onclick={() => { app.settingsOpen = !app.settingsOpen; if (app.settingsOpen) app.keysOpen = false; }} title="Settings">
      <SettingsIcon size={18} />
    </button>
  </header>

  <Toolbar />

  <div class="content-area">
    <MessageList bind:this={messageList} />
    <KeysPanel />
    <Settings />
  </div>

  <ChatBar />
</div>
{/if}
