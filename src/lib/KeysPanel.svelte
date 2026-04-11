<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { app, loadKeys } from "./stores.svelte";
  import { Copy, X, User, Plus } from "lucide-svelte";

  let contactUsername = $state("");
  let contactPubkey = $state("");

  async function addContact() {
    const u = contactUsername.trim();
    const k = contactPubkey.trim();
    if (!u || !k) return;
    try {
      await invoke("add_contact", { username: u, publicKeyHex: k });
      contactUsername = "";
      contactPubkey = "";
      await loadKeys();
    } catch (_) {}
  }

  async function removeContact(username: string) {
    try {
      await invoke("remove_contact", { username });
      await loadKeys();
    } catch (_) {}
  }

  async function copyPublicKey() {
    try {
      await navigator.clipboard.writeText(app.myPublicKey);
    } catch (_) {}
  }

  function fillDmUsername() {
    if (app.dmUsernames.length > 0) {
      contactUsername = app.dmUsernames[0];
    }
  }
</script>

{#if app.activePanel === "keys"}
  <div class="keys-panel-content">
    <div class="keys-section">
      <h3>Your Public Key</h3>
      <div class="key-display">
        <code>{app.myPublicKey}</code>
        <button class="btn-small btn-icon" onclick={copyPublicKey} title="Copy to clipboard"><Copy size={14} /></button>
      </div>
    </div>
    <div class="keys-section">
      <h3>Contacts</h3>
      {#if app.contacts.length === 0}
        <p class="no-contacts">No contacts added yet.</p>
      {:else}
        {#each app.contacts as c}
          <div class="contact-entry">
            <div class="contact-info">
              <span class="contact-name">
                {#if c.handshake_status === "complete"}
                  <span class="hs-badge complete" title="Encryption active"></span>
                {:else}
                  <span class="hs-badge pending" title="Handshake pending"></span>
                {/if}
                {c.username}
              </span>
              <span class="contact-fp">{c.fingerprint}</span>
            </div>
            <button
              class="btn-remove-contact btn-icon"
              onclick={() => removeContact(c.username)}><X size={14} /></button
            >
          </div>
        {/each}
      {/if}
      <div class="add-contact-form">
        <div class="contact-username-row">
          <input
            type="text"
            bind:value={contactUsername}
            placeholder="Discord username"
          />
          <button class="btn-small btn-icon" onclick={fillDmUsername} title="Use current DM user"><User size={14} /></button>
        </div>
        <input
          type="text"
          bind:value={contactPubkey}
          placeholder="Their public key (hex)"
        />
        <button class="btn-small btn-icon" onclick={addContact} title="Add contact"><Plus size={14} /></button>
      </div>
    </div>
  </div>
{/if}
