<script lang="ts">
  import type { LinkEntry } from "../links";
  import { hoverGlitch, clickScramble } from "./actions";

  let {
    link,
    animating,
    onAnimateStart,
    onAnimateEnd,
  }: {
    link: LinkEntry;
    animating: boolean;
    onAnimateStart: () => void;
    onAnimateEnd: () => void;
  } = $props();
</script>

<a
  href={link.url}
  class="text-[#c8c8c8] no-underline text-base pb-0.5 border-b border-transparent transition-colors w-fit hover:border-[#555]"
  use:hoverGlitch={{ text: link.label, enabled: !animating }}
  use:clickScramble={{
    text: link.label,
    url: link.url,
    duration: link.scrambleDuration,
    onStart: onAnimateStart,
    onEnd: onAnimateEnd,
    disabled: animating,
  }}
>
  {link.label}
</a>
