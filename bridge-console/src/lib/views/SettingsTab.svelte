<script lang="ts">
  import Modal from "../components/Modal.svelte";
  import {
    daemonCall,
    Method,
    type DiagnosticsBundle,
    type IdentityInfo,
    type IdentityRotateResult,
    type UpdateCheckResult,
  } from "../ipc";

  type Props = {
    identity: IdentityInfo | null;
    onrefresh: () => Promise<void>;
  };
  let { identity, onrefresh }: Props = $props();

  // --- Identity rotate ----------------------------------------------------

  let rotateOpen = $state(false);
  let rotateConfirmText = $state("");
  let rotateBusy = $state(false);
  let rotateError = $state<string | null>(null);
  let rotateResult = $state<IdentityRotateResult | null>(null);

  function openRotate() {
    rotateOpen = true;
    rotateConfirmText = "";
    rotateError = null;
    rotateResult = null;
  }
  function closeRotate() {
    if (rotateBusy) return;
    rotateOpen = false;
  }
  async function doRotate() {
    if (rotateConfirmText.trim() !== "rotate") {
      rotateError = 'Type "rotate" exactly to confirm.';
      return;
    }
    rotateBusy = true;
    rotateError = null;
    try {
      rotateResult = await daemonCall<IdentityRotateResult>(
        Method.IdentityRotate,
      );
      await onrefresh();
    } catch (e) {
      rotateError = e instanceof Error ? e.message : String(e);
    } finally {
      rotateBusy = false;
    }
  }

  // --- Update check -------------------------------------------------------

  let updateBusy = $state(false);
  let updateResult = $state<UpdateCheckResult | null>(null);
  let updateError = $state<string | null>(null);

  async function checkUpdates() {
    updateBusy = true;
    updateError = null;
    try {
      updateResult = await daemonCall<UpdateCheckResult>(Method.UpdateCheck);
    } catch (e) {
      updateError = e instanceof Error ? e.message : String(e);
    } finally {
      updateBusy = false;
    }
  }

  // --- Diagnostics --------------------------------------------------------

  let diagnosticsOpen = $state(false);
  let diagnosticsText = $state<string>("");
  let diagnosticsBusy = $state(false);
  let diagnosticsError = $state<string | null>(null);
  let diagnosticsCopied = $state(false);

  async function openDiagnostics() {
    diagnosticsOpen = true;
    diagnosticsBusy = true;
    diagnosticsError = null;
    diagnosticsCopied = false;
    try {
      const r = await daemonCall<DiagnosticsBundle>(Method.DiagnosticsBundle);
      diagnosticsText = r.bundle;
    } catch (e) {
      diagnosticsError = e instanceof Error ? e.message : String(e);
    } finally {
      diagnosticsBusy = false;
    }
  }
  function closeDiagnostics() {
    diagnosticsOpen = false;
  }
  async function copyDiagnostics() {
    try {
      await navigator.clipboard.writeText(diagnosticsText);
      diagnosticsCopied = true;
      window.setTimeout(() => {
        diagnosticsCopied = false;
      }, 2000);
    } catch (e) {
      diagnosticsError =
        "Could not copy to clipboard: " +
        (e instanceof Error ? e.message : String(e));
    }
  }

  async function copyPubkey() {
    if (!identity) return;
    try {
      await navigator.clipboard.writeText(identity.pubkey);
    } catch {
      // Browsers reject clipboard from non-secure contexts; the Tauri
      // webview is fine. Failure here is silent — user can manually
      // select+copy.
    }
  }
</script>

