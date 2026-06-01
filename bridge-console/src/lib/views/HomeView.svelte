<script lang="ts">
  import {
    daemonCall,
    Method,
    type DaemonStatus,
    type IdentityInfo,
    type ServerListEntry,
    TransportError,
  } from "../ipc";
  import ActivityTab from "./ActivityTab.svelte";
  import ServersTab from "./ServersTab.svelte";
  import SettingsTab from "./SettingsTab.svelte";

  type Props = {
    onpair: () => void;
  };
  let { onpair }: Props = $props();

  type Tab = "servers" | "activity" | "settings";
  let tab = $state<Tab>("servers");

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
</script>

<main>
  <header>
    <h1>MCP Bridge</h1>
    <button onclick={refresh} disabled={loading}>
      {loading ? "Loading…" : "Refresh"}
    </button>
  </header>

  {#if error}
    <div class="error-banner">{error}</div>
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
    </section>
  {/if}

  <div class="tabs" role="tablist" aria-label="Console sections">
    <button
      role="tab"
      class="tab"
      class:active={tab === "servers"}
      aria-selected={tab === "servers"}
      onclick={() => (tab = "servers")}
    >
      Servers
    </button>
    <button
      role="tab"
      class="tab"
      class:active={tab === "activity"}
      aria-selected={tab === "activity"}
      onclick={() => (tab = "activity")}
    >
      Activity
    </button>
    <button
      role="tab"
      class="tab"
      class:active={tab === "settings"}
      aria-selected={tab === "settings"}
      onclick={() => (tab = "settings")}
    >
      Settings
    </button>
  </div>

  {#if tab === "servers"}
    <ServersTab {servers} {loading} {error} onrefresh={refresh} {onpair} />
  {:else if tab === "activity"}
    <ActivityTab />
  {:else if tab === "settings"}
    <SettingsTab {identity} onrefresh={refresh} />
  {/if}
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
  .error-banner {
    margin-bottom: 20px;
  }
  .status {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 16px;
    padding: 16px;
    border-radius: 8px;
    background: light-dark(#f0f0f0, #242424);
    margin-bottom: 20px;
  }
  .status div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
    min-width: 0;
  }
  .label {
    color: light-dark(#666, #999);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    font-size: 10px;
  }
  .value {
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tabs {
    display: flex;
    gap: 4px;
    border-bottom: 1px solid light-dark(#e0e0e0, #333);
    margin-bottom: 20px;
  }
  .tab {
    border: none;
    background: transparent;
    color: light-dark(#666, #999);
    padding: 8px 14px;
    border-radius: 0;
    position: relative;
    margin-bottom: -1px;
    font-size: 14px;
  }
  .tab:hover:not(:disabled) {
    background: transparent;
    color: inherit;
  }
  .tab.active {
    color: inherit;
    border-bottom: 2px solid #0a84ff;
  }
</style>
