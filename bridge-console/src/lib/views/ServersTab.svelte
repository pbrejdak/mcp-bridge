<script lang="ts">
  import Modal from "../components/Modal.svelte";
  import { daemonCall, Method, type ServerListEntry } from "../ipc";

  type Props = {
    servers: ServerListEntry[];
    loading: boolean;
    error: string | null;
    onrefresh: () => Promise<void>;
    onpair: () => void;
  };
  let { servers, loading, error, onrefresh, onpair }: Props = $props();

  let revokeTarget = $state<ServerListEntry | null>(null);
  let revokeBusy = $state(false);
  let revokeError = $state<string | null>(null);

  function openRevoke(s: ServerListEntry) {
    revokeTarget = s;
    revokeError = null;
  }

  function closeRevoke() {
    revokeTarget = null;
    revokeBusy = false;
    revokeError = null;
  }

  async function confirmRevoke() {
    if (!revokeTarget) return;
    revokeBusy = true;
    revokeError = null;
    try {
      await daemonCall(Method.ServersRevoke, { pin_id: revokeTarget.pin_id });
      closeRevoke();
      await onrefresh();
    } catch (e) {
      revokeError = e instanceof Error ? e.message : String(e);
      revokeBusy = false;
    }
  }

  function fmtState(state: ServerListEntry["state"]): string {
    return state.toLowerCase();
  }
</script>

<section class="servers">
  <div class="section-header">
    <h2>Paired servers ({servers.length})</h2>
    <button class="primary" onclick={onpair} disabled={!!error || loading}>
      Pair new server
    </button>
  </div>
  {#if servers.length === 0 && !loading && !error}
    <div class="empty">
      No paired servers yet. Click <b>Pair new server</b> above to walk through
      the QR + SAS flow.
    </div>
  {:else if servers.length > 0}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Pin&nbsp;ID</th>
          <th>State</th>
          <th>Backend</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each servers as s (s.pin_id)}
          <tr class:revoked={s.state === "Revoked"}>
            <td>{s.name}</td>
            <td class="mono">{s.pin_id}</td>
            <td class="state state-{fmtState(s.state)}">{fmtState(s.state)}</td>
            <td class="mono">{s.backend_url}</td>
            <td class="row-actions">
              {#if s.state !== "Revoked"}
                <button
                  class="danger"
                  aria-label="Revoke {s.name}"
                  onclick={() => openRevoke(s)}
                >
                  Revoke
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

{#if revokeTarget}
  {@const target = revokeTarget}
  <Modal title="Revoke {target.name}?" onclose={closeRevoke}>
    <p class="muted">
      The phone behind <span class="mono">{target.pin_id}</span> will stop
      reaching this Bridge. The per-pin secrets are deleted from the keychain,
      and the Claude Desktop entry (and any other adapter entries carrying our
      sentinel) is removed.
    </p>
    <p class="muted">This is reversible only by re-pairing the phone.</p>
    {#if revokeError}
      <div class="error-banner">Revoke failed: {revokeError}</div>
    {/if}
    {#snippet actions()}
      <button onclick={closeRevoke} disabled={revokeBusy}>Cancel</button>
      <button class="danger primary" onclick={confirmRevoke} disabled={revokeBusy}>
        {revokeBusy ? "Revoking…" : "Revoke"}
      </button>
    {/snippet}
  </Modal>
{/if}

<style>
  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 12px;
  }
  .servers h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
    color: light-dark(#666, #999);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    text-align: left;
    font-weight: 500;
    color: light-dark(#666, #999);
    border-bottom: 1px solid light-dark(#e0e0e0, #333);
    padding: 8px 12px;
  }
  td {
    border-bottom: 1px solid light-dark(#eee, #2a2a2a);
    padding: 10px 12px;
  }
  tr.revoked {
    opacity: 0.55;
  }
  .state {
    display: inline-block;
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
  }
  .state-reachable {
    background: light-dark(#e6f7e6, #1e3a1e);
    color: light-dark(#0d6e0d, #7ad57a);
  }
  .state-unreachable {
    background: light-dark(#fff7e6, #3a2e1e);
    color: light-dark(#a86200, #f3c178);
  }
  .state-revoked {
    background: light-dark(#f0f0f0, #2a2a2a);
    color: light-dark(#666, #999);
  }
  .empty {
    padding: 32px;
    border: 1px dashed light-dark(#d0d0d0, #444);
    border-radius: 8px;
    text-align: center;
    color: light-dark(#666, #999);
    font-size: 14px;
  }
  .row-actions {
    text-align: right;
    width: 1%;
    white-space: nowrap;
  }
  .row-actions button {
    font-size: 12px;
    padding: 4px 10px;
  }
</style>
