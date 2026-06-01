<script lang="ts">
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
</script>

{#if view === "home"}
  {#key homeNonce}
    <HomeView onpair={openPair} />
  {/key}
{:else if view === "pair"}
  <PairView onclose={closePair} />
{/if}

<style>
  :global(:root) {
    color-scheme: light dark;
    font-family:
      -apple-system,
      BlinkMacSystemFont,
      "Segoe UI",
      Inter,
      system-ui,
      sans-serif;
  }
  :global(body) {
    margin: 0;
    background: light-dark(#fafafa, #1a1a1a);
    color: light-dark(#1a1a1a, #f0f0f0);
  }
</style>
