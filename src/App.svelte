<script lang="ts">
  import "./style.css";
  import { links } from "./links";
  import NavLink from "./lib/NavLink.svelte";
  import { hoverGlitch } from "./lib/actions";

  let animating = $state(false);
</script>

<div class="flex flex-col gap-10">
  <h1
    class="text-sm font-bold uppercase tracking-[0.3em] text-[#e0e0e0]"
    use:hoverGlitch={"Luke Pearce"}
  >
    Luke Pearce
  </h1>

  <nav class="flex flex-col gap-2.5" class:is-animating={animating}>
    {#each links as link (link.label)}
      <NavLink
        {link}
        {animating}
        onAnimateStart={() => (animating = true)}
        onAnimateEnd={() => (animating = false)}
      />
    {/each}
  </nav>
</div>
