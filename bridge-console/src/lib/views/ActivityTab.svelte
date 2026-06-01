<script lang="ts">
  import {
    daemonCall,
    Method,
    type LogEvent,
    type LogRecentResult,
  } from "../ipc";

  // Polling-based view of the daemon's ring buffer. Streaming via
  // log.subscribe lands when the IPC layer grows notification frames;
  // until then 1-second polling is acceptable for a desktop console
  // (the buffer is bounded at 1024 entries, so a long pause won't
  // explode memory).
  const POLL_INTERVAL_MS = 1000;
  /** Cap retained in-memory; older entries fall off as new ones arrive. */
  const KEEP = 500;

  let events = $state<LogEvent[]>([]);
  let cursor = $state<number | undefined>(undefined);
  let paused = $state(false);
  let error = $state<string | null>(null);
  let initialLoaded = $state(false);

  /** Pull events newer than `cursor`. Updates state and bumps the cursor
   *  to the highest seq returned. */
  async function pull() {
    try {
      const result = await daemonCall<LogRecentResult>(Method.LogRecent, {
        after: cursor ?? null,
        limit: 200,
      });
      if (result.events.length > 0) {
        const merged = events.concat(result.events);
        events = merged.length > KEEP ? merged.slice(merged.length - KEEP) : merged;
        cursor = result.events[result.events.length - 1].seq;
      }
      error = null;
      initialLoaded = true;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  function clear() {
    events = [];
    // Don't reset the cursor — we don't want re-fetching old buffered
    // events back; clearing is purely a UI affordance.
  }

  $effect(() => {
    if (paused) return;
    let cancelled = false;
    const tick = async () => {
      if (cancelled) return;
      await pull();
    };
    void tick();
    const handle = window.setInterval(() => void tick(), POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(handle);
    };
  });

  function fmtTime(unix: number): string {
    const d = new Date(unix * 1000);
    const hh = String(d.getHours()).padStart(2, "0");
    const mm = String(d.getMinutes()).padStart(2, "0");
    const ss = String(d.getSeconds()).padStart(2, "0");
    return `${hh}:${mm}:${ss}`;
  }
</script>

<section class="activity">
  <div class="section-header">
    <h2>
      Activity
      {#if events.length > 0}
        <span class="muted">· {events.length} entries</span>
      {/if}
    </h2>
    <div class="controls">
      <button onclick={() => (paused = !paused)}>
        {paused ? "Resume" : "Pause"}
      </button>
      <button onclick={clear} disabled={events.length === 0}>Clear</button>
    </div>
  </div>

  {#if error}
    <div class="error-banner">{error}</div>
  {/if}

  {#if !initialLoaded && events.length === 0 && !error}
    <p class="muted">Loading…</p>
  {:else if events.length === 0}
    <div class="empty">
      No log entries yet. The buffer fills as the daemon emits events —
      pair a server, browse, revoke, and they'll show up here.
    </div>
  {:else}
    <ul class="log">
      {#each events as e (e.seq)}
        <li class="entry level-{e.level.toLowerCase()}">
          <span class="time mono">{fmtTime(e.timestamp)}</span>
          <span class="level lvl-{e.level.toLowerCase()}">{e.level}</span>
          <span class="target mono">{e.target}</span>
          <span class="message">{e.message}</span>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .activity {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .section-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    font-size: 14px;
    font-weight: 600;
    margin: 0;
    color: light-dark(#666, #999);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  h2 .muted {
    text-transform: none;
    letter-spacing: 0;
    font-weight: 400;
    margin-left: 6px;
  }
  .controls {
    display: flex;
    gap: 8px;
  }
  .empty {
    padding: 32px;
    border: 1px dashed light-dark(#d0d0d0, #444);
    border-radius: 8px;
    text-align: center;
    color: light-dark(#666, #999);
    font-size: 14px;
  }
  .log {
    list-style: none;
    margin: 0;
    padding: 0;
    background: light-dark(#fff, #1c1c1c);
    border: 1px solid light-dark(#e0e0e0, #333);
    border-radius: 8px;
    max-height: 60vh;
    overflow: auto;
  }
  .entry {
    display: grid;
    grid-template-columns: 64px 56px 200px 1fr;
    gap: 12px;
    padding: 6px 12px;
    align-items: baseline;
    border-bottom: 1px solid light-dark(#f0f0f0, #2a2a2a);
    font-size: 12px;
  }
  .entry:last-child {
    border-bottom: none;
  }
  .time {
    color: light-dark(#888, #888);
    font-size: 11px;
  }
  .level {
    text-transform: uppercase;
    font-weight: 500;
    font-size: 10px;
    letter-spacing: 0.04em;
  }
  .lvl-error {
    color: light-dark(#a8071a, #ff7875);
  }
  .lvl-warn {
    color: light-dark(#a86200, #f3c178);
  }
  .lvl-info {
    color: light-dark(#0d6e0d, #7ad57a);
  }
  .lvl-debug {
    color: light-dark(#666, #999);
  }
  .lvl-trace {
    color: light-dark(#999, #666);
  }
  .target {
    color: light-dark(#666, #999);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .message {
    overflow-wrap: anywhere;
    word-break: break-word;
  }
</style>
