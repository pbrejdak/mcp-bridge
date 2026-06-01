<script lang="ts">
  import QRCode from "qrcode";
  import {
    daemonCall,
    Method,
    type PairInvite,
    type ServerListEntry,
    TransportError,
  } from "../ipc";

  type Props = {
    onclose: () => void;
  };
  let { onclose }: Props = $props();

  // State machine: starting → waiting → success | expired | cancelled | error
  type Stage =
    | { kind: "starting" }
    | { kind: "waiting"; invite: PairInvite; qrDataUrl: string; secondsLeft: number }
    | { kind: "success"; pin: ServerListEntry }
    | { kind: "expired"; invite: PairInvite }
    | { kind: "error"; message: string };

  let stage = $state<Stage>({ kind: "starting" });
  // Pin set captured BEFORE invite_start; the polling loop watches for
  // any pin_id not in this set to land in servers.list.
  let beforePins = new Set<string>();
  // The invite lifetime in seconds per SPEC §4.3. Mirrors the daemon's
  // InviteRegister constant.
  const INVITE_LIFETIME_SECS = 60;
  // The largest pin_id snapshot fetch we'll do per polling tick.
  const POLL_INTERVAL_MS = 500;

  /** Best-effort `pair.invite_cancel`. Failure is a warning only —
   *  the invite will expire on its own at most 60s from start. */
  async function cancelInvite(nonce: string) {
    try {
      await daemonCall(Method.PairInviteCancel, { nonce });
    } catch (e) {
      console.warn("pair.invite_cancel failed", e);
    }
  }

  async function start() {
    try {
      // Snapshot existing pins.
      const before = await daemonCall<ServerListEntry[]>(Method.ServersList);
      beforePins = new Set(before.map((p) => p.pin_id));

      // Generate invite.
      const invite = await daemonCall<PairInvite>(Method.PairInviteStart);
      const qrDataUrl = await QRCode.toDataURL(JSON.stringify(invite), {
        width: 280,
        margin: 1,
        errorCorrectionLevel: "M",
      });
      stage = {
        kind: "waiting",
        invite,
        qrDataUrl,
        secondsLeft: INVITE_LIFETIME_SECS,
      };
    } catch (e) {
      stage = {
        kind: "error",
        message:
          e instanceof TransportError
            ? "Daemon not reachable. Start it with `mcp-bridge daemon`."
            : e instanceof Error
              ? e.message
              : String(e),
      };
    }
  }

  // Kick off the flow.
  $effect(() => {
    void start();
  });

  // While we're in `waiting`, poll the registry every 500ms for a new
  // pin and tick a one-second countdown. Each is its own $effect so
  // they tear down cleanly when stage transitions out of `waiting`.
  $effect(() => {
    if (stage.kind !== "waiting") return;
    const nonce = stage.invite.nonce;

    let cancelled = false;
    const poll = window.setInterval(async () => {
      if (cancelled || stage.kind !== "waiting") return;
      try {
        const list = await daemonCall<ServerListEntry[]>(Method.ServersList);
        const fresh = list.find((p) => !beforePins.has(p.pin_id));
        if (fresh) {
          stage = { kind: "success", pin: fresh };
        }
      } catch {
        // Transient errors here just retry on the next tick.
      }
    }, POLL_INTERVAL_MS);

    const tick = window.setInterval(() => {
      if (stage.kind !== "waiting") return;
      const next = stage.secondsLeft - 1;
      if (next <= 0) {
        // Expired — clean up the invite on the daemon side and
        // surface the expired screen.
        const expiredInvite = stage.invite;
        stage = { kind: "expired", invite: expiredInvite };
        void cancelInvite(nonce);
      } else {
        stage = { ...stage, secondsLeft: next };
      }
    }, 1000);

    return () => {
      cancelled = true;
      window.clearInterval(poll);
      window.clearInterval(tick);
    };
  });

  function handleCancel() {
    if (stage.kind === "waiting") {
      void cancelInvite(stage.invite.nonce);
    }
    onclose();
  }

  function handleDone() {
    onclose();
  }

  async function handleRestart() {
    stage = { kind: "starting" };
    await start();
  }
</script>

