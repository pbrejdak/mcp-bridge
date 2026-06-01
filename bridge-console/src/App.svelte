<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { onOpenUrl } from "@tauri-apps/plugin-deep-link";

  import HomeView from "./lib/views/HomeView.svelte";
  import PairView from "./lib/views/PairView.svelte";

  type View = "home" | "pair";
  let view = $state<View>("home");
  // Bumped each time we return to home, so HomeView re-fetches its
  // data (in case a pair just completed and added a new pin).
  let homeNonce = $state(0);

  function openPair() {
    view = "pair";
  }
  function closePair() {
    homeNonce += 1;
    view = "home";
  }

  // mcp-bridge://pair[/...] → open the pair view. The path portion is
  // accepted for future semantics (e.g. carrying an invite token) but
  // ignored today; the pair flow always starts fresh.
  onMount(() => {
    let stopDeepLink: (() => void) | undefined;
    let stopTray: (() => void) | undefined;

    void onOpenUrl((urls) => {
      for (const url of urls) {
        if (url.toLowerCase().startsWith("mcp-bridge://pair")) {
          view = "pair";
          break;
        }
      }
    }).then((unlisten) => {
      stopDeepLink = unlisten;
    });

    // Tray menu "Pair new server" item emits `tray-action` with
    // payload "pair"; switch into the pair view from anywhere.
    void listen<string>("tray-action", (event) => {
      if (event.payload === "pair") {
        view = "pair";
      }
    }).then((unlisten) => {
      stopTray = unlisten;
    });

    return () => {
      stopDeepLink?.();
      stopTray?.();
    };
  });
</script>

{#if view === "home"}
  {#key homeNonce}
    <HomeView onpair={openPair} />
  {/key}
{:else if view === "pair"}
  <PairView onclose={closePair} />
{/if}
