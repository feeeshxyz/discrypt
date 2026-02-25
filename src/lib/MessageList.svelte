<script lang="ts">
  import { app } from "./stores.svelte";
  import { Lock } from "lucide-svelte";

  let messagesContainer: HTMLDivElement;

  export function scrollToBottom() {
    if (messagesContainer) {
      requestAnimationFrame(() => {
        messagesContainer.scrollTop = messagesContainer.scrollHeight;
      });
    }
  }

  export function autoScroll() {
    if (
      messagesContainer &&
      messagesContainer.scrollHeight -
        messagesContainer.scrollTop -
        messagesContainer.clientHeight <
        40
    ) {
      scrollToBottom();
    }
  }
</script>

<div class="messages-container" bind:this={messagesContainer}>
  {#if app.messages.length === 0}
    <div class="placeholder">
      <p>Not connected to Discord.</p>
      <p class="hint">
        Make sure Discord is running with <code>--remote-debugging-port=9222</code>
         and a chat is open
      </p>
    </div>
  {:else}
    {#each app.messages as msg}
      <div class="message" class:encrypted={msg.encrypted}>
        <div class="msg-avatar">
          {(msg.author || "?")[0].toUpperCase()}
        </div>
        <div class="msg-body">
          <div class="msg-author">
            {msg.author || "Unknown"}
            {#if msg.encrypted}<span class="lock-icon"><Lock size={12} /></span>{/if}
          </div>
          <div class="msg-content">{msg.content || ""}</div>
        </div>
      </div>
    {/each}
  {/if}
</div>
