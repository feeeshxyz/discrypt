<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let { onUnlocked }: { onUnlocked: () => void } = $props();

  let mode: "loading" | "create" | "migrate" | "unlock" = $state("loading");
  let password = $state("");
  let confirmPassword = $state("");
  let error = $state("");
  let busy = $state(false);

  async function checkStore() {
    try {
      const format = await invoke<string>("store_format");
      if (format === "encrypted") {
        mode = "unlock";
      } else if (format === "legacy") {
        mode = "migrate";
      } else {
        mode = "create";
      }
    } catch (e: any) {
      error = e?.toString() ?? "Failed to check store status";
    }
  }

  checkStore();

  async function handleCreate() {
    error = "";
    if (password.length < 4) {
      error = "Password must be at least 4 characters.";
      return;
    }
    if (password !== confirmPassword) {
      error = "Passwords do not match.";
      return;
    }
    busy = true;
    try {
      await invoke("create_store", { password });
      onUnlocked();
    } catch (e: any) {
      error = e?.toString() ?? "Failed to create store";
    } finally {
      busy = false;
    }
  }

  async function handleMigrate() {
    error = "";
    if (password.length < 4) {
      error = "Password must be at least 4 characters.";
      return;
    }
    if (password !== confirmPassword) {
      error = "Passwords do not match.";
      return;
    }
    busy = true;
    try {
      await invoke("migrate_store", { password });
      onUnlocked();
    } catch (e: any) {
      error = e?.toString() ?? "Failed to migrate store";
    } finally {
      busy = false;
    }
  }

  async function handleUnlock() {
    error = "";
    busy = true;
    try {
      await invoke("unlock_store", { password });
      onUnlocked();
    } catch (e: any) {
      error = e?.toString() ?? "Failed to unlock store";
    } finally {
      busy = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !busy) {
      if (mode === "create") handleCreate();
      else if (mode === "migrate") handleMigrate();
      else if (mode === "unlock") handleUnlock();
    }
  }
</script>

<div class="lock-screen" onkeydown={handleKeydown}>
  <div class="lock-card">
    <h1><svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" style="vertical-align: middle; margin-right: 6px;"><rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/></svg> Discrypt</h1>
    <p class="subtitle">End-to-end encrypted Discord companion</p>

    {#if mode === "loading"}
      <p class="loading">Checking store…</p>
    {:else if mode === "create"}
      <p class="hint">Create a password to protect your keys.</p>
      <input
        type="password"
        placeholder="Password"
        bind:value={password}
        disabled={busy}
        autofocus
      />
      <input
        type="password"
        placeholder="Confirm password"
        bind:value={confirmPassword}
        disabled={busy}
      />
      <button onclick={handleCreate} disabled={busy}>
        {busy ? "Creating…" : "Create Store"}
      </button>
    {:else if mode === "migrate"}
      <p class="hint">Existing keys found. Set a password to encrypt them.</p>
      <input
        type="password"
        placeholder="Password"
        bind:value={password}
        disabled={busy}
        autofocus
      />
      <input
        type="password"
        placeholder="Confirm password"
        bind:value={confirmPassword}
        disabled={busy}
      />
      <button onclick={handleMigrate} disabled={busy}>
        {busy ? "Upgrading…" : "Encrypt & Upgrade"}
      </button>
    {:else}
      <input
        type="password"
        placeholder="Password"
        bind:value={password}
        disabled={busy}
        autofocus
      />
      <button onclick={handleUnlock} disabled={busy}>
        {busy ? "Unlocking…" : "Unlock"}
      </button>
    {/if}

    {#if error}
      <p class="error">{error}</p>
    {/if}
  </div>
</div>

<style>
  .lock-screen {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: #1e1f22;
  }

  .lock-card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    background: #2b2d31;
    border-radius: 12px;
    padding: 40px 48px;
    min-width: 320px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  }

  h1 {
    margin: 0;
    font-size: 28px;
    color: #f2f3f5;
  }

  .subtitle {
    margin: 0 0 8px;
    color: #b5bac1;
    font-size: 14px;
  }

  .hint {
    margin: 0;
    color: #b5bac1;
    font-size: 13px;
  }

  .loading {
    color: #b5bac1;
  }

  input {
    width: 100%;
    padding: 10px 14px;
    border-radius: 6px;
    border: none;
    background: #1e1f22;
    color: #f2f3f5;
    font-size: 14px;
    outline: none;
    box-sizing: border-box;
  }
  input:focus {
    box-shadow: 0 0 0 2px #5865f2;
  }
  input:disabled {
    opacity: 0.6;
  }

  button {
    width: 100%;
    padding: 10px;
    border: none;
    border-radius: 6px;
    background: #5865f2;
    color: #fff;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s;
  }
  button:hover:not(:disabled) {
    background: #4752c4;
  }
  button:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .error {
    margin: 4px 0 0;
    color: #ed4245;
    font-size: 13px;
    text-align: center;
  }
</style>
