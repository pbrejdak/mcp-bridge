<script lang="ts">
  import {
    daemonCall,
    Method,
    type DaemonStatus,
    type IdentityInfo,
    type ServerListEntry,
    TransportError,
  } from "../ipc";

  type Props = {
    onpair: () => void;
  };
  let { onpair }: Props = $props();

  let status = $state<DaemonStatus | null>(null);
  let identity = $state<IdentityInfo | null>(null);
  let servers = $state<ServerListEntry[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);

  async function refresh() {
    loading = true;
    error = null;
    try {
      const [s, i, list] = await Promise.all([
        daemonCall<DaemonStatus>(Method.DaemonStatus),
        daemonCall<IdentityInfo>(Method.IdentityShow),
        daemonCall<ServerListEntry[]>(Method.ServersList),
      ]);
      status = s;
      identity = i;
      servers = list;
    } catch (e) {
      if (e instanceof TransportError) {
        error =
          "Daemon not reachable. Start it with `mcp-bridge daemon` and click Refresh.";
      } else {
        error = e instanceof Error ? e.message : String(e);
      }
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void refresh();
  });

  function fmtUptime(secs: number): string {
    if (secs < 60) return `${secs}s`;
    if (secs < 3600) return `${Math.floor(secs / 60)}m`;
    if (secs < 86400) return `${Math.floor(secs / 3600)}h`;
    return `${Math.floor(secs / 86400)}d`;
  }

  function fmtState(state: ServerListEntry["state"]): string {
    return state.toLowerCase();
  }
</script>

<main>
  <header>
    <h1>MCP Bridge</h1>
    <div class="actions">
      <button class="primary" onclick={onpair} disabled={!!error || loading}>
        Pair new server
      </button>
      <button onclick={refresh} disabled={loading}>
        {loading ? "Loading…" : "Refresh"}
      </button>
    </div>
  </header>

  {#if error}
    <div class="error" role="alert">{error}</div>
  {/if}

  {#if status && identity}
    <section class="status">
      <div>
        <span class="label">Daemon</span>
        <span class="value">v{status.version}</span>
      </div>
      <div>
        <span class="label">Uptime</span>
        <span class="value">{fmtUptime(status.uptime_seconds)}</span>
      </div>
      <div>
        <span class="label">Pair endpoint</span>
        <span class="value">{status.pair_endpoint}</span>
      </div>
      <div>
        <span class="label">Resolver</span>
        <span class="value">{identity.display_name}</span>
      </div>
      <div class="pubkey">
        <span class="label">Pubkey</span>
        <span class="value mono">{identity.pubkey}</span>
      </div>
    </section>
  {/if}

  <section class="servers">
    <h2>Paired servers ({servers.length})</h2>
    {#if servers.length === 0 && !loading && !error}
      <div class="empty">
        No paired servers yet. Click <b>Pair new server</b> above to walk
        through the QR + SAS flow.
      </div>
    {:else if servers.length > 0}
      <table>
        <thead>
          <tr>
            <th>Name</th>
            <th>Pin&nbsp;ID</th>
            <th>State</th>
            <th>Backend</th>
          </tr>
        </thead>
        <tbody>
          {#each servers as s (s.pin_id)}
            <tr class:revoked={s.state === "Revoked"}>
              <td>{s.name}</td>
              <td class="mono">{s.pin_id}</td>
              <td class="state state-{fmtState(s.state)}">{fmtState(s.state)}</td>
              <td class="mono">{s.backend_url}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>
</main>

<style>
  main {
    max-width: 900px;
    margin: 0 auto;
    padding: 24px 28px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 24px;
  }
  header h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: 8px;
  }
  button {
    appearance: none;
    border: 1px solid light-dark(#d0d0d0, #444);
    background: light-dark(#fff, #2a2a2a);
    color: inherit;
    padding: 6px 14px;
    border-radius: 6px;
    font: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: light-dark(#f0f0f0, #333);
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  button.primary {
    background: light-dark(#0a84ff, #0a84ff);
    border-color: light-dark(#0066cc, #0066cc);
    color: white;
  }
  button.primary:hover:not(:disabled) {
    background: light-dark(#0066cc, #0a6cdc);
  }
  .error {
    background: light-dark(#fff1f0, #401a1a);
    border: 1px solid light-dark(#ffccc7, #602a2a);
    color: light-dark(#a8071a, #ffa39e);
    padding: 10px 14px;
    border-radius: 6px;
    margin-bottom: 20px;
    font-size: 14px;
  }
  .status {
    display: grid;
    grid-template-columns: repeat(2, 1fr);
    gap: 12px 24px;
    padding: 16px;
    border-radius: 8px;
    background: light-dark(#f0f0f0, #242424);
    margin-bottom: 24px;
  }
  .status div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
  }
  .status .pubkey {
    grid-column: 1 / -1;
  }
  .label {
    color: light-dark(#666, #999);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 10px;
  }
  .value {
    font-size: 14px;
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .servers h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0 0 12px 0;
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
</style>