<section class="settings">
  <!-- Identity card -->
  <div class="card">
    <h3>Identity</h3>
    {#if identity}
      <dl class="kv">
        <dt>Display name</dt>
        <dd>{identity.display_name}</dd>
        <dt>Resolver pubkey</dt>
        <dd>
          <code class="mono pubkey">{identity.pubkey}</code>
          <button class="link" onclick={copyPubkey}>Copy</button>
        </dd>
      </dl>
    {:else}
      <p class="muted">Loading identity…</p>
    {/if}
    <div class="card-actions">
      <button class="danger" onclick={openRotate} disabled={!identity}>
        Rotate identity…
      </button>
    </div>
    <p class="hint muted">
      Rotation generates a new Resolver keypair and revokes every paired phone.
      Each phone must re-pair. The running daemon keeps the old keypair until
      you restart it.
    </p>
  </div>

  <!-- Updates card -->
  <div class="card">
    <h3>Updates</h3>
    {#if updateResult}
      <dl class="kv">
        <dt>Current</dt>
        <dd>{updateResult.current}</dd>
        <dt>State</dt>
        <dd>{updateResult.state}</dd>
        {#if updateResult.latest}
          <dt>Latest</dt>
          <dd>{updateResult.latest}</dd>
        {/if}
      </dl>
      <p class="muted">{updateResult.message}</p>
    {:else if updateError}
      <div class="error-banner">{updateError}</div>
    {:else}
      <p class="muted">Click below to check for a newer Bridge release.</p>
    {/if}
    <div class="card-actions">
      <button onclick={checkUpdates} disabled={updateBusy}>
        {updateBusy ? "Checking…" : "Check for updates"}
      </button>
    </div>
  </div>

  <!-- Diagnostics card -->
  <div class="card">
    <h3>Diagnostics</h3>
    <p class="muted">
      A plain-text envelope of daemon state — version, identity, paired pins,
      last 50 log lines. Safe to paste into a bug report; secrets never reach
      this bundle.
    </p>
    <div class="card-actions">
      <button onclick={openDiagnostics}>Show diagnostics…</button>
    </div>
  </div>
</section>

<!-- Rotate identity modal -->
{#if rotateOpen}
  {#if rotateResult}
    <Modal title="Identity rotated" onclose={closeRotate}>
      <p>
        A fresh Resolver keypair has been written to the keychain.
        <b>{rotateResult.revoked_pins}</b>
        pin{rotateResult.revoked_pins === 1 ? "" : "s"} revoked. Each phone
        will need to re-pair.
      </p>
      <dl class="kv">
        <dt>New pubkey</dt>
        <dd><code class="mono pubkey">{rotateResult.new_pubkey}</code></dd>
      </dl>
      {#if rotateResult.restart_required}
        <div class="warn-banner">
          <b>Restart required.</b> This runtime cannot hot-swap the keypair.
          Stop and restart the daemon (or
          <code>mcp-bridge daemon --uninstall && --install</code>) for the
          rotated identity to take effect on the wire.
        </div>
      {:else}
        <p class="muted">
          The in-memory keypair is hot-swapped — new invites carry the new
          pubkey, and the mDNS subscriber re-subscribes against the new
          service type.
        </p>
      {/if}
      {#snippet actions()}
        <button class="primary" onclick={closeRotate}>Done</button>
      {/snippet}
    </Modal>
  {:else}
    <Modal title="Rotate identity?" onclose={closeRotate}>
      <p class="muted">
        This generates a new Resolver keypair and <b>revokes every paired
        phone</b>. Each phone will need to re-pair from scratch.
      </p>
      <p>
        Type <code>rotate</code> to confirm:
      </p>
      <input
        type="text"
        bind:value={rotateConfirmText}
        placeholder="rotate"
        disabled={rotateBusy}
      />
      {#if rotateError}
        <div class="error-banner">{rotateError}</div>
      {/if}
      {#snippet actions()}
        <button onclick={closeRotate} disabled={rotateBusy}>Cancel</button>
        <button class="danger primary" onclick={doRotate} disabled={rotateBusy}>
          {rotateBusy ? "Rotating…" : "Rotate"}
        </button>
      {/snippet}
    </Modal>
  {/if}
{/if}

<!-- Diagnostics modal -->
{#if diagnosticsOpen}
  <Modal title="Diagnostics bundle" onclose={closeDiagnostics} wide>
    {#if diagnosticsBusy}
      <p class="muted">Loading…</p>
    {:else if diagnosticsError}
      <div class="error-banner">{diagnosticsError}</div>
    {:else}
      <pre class="bundle">{diagnosticsText}</pre>
    {/if}
    {#snippet actions()}
      <button onclick={copyDiagnostics} disabled={diagnosticsBusy || !!diagnosticsError}>
        {diagnosticsCopied ? "Copied!" : "Copy to clipboard"}
      </button>
      <button class="primary" onclick={closeDiagnostics}>Close</button>
    {/snippet}
  </Modal>
{/if}

<style>
  .settings {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
  .card {
    background: light-dark(#fff, #242424);
    border: 1px solid light-dark(#e0e0e0, #333);
    border-radius: 8px;
    padding: 16px 20px;
  }
  .card h3 {
    margin: 0 0 12px 0;
    font-size: 13px;
    font-weight: 600;
    color: light-dark(#666, #999);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .card-actions {
    display: flex;
    gap: 8px;
    margin-top: 12px;
  }
  .card .hint {
    margin: 10px 0 0;
    font-size: 12px;
    line-height: 1.5;
  }
  .kv {
    display: grid;
    grid-template-columns: 140px 1fr;
    gap: 6px 16px;
    margin: 0;
  }
  .kv dt {
    color: light-dark(#666, #999);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    align-self: center;
  }
  .kv dd {
    margin: 0;
    font-size: 13px;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .pubkey {
    overflow-wrap: anywhere;
    word-break: break-all;
  }
  .bundle {
    background: light-dark(#f5f5f5, #1a1a1a);
    border: 1px solid light-dark(#e0e0e0, #333);
    border-radius: 6px;
    padding: 12px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    line-height: 1.5;
    max-height: 60vh;
    overflow: auto;
    white-space: pre-wrap;
    word-break: break-word;
    margin: 0;
  }
  .warn-banner {
    background: light-dark(#fff7e6, #3a2e1e);
    border: 1px solid light-dark(#ffe7ba, #5a4a2e);
    color: light-dark(#874d00, #f3c178);
    padding: 10px 14px;
    border-radius: 6px;
    font-size: 13px;
    line-height: 1.5;
    margin-top: 10px;
  }
  .warn-banner :global(code) {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    background: light-dark(#fffbe6, #2a2418);
    padding: 1px 6px;
    border-radius: 4px;
  }
</style>
