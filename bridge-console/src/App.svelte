<script lang="ts">
  import { onMount } from "svelte";
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
    let stop: (() => void) | undefined;
    void onOpenUrl((urls) => {
      for (const url of urls) {
        if (url.toLowerCase().startsWith("mcp-bridge://pair")) {
          view = "pair";
          break;
        }
      }
    }).then((unlisten) => {
      stop = unlisten;
    });
    return () => stop?.();
  });
</script>

{#if view === "home"}
  {#key homeNonce}
    <HomeView onpair={openPair} />
  {/key}
{:else if view === "pair"}
  <PairView onclose={closePair} />
{/if}
