<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  import { app, loadKeys } from "./stores.svelte";

  let changePwOld = $state("");
  let changePwNew = $state("");
  let changePwConfirm = $state("");
  let changePwMsg = $state("");
  let changePwBusy = $state(false);

  let exportMsg = $state("");

  let importData = $state("");
  let importPassword = $state("");
  let importMsg = $state("");
  let importBusy = $state(false);

  async function changePassword() {
    changePwMsg = "";
    if (changePwNew.length < 4) {
      changePwMsg = "New password must be at least 4 characters.";
      return;
    }
    if (changePwNew !== changePwConfirm) {
      changePwMsg = "Passwords do not match.";
      return;
    }
    changePwBusy = true;
    try {
      await invoke("change_password", {
        oldPassword: changePwOld,
        newPassword: changePwNew,
      });
      changePwMsg = "Password changed successfully.";
      changePwOld = "";
      changePwNew = "";
      changePwConfirm = "";
    } catch (e: any) {
      changePwMsg = e?.toString() ?? "Failed to change password";
    } finally {
      changePwBusy = false;
    }
  }

  async function exportStore() {
    exportMsg = "";
    try {
      const data = await invoke<string>("export_store");
      await navigator.clipboard.writeText(data);
      exportMsg = "Store backup copied to clipboard.";
    } catch (e: any) {
      exportMsg = e?.toString() ?? "Export failed";
    }
  }

  async function importStore() {
    importMsg = "";
    const data = importData.trim();
    const pw = importPassword;
    if (!data) { importMsg = "Paste your backup data first."; return; }
    if (!pw) { importMsg = "Enter backup password."; return; }
    if (!confirm("This will replace your current keys and contacts. Continue?")) return;
    importBusy = true;
    try {
      await invoke("import_store", { jsonData: data, password: pw });
      importMsg = "Backup restored successfully!";
      importData = "";
      importPassword = "";
      await loadKeys();
    } catch (e: any) {
      importMsg = e?.toString() ?? "Import failed";
    } finally {
      importBusy = false;
    }
  }

  async function resetAll() {
    if (!confirm("This will delete all keys and contacts. Are you sure?")) return;
    if (!confirm("This action cannot be undone. Really delete everything?")) return;
    try {
      await invoke("reset_store");
      window.location.reload();
    } catch (e: any) {
      alert(e?.toString() ?? "Reset failed");
    }
  }


</script>

{#if app.activePanel === "settings"}
<div class="settings-panel-content">
  <div class="settings-section">
    <h3>Change Password</h3>
    <input type="password" placeholder="Current password" bind:value={changePwOld} disabled={changePwBusy} />
    <input type="password" placeholder="New password" bind:value={changePwNew} disabled={changePwBusy} />
    <input type="password" placeholder="Confirm new password" bind:value={changePwConfirm} disabled={changePwBusy} />
    <button class="btn-small" onclick={changePassword} disabled={changePwBusy}>
      {changePwBusy ? "Changing…" : "Change Password"}
    </button>
    {#if changePwMsg}<p class="settings-msg">{changePwMsg}</p>{/if}
  </div>

  <div class="settings-section">
    <h3>Backup</h3>
    <button class="btn-small" onclick={exportStore}>Copy Backup to Clipboard</button>
    {#if exportMsg}<p class="settings-msg">{exportMsg}</p>{/if}
  </div>

  <div class="settings-section">
    <h3>Import Backup</h3>
    <textarea class="import-textarea" bind:value={importData} placeholder="Paste backup JSON here…" disabled={importBusy} rows="3"></textarea>
    <input type="password" placeholder="Backup password" bind:value={importPassword} disabled={importBusy} />
    <button class="btn-small" onclick={importStore} disabled={importBusy}>
      {importBusy ? "Importing…" : "Import Backup"}
    </button>
    {#if importMsg}<p class="settings-msg">{importMsg}</p>{/if}
  </div>

  <div class="settings-section danger">
    <h3>Danger Zone</h3>
    <button class="btn-danger" onclick={resetAll}>Reset All Data</button>
    <p class="settings-hint">Deletes keys, contacts, and all local data.</p>
  </div>

  <div class="settings-section">
    <h3>About</h3>
    <p class="settings-about">Discrypt v0.1.0</p>
    <p class="settings-about">E2E encrypted Discord DM companion</p>
    <p class="settings-about">X25519 + AES-256-GCM • Argon2id key store</p>
  </div>
</div>
{/if}