<main>
  <header>
    <h1>Pair a new server</h1>
    <button class="link" onclick={handleCancel}>Cancel</button>
  </header>

  {#if stage.kind === "starting"}
    <div class="centered">
      <p class="muted">Generating invite…</p>
    </div>
  {:else if stage.kind === "waiting"}
    <section class="pair">
      <div class="qr">
        <img src={stage.qrDataUrl} alt="Pairing QR code" width="280" height="280" />
      </div>
      <dl class="details">
        <dt>Bridge name</dt>
        <dd>{stage.invite.resolver.display_name}</dd>

        <dt>LAN address</dt>
        <dd class="mono">{stage.invite.resolver.lan_addr}</dd>

        <dt>SAS phrase</dt>
        <dd class="sas">{stage.invite.resolver.sas}</dd>
      </dl>
      <p class="instructions">
        Scan the QR with the phone. After it sends the pair POST, the SAS
        phrase above must match what the phone shows. Confirm there, and
        this window will jump to the success screen.
      </p>
      <p class="countdown">
        Invite expires in <b>{stage.secondsLeft}s</b>
      </p>
    </section>
  {:else if stage.kind === "success"}
    <section class="success">
      <div class="badge ok">✓</div>
      <h2>Paired with {stage.pin.name}</h2>
      <dl class="details">
        <dt>Pin ID</dt>
        <dd class="mono">{stage.pin.pin_id}</dd>
        <dt>Backend</dt>
        <dd class="mono">{stage.pin.backend_url}</dd>
      </dl>
      <button class="primary" onclick={handleDone}>Done</button>
    </section>
  {:else if stage.kind === "expired"}
    <section class="expired">
      <div class="badge warn">!</div>
      <h2>Invite expired</h2>
      <p class="muted">
        No phone completed the pair within {INVITE_LIFETIME_SECS}s. The invite
        has been released; you can try again.
      </p>
      <div class="actions">
        <button class="primary" onclick={handleRestart}>Try again</button>
        <button onclick={onclose}>Back</button>
      </div>
    </section>
  {:else if stage.kind === "error"}
    <section class="error">
      <div class="badge error">×</div>
      <h2>Couldn't start pair</h2>
      <p>{stage.message}</p>
      <div class="actions">
        <button class="primary" onclick={handleRestart}>Try again</button>
        <button onclick={onclose}>Back</button>
      </div>
    </section>
  {/if}
</main>

<style>
  main {
    max-width: 560px;
    margin: 0 auto;
    padding: 28px;
  }
  header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    margin-bottom: 24px;
  }
  header h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 600;
  }
  button {
    appearance: none;
    border: 1px solid light-dark(#d0d0d0, #444);
    background: light-dark(#fff, #2a2a2a);
    color: inherit;
    padding: 8px 16px;
    border-radius: 6px;
    font: inherit;
    cursor: pointer;
  }
  button:hover:not(:disabled) {
    background: light-dark(#f0f0f0, #333);
  }
  button.primary {
    background: light-dark(#0a84ff, #0a84ff);
    border-color: light-dark(#0066cc, #0066cc);
    color: white;
  }
  button.primary:hover:not(:disabled) {
    background: light-dark(#0066cc, #0a6cdc);
  }
  button.link {
    background: transparent;
    border: none;
    color: light-dark(#666, #999);
    padding: 4px 8px;
  }
  button.link:hover {
    color: inherit;
    background: transparent;
    text-decoration: underline;
  }
  .centered {
    text-align: center;
    padding: 60px 0;
  }
  .muted {
    color: light-dark(#666, #999);
  }
  .pair {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 20px;
  }
  .qr {
    background: white;
    padding: 12px;
    border-radius: 8px;
    border: 1px solid light-dark(#e0e0e0, #333);
  }
  .qr img {
    display: block;
  }
  .details {
    width: 100%;
    display: grid;
    grid-template-columns: 110px 1fr;
    gap: 6px 16px;
    margin: 0;
  }
  .details dt {
    color: light-dark(#666, #999);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    align-self: center;
  }
  .details dd {
    margin: 0;
    font-size: 14px;
  }
  .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .sas {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 15px;
    letter-spacing: 0.02em;
    font-weight: 500;
  }
  .instructions {
    margin: 0;
    font-size: 13px;
    color: light-dark(#444, #ccc);
    line-height: 1.5;
    text-align: center;
  }
  .countdown {
    margin: 0;
    font-size: 12px;
    color: light-dark(#666, #999);
  }
  .success,
  .expired,
  .error {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
    text-align: center;
  }
  .success h2,
  .expired h2,
  .error h2 {
    margin: 0;
    font-size: 18px;
    font-weight: 600;
  }
  .badge {
    width: 56px;
    height: 56px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    font-size: 28px;
    font-weight: 700;
  }
  .badge.ok {
    background: light-dark(#e6f7e6, #1e3a1e);
    color: light-dark(#0d6e0d, #7ad57a);
  }
  .badge.warn {
    background: light-dark(#fff7e6, #3a2e1e);
    color: light-dark(#a86200, #f3c178);
  }
  .badge.error {
    background: light-dark(#fff1f0, #401a1a);
    color: light-dark(#a8071a, #ffa39e);
  }
  .actions {
    display: flex;
    gap: 8px;
  }
</style>
