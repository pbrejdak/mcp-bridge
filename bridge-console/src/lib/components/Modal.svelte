<script lang="ts">
  import type { Snippet } from "svelte";

  type Props = {
    title: string;
    onclose: () => void;
    children: Snippet;
    actions?: Snippet;
    /** Wider variant for long-text content like diagnostics bundles. */
    wide?: boolean;
  };
  let { title, onclose, children, actions, wide = false }: Props = $props();
</script>

<div
  class="modal-backdrop"
  role="dialog"
  aria-modal="true"
  aria-labelledby="modal-title"
  onclick={onclose}
  onkeydown={(e) => e.key === "Escape" && onclose()}
  tabindex="-1"
>
  <!-- Click inside must not propagate to the backdrop close handler. -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="modal"
    class:wide
    role="document"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <h2 id="modal-title">{title}</h2>
    <div class="modal-body">
      {@render children()}
    </div>
    {#if actions}
      <div class="modal-actions">
        {@render actions()}
      </div>
    {/if}
  </div>
</div>

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    display: grid;
    place-items: center;
    padding: 20px;
    z-index: 100;
  }
  .modal {
    background: light-dark(#fff, #2a2a2a);
    border: 1px solid light-dark(#e0e0e0, #444);
    border-radius: 10px;
    padding: 24px;
    max-width: 460px;
    width: 100%;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.2);
  }
  .modal.wide {
    max-width: 760px;
  }
  .modal h2 {
    margin: 0 0 12px 0;
    font-size: 17px;
    font-weight: 600;
  }
  .modal-body :global(p) {
    margin: 0 0 12px 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .modal-actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 20px;
  }
</style>
