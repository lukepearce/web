<script lang="ts">
  import "./style.css";
  import { links } from "./links";
  import NavLink from "./lib/NavLink.svelte";
  import { hoverGlitch } from "./lib/actions";
  import { initCursor } from "./lib/cursor";

  let animating = $state(false);

  const homePositions = [
    { x: 30, y: 28 },
    { x: 58, y: 22 },
    { x: 22, y: 52 },
    { x: 52, y: 48 },
    { x: 38, y: 74 },
  ];

  $effect(() => {
    const cleanup = initCursor();
    return cleanup;
  });
</script>

<div class="relative min-h-dvh">
  <h1
    class="absolute top-8 left-8 text-sm font-bold uppercase tracking-[0.3em] text-[#e0e0e0]"
    use:hoverGlitch={"Luke Pearce"}
  >
    Luke Pearce
  </h1>

  <nav
    class="absolute inset-0"
    class:is-animating={animating}
    style="perspective: 800px;"
  >
    {#each links as link, i (link.label)}
      <NavLink
        {link}
        {animating}
        homeX={homePositions[i].x}
        homeY={homePositions[i].y}
        onAnimateStart={() => (animating = true)}
        onAnimateEnd={() => (animating = false)}
      />
    {/each}
  </nav>
</div>
