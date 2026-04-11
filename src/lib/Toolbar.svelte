<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, setStatus, loadKeys, type FetchResult } from "./stores.svelte";
  import { onMount } from "svelte";
  import { KeyRound, Rocket, Loader } from "lucide-svelte";

  interface DiscordStatus {
    running: boolean;
    cdp_available: boolean;
  }

  let discordStatus: DiscordStatus | null = $state(null);
  let launching = $state(false);

  onMount(async () => {
    try {
      discordStatus = await invoke<DiscordStatus>("check_discord_status");
      if (discordStatus?.cdp_available && !app.connected) {
        doConnect();
      }
    } catch (_) {}

    const retryInterval = setInterval(async () => {
      if (app.connected || app.connecting) return;
      try {
        discordStatus = await invoke<DiscordStatus>("check_discord_status");
        if (discordStatus?.cdp_available) {
          doConnect();
        }
      } catch (_) {}
    }, 5000);

    return () => clearInterval(retryInterval);
  });

  async function doConnect() {
    app.connecting = true;
    setStatus("connecting", "Connecting…");
    try {
      const result = await invoke<FetchResult>("cdp_connect");
      setStatus("connected", "Connected — Live");
      app.connected = true;
      app.messages = result.messages;
      app.dmUsernames = result.dm_usernames || [];
      discordStatus = { running: true, cdp_available: true };
      await loadKeys();
    } catch (_) {
      setStatus("disconnected", "Disconnected");
      app.messages = [];
    } finally {
      app.connecting = false;
    }
  }

  async function doLaunchDiscord() {
    launching = true;
    try {
      await invoke<string>("launch_discord_with_cdp");
      setTimeout(async () => {
        try {
          discordStatus = await invoke<DiscordStatus>("check_discord_status");
        } catch (_) {}
        launching = false;
      }, 5000);
    } catch (e: any) {
      alert(e?.toString() ?? "Failed to launch Discord");
      launching = false;
    }
  }
</script>

{#if discordStatus && !discordStatus.cdp_available && !app.connected}
  <div class="discord-banner">
    {#if !discordStatus.running}
      <span>Discord is not running.</span>
    {:else}
      <span>Discord is running but CDP is not enabled.</span>
    {/if}
    <button class="btn-small btn-icon" onclick={doLaunchDiscord} disabled={launching} title="Launch Discord with CDP">
      {#if launching}<Loader size={14} />{:else}<span>Run now</span>{/if}
    </button>
    <span class="banner-hint">or add <code>--remote-debugging-port=9222</code> to your Discord shortcut</span>
  </div>
{/if}

<div class="toolbar">
  <button class="btn-keys btn-icon" onclick={() => { app.activePanel = app.activePanel === "keys" ? null : "keys"; }} title="Keys">
    <KeyRound size={18} />
  </button>
  <button
    class="status-btn {app.statusClass}"
    onclick={doConnect}
    disabled={app.connecting}
    title={app.connected ? 'Click to reconnect' : 'Click to connect'}
  >
    <span class="status-dot"></span>
    {app.statusText}
  </button>
</div>
